//! The screens for `--organize`: the plan, the confirmation and what happened.

use crate::core::organize::{MoveReport, MoveSkip, OrganizeBy, OrganizePlan, PlannedMove};
use crate::ui::input::read_line_from_user;
use crate::ui::style::{bold, dim, error, format_size, header, hint, plural, prompt, relative};
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// File names shown per folder before "… and N more".
const NAMES_PER_FOLDER: usize = 5;

/// Displays where each file would go, folder by folder, and what stays.
pub fn print_plan(root: &Path, plan: &OrganizePlan, by: OrganizeBy) {
    header(match by {
        OrganizeBy::Year => "Organize by year",
        OrganizeBy::Size => "Organize by size",
        OrganizeBy::Type => "Organize by type",
        OrganizeBy::Words => "Organize by words in the name",
    });

    let mut by_folder: BTreeMap<&Path, Vec<&PlannedMove>> = BTreeMap::new();
    for planned in &plan.moves {
        let folder = planned.to.parent().unwrap_or(root);
        by_folder.entry(folder).or_default().push(planned);
    }

    if by_folder.is_empty() {
        println!("\n  Nothing to organize here.");
    }
    for (folder, moves) in &by_folder {
        let bytes: u64 = moves.iter().map(|m| m.file.size_bytes).sum();
        println!(
            "\n  {}/  {} · {}",
            bold(&relative(folder, root)),
            plural(moves.len(), "file", "files"),
            format_size(bytes)
        );
        for planned in moves.iter().take(NAMES_PER_FOLDER) {
            let name = planned.to.file_name().unwrap_or_default();
            println!("    {}", name.to_string_lossy());
        }
        if moves.len() > NAMES_PER_FOLDER {
            println!(
                "    {}",
                dim(&format!("… and {} more", moves.len() - NAMES_PER_FOLDER))
            );
        }
    }

    if !plan.skipped.is_empty() {
        println!("\n  {}", bold("Staying where they are:"));
        print_skipped(root, &plan.skipped);
    }
    println!();
}

/// The y/N question before moving. True only for y or yes.
pub fn confirm_moves(count: usize) -> bool {
    println!(
        "Move the {}? Nothing is overwritten, and every move is logged so it can be undone.",
        plural(count, "file", "files")
    );
    hint("  Type y to move them, or press Enter to leave everything as it is.");
    prompt();
    let answer = read_line_from_user();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// The y/N question before undoing. True only for y or yes.
pub fn confirm_undo(count: usize) -> bool {
    println!(
        "Put back the {} organizing moved here? Nothing is overwritten.",
        plural(count, "file", "files")
    );
    hint("  Type y to put them back, or press Enter to leave everything as it is.");
    prompt();
    let answer = read_line_from_user();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

/// Displays what undoing did. What couldn't be put back stays in the log.
pub fn print_undo_report(root: &Path, report: &MoveReport) {
    println!(
        "\n{} where they were",
        bold(&format!(
            "Put back {}",
            plural(report.moved.len(), "file", "files")
        ))
    );
    if !report.skipped.is_empty() {
        println!("\n  {}", bold("Not put back (still in the log, to retry):"));
        print_skipped(root, &report.skipped);
    }
    println!();
}

/// Displays what happened. When the log couldn't be saved, every move is
/// printed instead, so there's still a record of where each file went.
pub fn print_move_report(
    root: &Path,
    report: &MoveReport,
    log: &Path,
    log_error: Option<io::Error>,
) {
    println!(
        "\n{} into folders",
        bold(&format!(
            "Moved {}",
            plural(report.moved.len(), "file", "files")
        ))
    );
    match log_error {
        None if !report.moved.is_empty() => {
            hint(&format!(
                "Every move is recorded in {}",
                relative(log, root)
            ));
        }
        None => {}
        Some(err) => {
            error(&format!(
                "Couldn't save the log at {}: {}. Where each file went:",
                log.display(),
                err
            ));
            for (from, to) in &report.moved {
                println!("  {} → {}", relative(from, root), relative(to, root));
            }
        }
    }
    if !report.skipped.is_empty() {
        println!("\n  {}", bold("Not moved:"));
        print_skipped(root, &report.skipped);
    }
    println!();
}

fn print_skipped(root: &Path, skipped: &[(PathBuf, MoveSkip)]) {
    for (path, skip) in skipped {
        let reason = match skip {
            MoveSkip::DestinationExists => {
                "a file with that name is already in the folder".to_string()
            }
            MoveSkip::SameDestination => "another file here goes to the same name".to_string(),
            MoveSkip::InProgress => "it changed moments ago, maybe still downloading".to_string(),
            MoveSkip::ChangedSinceScan => "it changed since the list was made".to_string(),
            MoveSkip::CouldNotMove(message) => format!("the system refused: {}", message),
            MoveSkip::Gone => "it's no longer where it was moved to".to_string(),
            MoveSkip::NotFromThisFolder => {
                "the log line doesn't match a move made here, so it was left alone".to_string()
            }
        };
        println!("    {}: {}", relative(path, root), dim(&reason));
    }
}
