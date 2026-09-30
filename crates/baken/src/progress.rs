use baken_core::{CancelToken, Progress};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;

fn make_progress_bar(len: usize, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(len as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{{spinner:.green}} {} [{{bar:40.cyan/blue}}] {{pos}}/{{len}}",
                label
            ))
            .unwrap()
            .progress_chars("█▓░"),
    );
    pb
}

/// Run `f` with a progress bar of `len` steps, cleared when it returns.
pub fn with_bar<T>(len: usize, label: &str, f: impl FnOnce(&dyn Progress, &CancelToken) -> T) -> T {
    let pb = make_progress_bar(len, label);
    let out = f(&BarProgress(pb.clone()), &CancelToken::new());
    pb.finish_and_clear();
    out
}

/// Adapts an indicatif bar to the core `Progress` callback.
struct BarProgress(ProgressBar);

impl Progress for BarProgress {
    fn on_file_done(&self, _done: usize, _total: usize, _file: &Path) {
        self.0.inc(1);
    }
}
