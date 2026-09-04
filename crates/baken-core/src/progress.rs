use std::path::Path;

/// Receives per-file completion events from long-running operations.
/// Files are processed in parallel, so `file` is the one that just finished,
/// not necessarily the next in input order. Called from worker threads.
pub trait Progress: Sync {
    fn on_file_done(&self, done: usize, total: usize, file: &Path);
}

/// No-op progress sink.
impl Progress for () {
    fn on_file_done(&self, _done: usize, _total: usize, _file: &Path) {}
}
