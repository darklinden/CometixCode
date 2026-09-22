//! Maps to: CC `utils/plugins/walkPluginMarkdown.ts`.

use std::future::Future;
use std::path::{Path, PathBuf};

/// Maps to the source walkPluginMarkdown options object.
#[derive(Clone, Debug, Default)]
pub struct WalkPluginMarkdownOptions {
    pub stop_at_skill_dir: bool,
    pub log_label: Option<String>,
}

/// Maps to CC `walkPluginMarkdown` and its nested `scan`.
/// Owned callback captures are the native representation of promises that may
/// outlive a rejected sibling. Dropping a JoinHandle does not cancel its task.
pub async fn walk_plugin_markdown<F, Fut>(
    root_dir: &Path,
    on_file: F,
    opts: WalkPluginMarkdownOptions,
) where
    F: Fn(PathBuf, Vec<String>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    scan(
        crate::utils::fs_operations::get_fs_implementation(),
        root_dir.to_path_buf(),
        Vec::new(),
        on_file,
        opts,
    )
    .await;
}

fn scan<F, Fut>(
    fs: std::sync::Arc<dyn crate::utils::fs_operations::FsOperations>,
    dir_path: PathBuf,
    namespace: Vec<String>,
    on_file: F,
    opts: WalkPluginMarkdownOptions,
) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>>
where
    F: Fn(PathBuf, Vec<String>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    Box::pin(async move {
        let result: anyhow::Result<()> = async {
            let entries = fs.readdir(&dir_path).await?;
            // Node Dirent already carries the type returned by readdir. Read
            // each native entry type once; never stat/follow symbolic links.
            let mut typed_entries = Vec::with_capacity(entries.len());
            for entry in entries {
                let file_type = entry.file_type()?;
                typed_entries.push((entry, file_type));
            }
            let stop = opts.stop_at_skill_dir
                && typed_entries.iter().any(|(entry, kind)| {
                    kind.is_file()
                        && entry
                            .file_name()
                            .to_string_lossy()
                            .eq_ignore_ascii_case("skill.md")
                });
            let runtime = crate::utils::process_runtime::runtime_handle_for_detached_work()
                .ok_or_else(|| anyhow::anyhow!("plugin markdown scan requires process runtime"))?;
            let mut pending = Vec::new();
            for (entry, kind) in typed_entries {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().into_owned();
                let callback = on_file.clone();
                let mut child_namespace = namespace.clone();
                if kind.is_dir() && !stop {
                    child_namespace.push(name);
                    let child_opts = opts.clone();
                    let fs = fs.clone();
                    pending.push(runtime.spawn(async move {
                        scan(fs, path, child_namespace, callback, child_opts).await;
                        Ok::<_, anyhow::Error>(())
                    }));
                } else if kind.is_file() && name.to_ascii_lowercase().ends_with(".md") {
                    pending
                        .push(runtime.spawn(async move { callback(path, child_namespace).await }));
                }
            }
            // Promise.all rejects on the first error without cancelling other
            // work. Detached sibling tasks keep their owned callback state.
            futures::future::try_join_all(
                pending
                    .into_iter()
                    .map(|task| async move { task.await.map_err(anyhow::Error::from)? }),
            )
            .await?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            crate::utils::debug::log_for_debugging_with_level(
                &format!(
                    "Failed to scan {} directory {}: {error}",
                    opts.log_label.as_deref().unwrap_or("plugin"),
                    dir_path.display()
                ),
                crate::utils::debug::DebugLogLevel::Error,
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn walker_matches_bun_skill_leaf_namespace_and_symlink_oracle() {
        crate::utils::process_runtime::initialize_test_process_runtime();
        let root = std::env::temp_dir().join(format!("plugin-walker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        for file in ["SKILL.md", "peer.MD", "sub/deep.md"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("peer.MD"), root.join("alias.md")).unwrap();
        // Actual source oracle: research/proof/plugin-command-dependencies-0916/walker-oracle.json.
        for (stop, expected) in [
            (true, vec![("SKILL.md", vec![]), ("peer.MD", vec![])]),
            (
                false,
                vec![
                    ("SKILL.md", vec![]),
                    ("peer.MD", vec![]),
                    ("sub/deep.md", vec!["sub"]),
                ],
            ),
        ] {
            let files = Arc::new(Mutex::new(Vec::new()));
            let result = files.clone();
            let base = root.clone();
            walk_plugin_markdown(
                &root,
                move |path, namespace| {
                    let files = files.clone();
                    let base = base.clone();
                    async move {
                        files.lock().unwrap().push((
                            path.strip_prefix(base)
                                .unwrap()
                                .to_string_lossy()
                                .into_owned(),
                            namespace,
                        ));
                        Ok(())
                    }
                },
                WalkPluginMarkdownOptions {
                    stop_at_skill_dir: stop,
                    ..Default::default()
                },
            )
            .await;
            let mut actual = result.lock().unwrap().clone();
            actual.sort();
            let expected = expected
                .into_iter()
                .map(|(path, ns)| {
                    (
                        path.to_string(),
                        ns.into_iter().map(str::to_string).collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rejected_callback_does_not_cancel_bun_promise_all_sibling() {
        crate::utils::process_runtime::initialize_test_process_runtime();
        let root =
            std::env::temp_dir().join(format!("plugin-walker-reject-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("fail.md"), "").unwrap();
        std::fs::write(root.join("slow.md"), "").unwrap();
        let release = Arc::new(tokio::sync::Notify::new());
        let done = Arc::new(tokio::sync::Notify::new());
        let callback_release = release.clone();
        let callback_done = done.clone();
        walk_plugin_markdown(
            &root,
            move |path, _| {
                let release = callback_release.clone();
                let done = callback_done.clone();
                async move {
                    if path.file_name().unwrap() == "fail.md" {
                        anyhow::bail!("fixture reject");
                    }
                    release.notified().await;
                    done.notify_one();
                    Ok(())
                }
            },
            Default::default(),
        )
        .await;
        // notify_one stores a permit even when the detached sibling has not
        // yet been polled; no scheduler timing assumption or blocking sleep.
        release.notify_one();
        tokio::time::timeout(std::time::Duration::from_secs(2), done.notified())
            .await
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
