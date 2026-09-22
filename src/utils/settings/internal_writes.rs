//! Maps to CC `utils/settings/internalWrites.ts`: consume a recent mark once.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

static TIMESTAMPS: LazyLock<Mutex<HashMap<PathBuf, i128>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn now() -> i128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as i128,
        Err(error) => -(error.duration().as_millis() as i128),
    }
}
pub(super) fn mark_internal_write(path: &Path) {
    TIMESTAMPS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(path.to_owned(), now());
}
pub(super) fn consume_internal_write(path: &Path, window_ms: u64) -> bool {
    consume_at(path, window_ms, now())
}
fn consume_at(path: &Path, window_ms: u64, time: i128) -> bool {
    let mut timestamps = TIMESTAMPS.lock().unwrap_or_else(|e| e.into_inner());
    if timestamps
        .get(path)
        .is_some_and(|timestamp| time - timestamp < i128::from(window_ms))
    {
        timestamps.remove(path);
        true
    } else {
        false
    }
}
pub(super) fn clear_internal_writes() {
    TIMESTAMPS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn internal_write_consumption_matches_official_window_and_single_use() {
        let path = PathBuf::from(format!("/internal-write-{}", uuid::Uuid::new_v4()));
        TIMESTAMPS.lock().unwrap().insert(path.clone(), 100);
        assert!(!consume_at(&path, 5_000, 5_100));
        // Expired marks are retained by the source; rolling the wall clock back
        // still permits a subsequent match.
        assert!(consume_at(&path, 5_000, 5_099));
        assert!(!consume_at(&path, 5_000, 5_099));
    }
}
