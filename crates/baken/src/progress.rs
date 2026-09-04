use baken_core::Progress;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;

pub fn make_progress_bar(len: usize, label: &str) -> ProgressBar {
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

/// Adapts an indicatif bar to the core `Progress` callback.
pub struct BarProgress(pub ProgressBar);

impl Progress for BarProgress {
    fn on_file_done(&self, _done: usize, _total: usize, _file: &Path) {
        self.0.inc(1);
    }
}
