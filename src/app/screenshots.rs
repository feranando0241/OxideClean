//! `--screenshots`: list the screenshots left behind after pasting them, and
//! move the chosen ones to the Trash (see ADR-0008).

use super::{AppError, TrashMode, all_moved, nothing_moved, require_terminal, threshold_of};
use crate::core::models::FileInfo;
use crate::core::screenshots::{find_screenshots, trash_screenshots};
use crate::ui::progress::with_spinner;
use crate::ui::screenshots::{
    choose_screenshots, print_screenshot_trash_report, print_screenshots,
};
use std::path::Path;
use std::time::SystemTime;

/// Shows the screenshots older than `n` days and acts according to `mode`.
/// Unattended, every one listed moves. `n` must be a valid threshold
/// (`core::old_files::days`).
pub fn handle_screenshots(
    root: &Path,
    files: &[FileInfo],
    n: u64,
    mode: TrashMode,
) -> Result<(), AppError> {
    let threshold = threshold_of(n);
    // Read the clock after the scan, so no date written during it is in the future
    let found = find_screenshots(files, SystemTime::now(), threshold);
    print_screenshots(root, &found, n, mode == TrashMode::ListOnly);

    let selected = match mode {
        TrashMode::ListOnly => return Ok(()),
        _ if found.is_empty() => return Ok(()),
        TrashMode::Unattended => found,
        TrashMode::Choose => {
            require_terminal()?;
            match choose_screenshots(&found) {
                Some(selected) => selected,
                None => return nothing_moved(),
            }
        }
    };

    let report = with_spinner(
        "Checking screenshots before moving them",
        "files",
        // A fresh clock: the user may have taken a while to choose
        |progress| trash_screenshots(&selected, SystemTime::now(), threshold, progress),
    );
    print_screenshot_trash_report(root, &report);
    all_moved(report.skipped.len())
}
