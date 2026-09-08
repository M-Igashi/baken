//! Loudness analysis and gain application (`baken headroom`).

mod analyzer;
mod processor;
mod report;
mod scanner;

pub use analyzer::{
    AudioAnalysis, GainMethod, GainMode, TpTargetMode, DEFAULT_TARGET_TRUE_PEAK, GAIN_STEP,
    HIGH_BITRATE_THRESHOLD, SPLIT_TARGET_TRUE_PEAK_HIGH, SPLIT_TARGET_TRUE_PEAK_LOW,
};
pub use processor::{create_backup_dir, ensure_backup_dir};
pub use report::{generate_csv, AnalysisSummary};
pub use scanner::{resolve_inputs, scan_audio_files, supported_extensions, BACKUP_MARKER};

use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{CancelToken, Error, Progress, Result};

/// Result of [`analyze`]: analyses in input order, plus files that could not be read.
#[derive(Debug, Default)]
pub struct AnalyzeOutcome {
    pub analyses: Vec<AudioAnalysis>,
    pub failures: Vec<(PathBuf, Error)>,
}

/// Result of [`apply`]. Files already written before a cancellation stay written.
#[derive(Debug, Default)]
pub struct ApplyOutcome {
    pub processed: usize,
    pub failures: Vec<(PathBuf, Error)>,
    pub cancelled: bool,
}

/// Measure loudness and True Peak for every file in parallel and decide the
/// gain method per file. `gain_mode` selects whether files above the ceiling
/// are lowered ([`GainMode::Normalize`]) or skipped ([`GainMode::BoostOnly`]).
/// Returns `Error::Cancelled` if the token fires; a partial analysis is not
/// useful to anyone.
pub fn analyze(
    files: &[PathBuf],
    tp_mode: TpTargetMode,
    gain_mode: GainMode,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> Result<AnalyzeOutcome> {
    let done = AtomicUsize::new(0);
    // par_iter preserves input order in the collected Vec.
    let results: Vec<Option<std::result::Result<AudioAnalysis, (PathBuf, Error)>>> = files
        .par_iter()
        .map(|file| {
            if cancel.is_cancelled() {
                return None;
            }
            let result = analyzer::analyze_file_with_target(file, tp_mode, gain_mode)
                .map_err(|e| (file.clone(), Error::from(e)));
            progress.on_file_done(done.fetch_add(1, Ordering::Relaxed) + 1, files.len(), file);
            Some(result)
        })
        .collect();

    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }

    let mut outcome = AnalyzeOutcome::default();
    for result in results.into_iter().flatten() {
        match result {
            Ok(a) => outcome.analyses.push(a),
            Err(f) => outcome.failures.push(f),
        }
    }
    Ok(outcome)
}

/// Apply the gain decided by [`analyze`] to each file in parallel, optionally
/// copying the original into `backup_dir` first (directory structure relative
/// to `base_dir` is preserved).
pub fn apply(
    analyses: &[AudioAnalysis],
    base_dir: &Path,
    backup_dir: Option<&Path>,
    progress: &dyn Progress,
    cancel: &CancelToken,
) -> ApplyOutcome {
    let done = AtomicUsize::new(0);
    let results: Vec<Option<std::result::Result<(), (PathBuf, Error)>>> = analyses
        .par_iter()
        .map(|analysis| {
            if cancel.is_cancelled() {
                return None;
            }
            let result = processor::process_file(analysis, base_dir, backup_dir)
                .map_err(|e| (analysis.path.clone(), Error::from(e)));
            progress.on_file_done(
                done.fetch_add(1, Ordering::Relaxed) + 1,
                analyses.len(),
                &analysis.path,
            );
            Some(result)
        })
        .collect();

    let mut outcome = ApplyOutcome {
        cancelled: cancel.is_cancelled(),
        ..Default::default()
    };
    for result in results.into_iter().flatten() {
        match result {
            Ok(()) => outcome.processed += 1,
            Err(f) => outcome.failures.push(f),
        }
    }
    outcome
}

/// Keep the analyses that need a gain change and whose method is enabled.
pub fn select_processable(
    analyses: &[AudioAnalysis],
    lossless: bool,
    reencode: bool,
) -> Vec<AudioAnalysis> {
    analyses
        .iter()
        .filter(|a| {
            a.needs_gain()
                && if a.requires_reencode() {
                    reencode
                } else {
                    lossless
                }
        })
        .cloned()
        .collect()
}

/// Deepest directory containing every file, used as the backup root.
pub fn common_base_dir(files: &[PathBuf]) -> Option<PathBuf> {
    let mut iter = files
        .iter()
        .filter_map(|f| f.parent().map(Path::to_path_buf));
    let first = iter.next()?;
    Some(iter.fold(first, |acc, p| common_prefix(&acc, &p)))
}

fn common_prefix(a: &Path, b: &Path) -> PathBuf {
    a.components()
        .zip(b.components())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x)
        .collect()
}
