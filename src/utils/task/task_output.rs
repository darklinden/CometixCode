//! Single owner for shell command output.
//!
//! Maps to: CC `utils/task/TaskOutput.ts:1-319`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const DEFAULT_MAX_MEMORY: usize = 8 * 1024 * 1024;
const PROGRESS_TAIL_BYTES: usize = 4096;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TaskOutputProgress {
    pub last_lines: String,
    pub all_lines: String,
    pub total_lines: usize,
    pub total_bytes: u64,
    pub is_incomplete: bool,
}

#[derive(Debug)]
struct TaskOutputInner {
    task_id: String,
    path: PathBuf,
    stdout_to_file: bool,
    stdout_buffer: Mutex<String>,
    stderr_buffer: Mutex<String>,
    total_lines: AtomicUsize,
    total_bytes: AtomicU64,
    max_memory: usize,
    overflowed: AtomicBool,
    output_file_redundant: AtomicBool,
    output_file_size: AtomicU64,
}

/// Maps to CC `TaskOutput`.
#[derive(Clone, Debug)]
pub struct TaskOutput {
    inner: Arc<TaskOutputInner>,
}

impl TaskOutput {
    /// Maps to CC `new TaskOutput(taskId, onProgress, stdoutToFile, maxMemory)`.
    /// Progress callbacks remain with BashTool's generator in Rust; this owner
    /// exposes the same one-second tail snapshot through [`poll_progress`].
    pub fn new(task_id: impl Into<String>, stdout_to_file: bool) -> Self {
        let task_id = task_id.into();
        Self {
            inner: Arc::new(TaskOutputInner {
                path: crate::utils::task::disk_output::get_task_output_path(&task_id),
                task_id,
                stdout_to_file,
                stdout_buffer: Mutex::new(String::new()),
                stderr_buffer: Mutex::new(String::new()),
                total_lines: AtomicUsize::new(0),
                total_bytes: AtomicU64::new(0),
                max_memory: DEFAULT_MAX_MEMORY,
                overflowed: AtomicBool::new(false),
                output_file_redundant: AtomicBool::new(false),
                output_file_size: AtomicU64::new(0),
            }),
        }
    }

    pub fn task_id(&self) -> &str {
        &self.inner.task_id
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn stdout_to_file(&self) -> bool {
        self.inner.stdout_to_file
    }

    /// Maps to CC `writeStdout(data)` (pipe mode).
    pub fn write_stdout(&self, data: &[u8]) {
        self.write_buffered(data, false);
    }

    /// Maps to CC `writeStderr(data)` (pipe mode).
    pub fn write_stderr(&self, data: &[u8]) {
        self.write_buffered(data, true);
    }

    fn write_buffered(&self, data: &[u8], is_stderr: bool) {
        let text = String::from_utf8_lossy(data);
        self.inner
            .total_bytes
            .fetch_add(data.len() as u64, Ordering::Relaxed);
        self.inner.total_lines.fetch_add(
            text.bytes().filter(|byte| *byte == b'\n').count(),
            Ordering::Relaxed,
        );

        if self.inner.overflowed.load(Ordering::Acquire) {
            if crate::utils::session_storage::is_session_write_enabled() {
                let content = if is_stderr {
                    format!("[stderr] {text}")
                } else {
                    text.into_owned()
                };
                let _ =
                    crate::utils::task::disk_output::append_task_output(self.task_id(), &content);
            }
            return;
        }

        let stdout_len = self
            .inner
            .stdout_buffer
            .lock()
            .map(|buffer| buffer.len())
            .unwrap_or_default();
        let stderr_len = self
            .inner
            .stderr_buffer
            .lock()
            .map(|buffer| buffer.len())
            .unwrap_or_default();
        if stdout_len + stderr_len + text.len() > self.inner.max_memory {
            if crate::utils::session_storage::is_session_write_enabled() {
                self.spill_to_disk();
                self.write_buffered(data, is_stderr);
                return;
            }

            // The explicit no-write seam cannot spill. Keep a bounded UTF-8
            // prefix rather than turning an untrusted command into unbounded
            // process memory growth.
            let remaining = self
                .inner
                .max_memory
                .saturating_sub(stdout_len + stderr_len);
            let mut end = text.len().min(remaining);
            while end > 0 && !text.is_char_boundary(end) {
                end -= 1;
            }
            let buffer = if is_stderr {
                &self.inner.stderr_buffer
            } else {
                &self.inner.stdout_buffer
            };
            if let Ok(mut buffer) = buffer.lock() {
                buffer.push_str(&text[..end]);
            }
            self.inner.overflowed.store(true, Ordering::Release);
            return;
        }

        let buffer = if is_stderr {
            &self.inner.stderr_buffer
        } else {
            &self.inner.stdout_buffer
        };
        if let Ok(mut buffer) = buffer.lock() {
            buffer.push_str(&text);
        }
    }

    /// Maps to CC `spillToDisk()`.
    pub fn spill_to_disk(&self) {
        if self.inner.overflowed.swap(true, Ordering::AcqRel) {
            return;
        }
        let stdout = self
            .inner
            .stdout_buffer
            .lock()
            .map(|mut value| std::mem::take(&mut *value))
            .unwrap_or_default();
        let stderr = self
            .inner
            .stderr_buffer
            .lock()
            .map(|mut value| std::mem::take(&mut *value))
            .unwrap_or_default();
        if !stdout.is_empty() {
            let _ = crate::utils::task::disk_output::append_task_output(self.task_id(), &stdout);
        }
        if !stderr.is_empty() {
            let _ = crate::utils::task::disk_output::append_task_output(
                self.task_id(),
                &format!("[stderr] {stderr}"),
            );
        }
    }

    /// One shared-poller tick equivalent. Maps to CC `TaskOutput.#tick()`.
    pub fn poll_progress(&self) -> TaskOutputProgress {
        if !self.stdout_to_file() {
            let stdout = self
                .inner
                .stdout_buffer
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            let stderr = self
                .inner
                .stderr_buffer
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            let full = format!("{stdout}{stderr}");
            return progress_from_content(
                &full,
                self.inner.total_lines.load(Ordering::Relaxed),
                self.inner.total_bytes.load(Ordering::Relaxed),
                self.inner.overflowed.load(Ordering::Acquire),
            );
        }

        let Ok(result) = futures::executor::block_on(crate::utils::fs_operations::tail_file(
            self.path(),
            PROGRESS_TAIL_BYTES,
        )) else {
            return TaskOutputProgress::default();
        };
        let crate::utils::fs_operations::ReadFileRangeResult {
            content,
            bytes_read,
            bytes_total,
        } = result;
        if content.is_empty() {
            return TaskOutputProgress {
                total_lines: self.inner.total_lines.load(Ordering::Relaxed),
                total_bytes: bytes_total,
                ..Default::default()
            };
        }
        let sampled_lines = content.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let exact_or_estimated = if bytes_read as u64 >= bytes_total {
            sampled_lines
        } else {
            let estimate = ((bytes_total as f64 / bytes_read.max(1) as f64) * sampled_lines as f64)
                .round() as usize;
            estimate.max(self.inner.total_lines.load(Ordering::Relaxed))
        };
        self.inner
            .total_lines
            .store(exact_or_estimated, Ordering::Relaxed);
        self.inner.total_bytes.store(bytes_total, Ordering::Relaxed);
        progress_from_content(
            &content,
            exact_or_estimated,
            bytes_total,
            (bytes_read as u64) < bytes_total,
        )
    }

    /// Maps to CC `getStdout()`.
    pub fn get_stdout(&self) -> String {
        if self.stdout_to_file() {
            return self.read_stdout_from_file();
        }
        if self.inner.overflowed.load(Ordering::Acquire) {
            let size_kb = self
                .inner
                .total_bytes
                .load(Ordering::Relaxed)
                .div_ceil(1024);
            if !crate::utils::session_storage::is_session_write_enabled() {
                let stdout = self
                    .inner
                    .stdout_buffer
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                let stderr = self
                    .inner
                    .stderr_buffer
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_default();
                return format!(
                    "{stdout}{stderr}\nOutput truncated ({size_kb}KB total; persistence is disabled)."
                );
            }
            let output = crate::utils::task::disk_output::get_task_output(self.task_id(), 4096);
            let notice = format!(
                "\nOutput truncated ({size_kb}KB total). Full output saved to: {}",
                self.path().display()
            );
            return if output.is_empty() {
                notice.trim_start().to_string()
            } else {
                format!("{output}{notice}")
            };
        }
        self.inner
            .stdout_buffer
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    fn read_stdout_from_file(&self) -> String {
        let max = crate::utils::shell::output_limits::get_max_output_length();
        match futures::executor::block_on(crate::utils::fs_operations::read_file_range(
            self.path(),
            0,
            max,
        )) {
            Ok(Some(crate::utils::fs_operations::ReadFileRangeResult {
                content,
                bytes_read,
                bytes_total,
            })) => {
                self.inner
                    .output_file_size
                    .store(bytes_total, Ordering::Release);
                self.inner
                    .output_file_redundant
                    .store(bytes_total <= bytes_read as u64, Ordering::Release);
                content
            }
            Ok(None) => {
                self.inner
                    .output_file_redundant
                    .store(true, Ordering::Release);
                String::new()
            }
            Err(error) => format!(
                "<bash output unavailable: output file {} could not be read ({}). This usually means another Claude Code process in the same project deleted it during startup cleanup.>",
                self.path().display(),
                error
                    .raw_os_error()
                    .map_or_else(|| "unknown".to_string(), |code| code.to_string())
            ),
        }
    }

    /// Maps to CC `getStderr()`.
    pub fn get_stderr(&self) -> String {
        if self.inner.overflowed.load(Ordering::Acquire) {
            return String::new();
        }
        self.inner
            .stderr_buffer
            .lock()
            .map(|value| value.clone())
            .unwrap_or_default()
    }

    pub fn output_file_redundant(&self) -> bool {
        self.inner.output_file_redundant.load(Ordering::Acquire)
    }

    pub fn output_file_size(&self) -> u64 {
        self.inner.output_file_size.load(Ordering::Acquire)
    }

    pub fn is_overflowed(&self) -> bool {
        self.inner.overflowed.load(Ordering::Acquire)
    }

    /// Maps to CC `flush()`.
    pub async fn flush(&self) -> std::io::Result<()> {
        crate::utils::task::disk_output::flush_task_output(self.task_id()).await
    }

    /// Maps to CC `deleteOutputFile()`.
    pub fn delete_output_file(&self) {
        let _ = crate::utils::task::disk_output::cleanup_task_output(self.task_id());
    }

    /// Maps to CC `clear()`; output-file deletion remains the caller's explicit
    /// `deleteOutputFile` decision.
    pub fn clear(&self) {
        if let Ok(mut stdout) = self.inner.stdout_buffer.lock() {
            stdout.clear();
        }
        if let Ok(mut stderr) = self.inner.stderr_buffer.lock() {
            stderr.clear();
        }
    }
}

fn recent_line_slice(content: &str, count: usize) -> String {
    let bytes = content.as_bytes();
    let mut position = bytes.len();
    let mut lines = 0usize;
    let mut start = 0usize;
    while position > 0 {
        let found = bytes[..position].iter().rposition(|byte| *byte == b'\n');
        lines += 1;
        match found {
            Some(index) => {
                position = index;
                if lines == count {
                    start = if index == 0 { 0 } else { index + 1 };
                    break;
                }
            }
            None => {
                start = 0;
                break;
            }
        }
    }
    content[start..].to_string()
}

fn progress_from_content(
    content: &str,
    total_lines: usize,
    total_bytes: u64,
    incomplete: bool,
) -> TaskOutputProgress {
    TaskOutputProgress {
        last_lines: recent_line_slice(content, 5),
        all_lines: recent_line_slice(content, 100),
        total_lines,
        total_bytes,
        is_incomplete: incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_mode_reports_prefix_and_progress_tail_like_official() {
        let task_id = format!("task-{}", uuid::Uuid::new_v4().simple());
        let output = TaskOutput::new(&task_id, true);
        std::fs::create_dir_all(output.path().parent().unwrap()).unwrap();
        std::fs::write(output.path(), "one\ntwo\nthree\nfour\nfive\nsix\n").unwrap();
        let progress = output.poll_progress();
        assert_eq!(progress.last_lines, "three\nfour\nfive\nsix\n");
        assert_eq!(progress.total_bytes, 28);
        assert_eq!(output.get_stdout(), "one\ntwo\nthree\nfour\nfive\nsix\n");
        assert!(output.output_file_redundant());
        output.delete_output_file();
    }

    #[cfg(unix)]
    #[test]
    fn file_mode_read_and_progress_matches_official_symlink_following() {
        use std::os::unix::fs::symlink;

        let task_id = format!("task-symlink-{}", uuid::Uuid::new_v4().simple());
        let output = TaskOutput::new(&task_id, true);
        std::fs::create_dir_all(output.path().parent().unwrap()).unwrap();
        let victim = std::env::temp_dir().join(format!(
            "cometix-task-output-victim-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&victim, "secret-must-not-be-read-or-changed").unwrap();
        symlink(&victim, output.path()).unwrap();

        let content = output.get_stdout();
        // CC TaskOutput.ts:114/300 use raw tailFile/readFileRange, which follow links.
        assert_eq!(content, "secret-must-not-be-read-or-changed");
        assert_eq!(output.poll_progress().total_bytes, content.len() as u64);
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            "secret-must-not-be-read-or-changed"
        );
        output.delete_output_file();
        let _ = std::fs::remove_file(victim);
    }

    #[test]
    fn pipe_mode_keeps_stdout_and_stderr_separate() {
        let output = TaskOutput::new("pipe-test", false);
        output.write_stdout(b"out\n");
        output.write_stderr(b"err\n");
        assert_eq!(output.get_stdout(), "out\n");
        assert_eq!(output.get_stderr(), "err\n");
    }
}
