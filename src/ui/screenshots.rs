//! The screens for screenshots (see ADR-0008): the list, choosing which to
//! move to the Trash, and what happened.

use crate::core::screenshots::{Detected, Screenshot, ScreenshotSkip, ScreenshotTrashReport};
use crate::ui::input::{is_yes, pick, read_line_from_user};
use crate::ui::report::{print_trash_outcome, system_refused};
use crate::ui::style::{
    bold, dim, error, format_age, format_size, header, hint, plural, prompt, relative, rule,
    size_cell, truncate, yellow,
};
use std::path::Path;

/// Max characters shown for a screenshot's path.
const PATH_W: usize = 52;

/// Displays the screenshots found, numbered as `choose_screenshots` reads them.
pub fn print_screenshots(root: &Path, found: &[Screenshot], days: u64, listing_only: bool) {
    header(&format!(
        "Screenshots older than {}",
        plural(days as usize, "day", "days")
    ));
    if found.is_empty() {
        println!("\n  None found.\n");
        return;
    }

    println!(
        "\n{}",
        dim(&format!(
            "{:<4} {:<PATH_W$} {:>10}  {:>9}",
            "#", "FILE", "SIZE", "AGE"
        ))
    );
    for (i, shot) in found.iter().enumerate() {
        println!(
            "{:<4} {:<PATH_W$} {}  {:>9}{}",
            i + 1,
            truncate(&relative(&shot.file.path, root), PATH_W),
            size_cell(shot.file.size_bytes),
            format_age(shot.age),
            if shot.detected == Detected::Name {
                dim(" (name)")
            } else {
                String::new()
            }
        );
    }
    let total: u64 = found.iter().map(|s| s.file.size_bytes).sum();
    println!();
    rule();
    println!(
        "{} · {} in total",
        plural(found.len(), "screenshot", "screenshots"),
        yellow(&bold(&format_size(total)))
    );
    if found.iter().any(|s| s.detected == Detected::Name) {
        hint("(name) Found only by its name: macOS didn't mark it, so check it's really one.");
    }
    if listing_only {
        hint("Nothing was moved or changed. Add --trash to choose which to move.");
    }
    println!();
}

/// Asks which screenshots to move. Returns them, or `None` to leave
/// everything as it is.
pub fn choose_screenshots(found: &[Screenshot]) -> Option<Vec<Screenshot>> {
    loop {
        println!("\n{}", bold("What would you like to do?"));
        println!(
            "  • Type y        to move all {}",
            plural(found.len(), "screenshot", "screenshots")
        );
        println!("  • Type numbers  to move only those (e.g. 1,3,5-7)");
        println!("  • Press Enter   to leave everything as it is");
        hint("  They can be restored from the Trash. Space comes back when you empty it.");
        prompt();

        let answer = read_line_from_user();
        if answer.is_empty() {
            return None;
        }
        if is_yes(&answer) {
            return Some(found.to_vec());
        }
        match pick(&answer, found) {
            Ok(selected) => return Some(selected),
            Err(message) => error(&format!("{} Or press Enter to leave them.", message)),
        }
    }
}

/// Displays what happened when moving screenshots to the Trash.
pub fn print_screenshot_trash_report(root: &Path, report: &ScreenshotTrashReport) {
    print_trash_outcome(
        root,
        report,
        "screenshot",
        "screenshots",
        |skip| match skip {
            ScreenshotSkip::ChangedSinceScan => "it changed since the list was made".to_string(),
            ScreenshotSkip::CouldNotMove(message) => system_refused(message),
        },
    );
}
