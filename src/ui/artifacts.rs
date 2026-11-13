//! The screens for build folders (see ADR-0005): the list, choosing which to
//! move to the Trash, and what happened.

use crate::core::artifact_trash::{ArtifactSkip, ArtifactTrashReport};
use crate::core::artifacts::{Artifact, ArtifactKind, Artifacts, Confidence, ProjectState};
use crate::ui::input::{is_yes, pick, read_line_from_user};
use crate::ui::report::{print_trash_outcome, system_refused};
use crate::ui::style::{
    bold, dim, error, format_age, format_size, header, hint, plural, prompt, relative, rule,
    size_cell, truncate, yellow,
};
use std::path::Path;

/// Max characters shown for a project in the table.
const PROJECT_W: usize = 34;

/// Displays the artifacts found, how to rebuild them, and what was left alone.
pub fn print_artifacts(root: &Path, report: &Artifacts, days: u64, all: bool, listing_only: bool) {
    if all {
        header("Build folders in every project");
    } else {
        header(&format!(
            "Build folders in projects untouched for {}+ days",
            days
        ));
    }

    if report.found.is_empty() {
        println!("\n  None found.");
    } else {
        print_table(root, &report.found, all);
        print_totals(report, days);
    }
    print_notes(report, days, listing_only);
}

/// One row per build folder, numbered as `choose_artifacts` reads them: only
/// what can be moved gets a number. `all` adds the STATE column.
fn print_table(root: &Path, found: &[Artifact], all: bool) {
    println!(
        "\n{}",
        dim(&format!(
            "{:<4} {:<PROJECT_W$} {:<6} {:>10}  {:>10}  {}{}",
            "#",
            "PROJECT",
            "TYPE",
            "SIZE",
            "UNTOUCHED",
            if all { "STATE      " } else { "" },
            "REBUILD WITH"
        ))
    );
    let mut number = 0;
    for artifact in found {
        let label = if artifact.can_move() {
            number += 1;
            number.to_string()
        } else {
            "-".to_string()
        };
        println!(
            "{:<4} {:<PROJECT_W$} {:<6} {}  {:>10}  {}{}{}{}",
            label,
            truncate(&relative(&artifact.project, root), PROJECT_W),
            kind_name(artifact.kind),
            size_cell(artifact.size.bytes),
            artifact.untouched.map_or("?".to_string(), format_age),
            if all { state_cell(artifact.state) } else { "" },
            dim(artifact.rebuild.unwrap_or("nothing records the packages")),
            if artifact.confidence == Confidence::Pinned {
                ""
            } else {
                " (!)"
            },
            if artifact.built_programs > 0 {
                " (bin)"
            } else {
                ""
            }
        );
    }
}

fn kind_name(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Rust => "Rust",
        ArtifactKind::Node => "Node",
        ArtifactKind::Python => "Python",
    }
}

/// The STATE column, padded to the width of its title.
fn state_cell(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Forgotten => "forgotten  ",
        ProjectState::Active => "active     ",
        ProjectState::NotJudged => "unknown    ",
    }
}

/// How much the listed folders add up to, and how much of it is in projects
/// in use.
fn print_totals(report: &Artifacts, days: u64) {
    println!();
    rule();
    println!(
        "{} · {} in total",
        plural(report.found.len(), "build folder", "build folders"),
        yellow(&bold(&format_size(report.total.bytes)))
    );
    let active_bytes: u64 = report
        .found
        .iter()
        .filter(|a| a.state == ProjectState::Active)
        .map(|a| a.size.bytes)
        .sum();
    if active_bytes > 0 {
        println!(
            "{} of that is in active projects (used, or arrived here, in the last {}).",
            format_size(active_bytes),
            plural(days as usize, "day", "days")
        );
    }
}

/// Never drop anything silently: explains each mark used in the list, and
/// says what was left out and why.
fn print_notes(report: &Artifacts, days: u64, listing_only: bool) {
    let any = |is: fn(&Artifact) -> bool| report.found.iter().any(is);

    println!();
    if any(|a| a.confidence != Confidence::Pinned) {
        hint("(!) No exact record of versions: a rebuild may not give the same result.");
    }
    if any(|a| !a.can_move()) {
        hint("- Can't be moved to the Trash: a rebuild can't be relied on.");
    }
    if any(|a| a.built_programs > 0) {
        hint("(bin) target/release holds programs: only cargo build --release brings them back.");
    }
    if any(|a| a.unreadable > 0) {
        hint("Some folders couldn't be read completely, so their real size may be larger.");
    }
    if any(|a| a.state == ProjectState::Active) {
        hint("Active projects: a build or server running there can break if its folder goes away.");
    }
    if report.active > 0 {
        hint(&format!(
            "{} left alone: {} project was used, or arrived here, in the last {}.",
            plural(report.active, "build folder", "build folders"),
            if report.active == 1 { "its" } else { "their" },
            plural(days as usize, "day", "days")
        ));
    }
    if report.not_judged > 0 {
        hint(&format!(
            "{} left alone: a project file has no date, or one in the future.",
            plural(report.not_judged, "build folder", "build folders")
        ));
    }
    if report.unverified > 0 {
        hint(&format!(
            "{} beside a Cargo.toml left alone: no sign that Cargo made it.",
            plural(report.unverified, "target folder", "target folders")
        ));
    }
    if !report.found.is_empty() {
        hint("Rebuilding can fail on old projects if a package or tool has moved on.");
    }
    if listing_only {
        hint("Nothing was moved or changed.");
    }
    println!();
}

/// Asks which build folders to move. Returns them, or `None` to leave
/// everything as it is. Anything not clear-cut has to be picked by number and
/// confirmed, so a quick `y` can't reach an active project.
pub fn choose_artifacts(report: &Artifacts) -> Option<Vec<Artifact>> {
    let movable: Vec<Artifact> = report
        .found
        .iter()
        .filter(|a| a.can_move())
        .cloned()
        .collect();
    let clear_cut: Vec<Artifact> = movable
        .iter()
        .filter(|a| a.moves_unattended())
        .cloned()
        .collect();
    loop {
        println!("\n{}", bold("What would you like to do?"));
        if !clear_cut.is_empty() {
            println!(
                "  • Type y        to move the {} from forgotten projects",
                plural(clear_cut.len(), "folder", "folders")
            );
        }
        println!("  • Type numbers  to move only those (e.g. 1,3,5-7)");
        println!("  • Press Enter   to leave everything as it is");
        hint("  Moved folders can be restored from the Trash. Space comes back when you empty it.");
        prompt();

        let answer = read_line_from_user();
        if answer.is_empty() {
            return None;
        }
        if is_yes(&answer) {
            if clear_cut.is_empty() {
                error("Nothing can move without being chosen. Type numbers, or press Enter.");
                continue;
            }
            return Some(clear_cut);
        }
        match pick(&answer, &movable) {
            Ok(selected) => return confirm_if_delicate(selected),
            Err(message) => error(&format!("{} Or press Enter to leave them.", message)),
        }
    }
}

/// Asks once more if a selection includes something not clear-cut.
fn confirm_if_delicate(selected: Vec<Artifact>) -> Option<Vec<Artifact>> {
    let delicate = selected.iter().filter(|a| !a.moves_unattended()).count();
    if delicate == 0 {
        return Some(selected);
    }
    println!(
        "\n{} active, or holding programs in target/release.",
        plural(delicate, "folder is", "folders are")
    );
    println!("A build or server running there can break. Type yes to move them anyway.");
    prompt();
    read_line_from_user()
        .eq_ignore_ascii_case("yes")
        .then_some(selected)
}

/// Displays what happened when moving build folders to the Trash.
pub fn print_artifact_trash_report(root: &Path, report: &ArtifactTrashReport) {
    print_trash_outcome(
        root,
        report,
        "build folder",
        "build folders",
        |skip| match skip {
            ArtifactSkip::NotMovable => "a rebuild can't be relied on".to_string(),
            ArtifactSkip::NoLongerABuildFolder => {
                "it no longer looks like a build folder".to_string()
            }
            ArtifactSkip::ProjectInUse => "the project has been used since the list".to_string(),
            ArtifactSkip::RecentlyChanged => {
                "it changed moments ago, maybe a build is running".to_string()
            }
            ArtifactSkip::CouldNotMove(message) => system_refused(message),
        },
    );
}
