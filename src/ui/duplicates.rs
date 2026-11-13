//! The screens for identical files: the groups, the choices after they are
//! shown (move all of them, only some groups, or first pick a folder whose
//! copies should be kept), and what happened.

use crate::core::duplicates::trash::{SkipReason, TrashReport};
use crate::core::duplicates::{
    DuplicateGroup, extra_copies_in, keep_copies_inside, wasted_bytes_in,
};
use crate::ui::input::{confirm, is_one_of, pick, read_line_from_user, resolve_user_folder};
use crate::ui::style::{
    bold, dim, error, format_size, green, header, hint, plural, prompt, red, relative, rule, yellow,
};
use std::path::Path;

/// Displays exact duplicates verified by hash and the recoverable space.
pub fn print_exact_duplicates(root: &Path, groups: &[DuplicateGroup]) {
    if groups.is_empty() {
        println!("\nNo exact duplicate files found.");
        return;
    }

    header("Identical files");
    hint("These files have exactly the same content. In each group the oldest");
    hint("copy is kept (or every copy inside the folder you pick); the extra");
    hint("copies can be moved to the Trash.");

    for (i, group) in groups.iter().enumerate() {
        print_group(root, i + 1, group);
    }

    println!();
    rule();
    println!(
        "{} in {} · {} can be freed\n",
        plural(extra_copies_in(groups), "extra copy", "extra copies"),
        plural(groups.len(), "group", "groups"),
        yellow(&bold(&format_size(wasted_bytes_in(groups))))
    );
}

/// One group: what it is, then every path, marked as kept or extra.
fn print_group(root: &Path, number: usize, group: &DuplicateGroup) {
    println!(
        "\n{}  {}",
        bold(&format!("Group {}", number)),
        dim(&format!(
            "{} · {} each · {} can be freed",
            group.original().name(),
            format_size(group.file_size),
            format_size(group.wasted_bytes())
        ))
    );
    // Copies in age order; hardlinks of one copy share its label
    for (index, copy) in group.copies.iter().enumerate() {
        let label = if group.is_kept(index) {
            green("keep ")
        } else {
            red("extra")
        };
        for path in &copy.paths {
            println!("  {}  {}", label, relative(&path.path, root));
        }
    }
}

/// The single y/N question for `--trash` without `--yes`.
pub fn confirm_trash_all(groups: &[DuplicateGroup]) -> bool {
    let copies = extra_copies_in(groups);
    confirm(
        &format!(
            "Move the {} to the Trash? You can restore {} from there later.",
            plural(copies, "extra copy", "extra copies"),
            if copies == 1 { "it" } else { "them" }
        ),
        "  Type y to move them, or press Enter to leave everything as it is.",
    )
}

/// Asks what to do with `groups` until the user decides. Returns the groups
/// to move to the Trash, or `None` to leave everything as it is. Choosing a
/// folder to keep (`k <folder>`) updates `groups` and shows them again.
pub fn choose_groups_to_trash(
    root: &Path,
    groups: &mut [DuplicateGroup],
) -> Option<Vec<DuplicateGroup>> {
    loop {
        println!("\n{}", bold("What would you like to do?"));
        println!("  • Type y              to move every extra copy to the Trash");
        println!("  • Type group numbers  to move only those groups (e.g. 1,3,5-7)");
        println!("  • Type k <folder>     to keep the copies inside a folder (e.g. k Photos)");
        println!("  • Press Enter         to leave everything as it is");
        hint("  Moved files can be restored from the Trash.");
        prompt();

        let answer = read_line_from_user();
        if answer.is_empty() {
            return None;
        }
        if is_one_of(&answer, &["y", "yes", "all"]) {
            return Some(groups.to_vec());
        }
        if let Some(folder) = answer.strip_prefix("k ").map(str::trim) {
            keep_copies_in(root, groups, folder);
            continue;
        }

        match pick(&answer, groups) {
            Ok(selected) => {
                hint(&format!(
                    "Moving {} from {}.",
                    plural(extra_copies_in(&selected), "extra copy", "extra copies"),
                    plural(selected.len(), "group", "groups")
                ));
                return Some(selected);
            }
            Err(message) => error(&format!(
                "{} Type y, group numbers, k <folder>, or press Enter.",
                message
            )),
        }
    }
}

/// Switches every group that has copies inside `folder` to keep all of them,
/// then shows the groups again so the user sees what changed.
fn keep_copies_in(root: &Path, groups: &mut [DuplicateGroup], folder: &str) {
    let path = resolve_user_folder(root, folder);
    if !path.is_dir() {
        error(&format!("'{}' is not a folder here.", folder));
        return;
    }
    let found = keep_copies_inside(groups, &path);
    if found == 0 {
        error(&format!(
            "No group has a copy inside '{}'. Nothing changed.",
            folder
        ));
        return;
    }
    print_exact_duplicates(root, groups);
    hint(&kept_inside_note(folder, found, groups.len()));
}

/// The line that says how many groups now keep their copies in `folder`.
pub fn kept_inside_note(folder: &str, found: usize, groups: usize) -> String {
    format!(
        "Keeping every copy inside '{}' in {} of {}.",
        folder,
        found,
        plural(groups, "group", "groups")
    )
}

/// Displays what happened when moving duplicates to the Trash.
pub fn print_trash_report(root: &Path, report: &TrashReport) {
    println!(
        "\n{} moved to the Trash · {} freed",
        bold(&plural(report.moved, "file", "files")),
        bold(&format_size(report.freed_bytes))
    );
    if report.moved > 0 {
        hint("Changed your mind? You can restore them from the Trash.");
    }

    if !report.failed.is_empty() {
        let title = format!(
            "{} not moved:",
            plural(report.failed.len(), "file was", "files were")
        );
        println!("\n{}", yellow(&bold(&title)));
        for (path, reason) in &report.failed {
            let why = match reason {
                SkipReason::KeptCopyChanged(kept) => format!(
                    "skipped: the kept copy '{}' is missing or changed since the scan",
                    relative(kept, root)
                ),
                SkipReason::ChangedSinceScan => {
                    "skipped: missing or changed since the scan".to_string()
                }
                SkipReason::CouldNotMove(err) => format!("could not be moved: {}", err),
            };
            println!("  {} {}", relative(path, root), dim(&format!("— {}", why)));
        }
    }
    println!();
}
