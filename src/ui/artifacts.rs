//! The screen for build artifacts in forgotten projects (see ADR-0005).

use crate::core::artifacts::{ArtifactKind, Artifacts, Confidence, ProjectState};
use crate::ui::style::{
    bold, dim, format_age, format_size, header, hint, plural, relative, rule, truncate, yellow,
};
use std::path::Path;

/// Max characters shown for a project in the table.
const PROJECT_W: usize = 34;

/// Displays the artifacts found, how to rebuild them, and what was left alone.
pub fn print_artifacts(root: &Path, report: &Artifacts, days: u64, all: bool) {
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
        println!(
            "\n{}",
            dim(&format!(
                "{:<PROJECT_W$} {:<6} {:>10}  {:>10}  {}{}",
                "PROJECT",
                "TYPE",
                "SIZE",
                "UNTOUCHED",
                if all { "STATE      " } else { "" },
                "REBUILD WITH"
            ))
        );
        for artifact in &report.found {
            let kind = match artifact.kind {
                ArtifactKind::Rust => "Rust",
                ArtifactKind::Node => "Node",
                ArtifactKind::Python => "Python",
            };
            let warning = if artifact.confidence == Confidence::Pinned {
                ""
            } else {
                " (!)"
            };
            let state = match (all, artifact.state) {
                (false, _) => "",
                (true, ProjectState::Forgotten) => "forgotten  ",
                (true, ProjectState::Active) => "active     ",
                (true, ProjectState::NotJudged) => "unknown    ",
            };
            println!(
                "{:<PROJECT_W$} {:<6} {}  {:>10}  {}{}{}",
                truncate(&relative(&artifact.project, root), PROJECT_W),
                kind,
                bold(&format!("{:>10}", format_size(artifact.size.bytes))),
                artifact.untouched.map_or("?".to_string(), format_age),
                state,
                dim(artifact.rebuild.unwrap_or("nothing records the packages")),
                warning
            );
        }

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

    // Never drop anything silently: say what was left out and why
    println!();
    if report
        .found
        .iter()
        .any(|a| a.confidence != Confidence::Pinned)
    {
        hint("(!) No exact record of versions: a rebuild may not give the same result.");
    }
    if report.found.iter().any(|a| a.unreadable > 0) {
        hint("Some folders couldn't be read completely, so their real size may be larger.");
    }
    if report.found.iter().any(|a| a.state == ProjectState::Active) {
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
    hint("Nothing was moved or changed.");
    println!();
}
