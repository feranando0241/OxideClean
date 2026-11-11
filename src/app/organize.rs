//! `--organize`: plan moving the loose files of a folder into subfolders,
//! and move them only after a confirmation (or `--yes`).

use super::{AppError, require_terminal};
use crate::core::organize::{
    LOG_NAME, OrganizeBy, append_log, apply_plan, logged_moves, plan_organize, undo_organize,
};
use crate::core::scanner::scan_loose_files;
use crate::ui::organize::{
    confirm_moves, confirm_undo, print_move_report, print_plan, print_undo_report,
};
use crate::ui::progress::Spinner;
use crate::ui::style::hint;
use std::path::Path;
use std::time::SystemTime;

/// Shows the plan for `root`; with `apply`, moves the files and logs each
/// move in `root/LOG_NAME`. Only the files directly inside `root` are read,
/// so this doesn't scan the whole tree.
pub fn handle_organize(
    root: &Path,
    by: OrganizeBy,
    apply: bool,
    assume_yes: bool,
) -> Result<(), AppError> {
    let files = scan_loose_files(root).map_err(AppError::Scan)?;
    let plan = plan_organize(root, &files, by, SystemTime::now());
    print_plan(root, &plan, by);

    if plan.moves.is_empty() {
        return Ok(());
    }
    if !apply {
        hint("Nothing was moved. Run again with --apply to move them.\n");
        return Ok(());
    }
    if !assume_yes {
        require_terminal()?;
        if !confirm_moves(plan.moves.len()) {
            println!("Nothing was moved.\n");
            return Ok(());
        }
    }

    let report = {
        let spinner = Spinner::start("Moving files", "files");
        // A fresh clock: the user may have taken a while to answer
        apply_plan(&plan, SystemTime::now(), &|done, total| {
            spinner.update(done, total)
        })
    };
    let log_error = if report.moved.is_empty() {
        None
    } else {
        append_log(root, &report.moved).err()
    };
    print_move_report(root, &report, &root.join(LOG_NAME), log_error);

    if report.skipped.is_empty() {
        Ok(())
    } else {
        Err(AppError::NotAllMoved(report.skipped.len()))
    }
}

/// Puts back every file the log of `root` records, after a y/N question
/// unless `assume_yes`.
pub fn handle_undo(root: &Path, assume_yes: bool) -> Result<(), AppError> {
    let log = root.join(LOG_NAME);
    let unreadable = |source| AppError::UnreadableFile {
        path: log.clone(),
        source,
    };
    let count = logged_moves(root).map_err(unreadable)?;
    if count == 0 {
        println!("Nothing to undo: no files were organized in this folder.\n");
        return Ok(());
    }
    if !assume_yes {
        require_terminal()?;
        if !confirm_undo(count) {
            println!("Nothing was moved.\n");
            return Ok(());
        }
    }

    let report = {
        let spinner = Spinner::start("Putting files back", "files");
        undo_organize(root, &|done, total| spinner.update(done, total)).map_err(unreadable)?
    };
    print_undo_report(root, &report);
    if report.skipped.is_empty() {
        Ok(())
    } else {
        Err(AppError::NotAllMoved(report.skipped.len()))
    }
}
