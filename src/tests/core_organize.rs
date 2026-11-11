//! Unit tests for `src/core/organize.rs`, loaded there as its `tests` module.
//! No tool can set a ctime back, so real files are judged from a `now` far in
//! the future, where nothing looks recently changed.

use super::*;
use crate::core::scanner::scan_directory;
use crate::core::test_support::{no_progress, temp_dir, write};
use std::fs::File;

const DAY: Duration = Duration::from_secs(86_400);

fn later() -> SystemTime {
    SystemTime::now() + DAY * 400
}

fn at(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

fn plan(dir: &Path, by: OrganizeBy) -> OrganizePlan {
    let files = scan_directory(dir, &no_progress).unwrap();
    plan_organize(dir, &files, by, later())
}

/// A file that only exists in memory, for cases a disk can't hold.
fn fake(path: PathBuf, size: u64) -> FileInfo {
    FileInfo {
        modified: Some(at(1_700_000_000)),
        ..FileInfo::from_path(path, size)
    }
}

fn skip_of(plan: &OrganizePlan, path: &Path) -> Option<MoveSkip> {
    plan.skipped
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, s)| s.clone())
}

#[test]
fn test_year_of_known_dates() {
    assert_eq!(year_of(at(0)), Some(1970));
    assert_eq!(year_of(at(951_868_800)), Some(2000)); // 2000-03-01
    assert_eq!(year_of(at(1_704_067_199)), Some(2023)); // 2023-12-31 23:59:59
    assert_eq!(year_of(at(1_704_067_200)), Some(2024)); // 2024-01-01
    assert_eq!(year_of(at(1_709_164_800)), Some(2024)); // 2024-02-29
    assert_eq!(year_of(UNIX_EPOCH - DAY), None);
}

#[test]
fn test_unknown_and_future_dates_have_no_year() {
    let now = at(1_704_067_200);
    let mut file = fake(PathBuf::from("/r/a.txt"), 1);
    file.modified = None;
    assert_eq!(
        folder_for(&file, OrganizeBy::Year, now).as_deref(),
        Some("Unknown year")
    );
    file.modified = Some(now + DAY);
    assert_eq!(
        folder_for(&file, OrganizeBy::Year, now).as_deref(),
        Some("Unknown year")
    );
    file.modified = Some(now - DAY);
    assert_eq!(
        folder_for(&file, OrganizeBy::Year, now).as_deref(),
        Some("2023")
    );
}

#[test]
fn test_size_boundaries() {
    let folder = |size| {
        folder_for(
            &fake(PathBuf::from("/r/a"), size),
            OrganizeBy::Size,
            later(),
        )
    };
    assert_eq!(folder(MB - 1).as_deref(), Some("Under 1 MB"));
    assert_eq!(folder(MB).as_deref(), Some("1 MB to 100 MB"));
    assert_eq!(folder(100 * MB).as_deref(), Some("100 MB to 1 GB"));
    assert_eq!(folder(GB).as_deref(), Some("Over 1 GB"));
}

#[test]
fn test_type_by_extension_any_case() {
    let folder = |name: &str| folder_for(&fake(PathBuf::from(name), 1), OrganizeBy::Type, later());
    assert_eq!(folder("/r/IMG_1.JPG").as_deref(), Some("Images"));
    assert_eq!(folder("/r/backup.tar.gz").as_deref(), Some("Archives"));
    assert_eq!(folder("/r/Makefile").as_deref(), Some("Other"));
    assert_eq!(folder("/r/thing.xyz").as_deref(), Some("Other"));
}

#[test]
fn test_only_loose_visible_files_are_planned() {
    let dir = temp_dir("org_loose");
    write(&dir, "photo.png", 10);
    write(&dir, ".zshrc", 10);
    write(&dir, "project/src/main.rs", 10);
    let plan = plan(&dir, OrganizeBy::Type);
    let planned: Vec<_> = plan.moves.iter().map(|m| m.to.clone()).collect();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(planned, vec![dir.join("Images/photo.png")]);
}

#[test]
fn test_moves_and_second_run_plans_nothing() {
    let dir = temp_dir("org_twice");
    fs::write(dir.join("notes.txt"), b"keep me").unwrap();
    write(&dir, "song.mp3", 10);
    let report = apply_plan(&plan(&dir, OrganizeBy::Type), later(), &no_progress);
    let content = fs::read(dir.join("Documents/notes.txt")).unwrap();
    let again = plan(&dir, OrganizeBy::Type);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(report.moved.len(), 2);
    assert!(report.skipped.is_empty());
    assert_eq!(content, b"keep me");
    assert!(again.moves.is_empty() && again.skipped.is_empty());
}

#[test]
fn test_never_overwrites_a_file_already_there() {
    let dir = temp_dir("org_exists");
    fs::write(dir.join("a.txt"), b"new").unwrap();
    fs::create_dir(dir.join("Documents")).unwrap();
    fs::write(dir.join("Documents/a.txt"), b"old").unwrap();
    let plan = plan(&dir, OrganizeBy::Type);
    let skip = skip_of(&plan, &dir.join("a.txt"));
    let both = (
        fs::read(dir.join("a.txt")).unwrap(),
        fs::read(dir.join("Documents/a.txt")).unwrap(),
    );
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(skip, Some(MoveSkip::DestinationExists));
    assert_eq!(both, (b"new".to_vec(), b"old".to_vec()));
}

#[test]
fn test_destination_taken_after_planning_is_not_overwritten() {
    let dir = temp_dir("org_race");
    fs::write(dir.join("a.txt"), b"new").unwrap();
    let plan = plan(&dir, OrganizeBy::Type);
    fs::create_dir(dir.join("Documents")).unwrap();
    fs::write(dir.join("Documents/a.txt"), b"arrived meanwhile").unwrap();
    let report = apply_plan(&plan, later(), &no_progress);
    let kept = fs::read(dir.join("Documents/a.txt")).unwrap();
    let source_kept = dir.join("a.txt").exists();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        report.skipped,
        vec![(dir.join("a.txt"), MoveSkip::DestinationExists)]
    );
    assert_eq!(kept, b"arrived meanwhile");
    assert!(source_kept);
}

#[test]
fn test_names_differing_only_in_case_collide() {
    let root = PathBuf::from("/nonexistent_oxide_root");
    let files = [fake(root.join("A.jpg"), 1), fake(root.join("a.jpg"), 1)];
    let plan = plan_organize(&root, &files, OrganizeBy::Type, later());
    assert_eq!(plan.moves.len(), 1);
    assert_eq!(
        skip_of(&plan, &root.join("a.jpg")),
        Some(MoveSkip::SameDestination)
    );
}

#[cfg(unix)]
#[test]
fn test_non_utf8_name_is_kept_exactly() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let root = PathBuf::from("/nonexistent_oxide_root");
    let name = OsStr::from_bytes(b"caf\xe9.txt");
    let plan = plan_organize(
        &root,
        &[fake(root.join(name), 1)],
        OrganizeBy::Type,
        later(),
    );
    assert_eq!(plan.moves[0].to, root.join("Documents").join(name));
}

#[test]
fn test_downloads_in_progress_are_left_alone() {
    let dir = temp_dir("org_progress");
    write(&dir, "movie.mp4.crdownload", 10);
    write(&dir, "fresh.pdf", 10);
    let files = scan_directory(&dir, &no_progress).unwrap();
    // Judged from the real now: fresh.pdf arrived moments ago
    let plan = plan_organize(&dir, &files, OrganizeBy::Type, SystemTime::now());
    let _ = fs::remove_dir_all(&dir);
    assert!(plan.moves.is_empty());
    assert_eq!(
        skip_of(&plan, &dir.join("movie.mp4.crdownload")),
        Some(MoveSkip::InProgress)
    );
    assert_eq!(
        skip_of(&plan, &dir.join("fresh.pdf")),
        Some(MoveSkip::InProgress)
    );
}

#[test]
fn test_file_changed_after_planning_stays() {
    let dir = temp_dir("org_changed");
    write(&dir, "grown.txt", 10);
    write(&dir, "gone.txt", 10);
    write(&dir, "retouched.txt", 10);
    let plan = plan(&dir, OrganizeBy::Type);
    write(&dir, "grown.txt", 20);
    fs::remove_file(dir.join("gone.txt")).unwrap();
    File::options()
        .write(true)
        .open(dir.join("retouched.txt"))
        .unwrap()
        .set_modified(at(1_000_000_000))
        .unwrap();
    let report = apply_plan(&plan, later(), &no_progress);
    let still_there = dir.join("grown.txt").exists() && dir.join("retouched.txt").exists();
    let _ = fs::remove_dir_all(&dir);
    assert!(report.moved.is_empty());
    assert_eq!(report.skipped.len(), 3);
    assert!(
        report
            .skipped
            .iter()
            .all(|(_, s)| *s == MoveSkip::ChangedSinceScan)
    );
    assert!(still_there);
}

#[test]
fn test_one_failure_does_not_stop_the_rest() {
    let dir = temp_dir("org_fail");
    write(&dir, "a.txt", 10);
    write(&dir, "b.txt", 10);
    let plan = plan(&dir, OrganizeBy::Type);
    let report = apply_with(&plan, later(), &no_progress, &|from, to| {
        if from.ends_with("a.txt") {
            Err(io::Error::other("denied"))
        } else {
            move_no_overwrite(from, to)
        }
    });
    let a_kept = dir.join("a.txt").exists();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        report.moved,
        vec![(dir.join("b.txt"), dir.join("Documents/b.txt"))]
    );
    assert_eq!(
        report.skipped,
        vec![(dir.join("a.txt"), MoveSkip::CouldNotMove("denied".into()))]
    );
    assert!(a_kept);
}

fn word_folder(name: &str) -> Option<String> {
    folder_for(
        &fake(PathBuf::from("/r").join(name), 1),
        OrganizeBy::Words,
        later(),
    )
}

#[test]
fn test_word_table_is_lowercase_without_accents() {
    for (_, words) in WORD_FOLDERS {
        for word in *words {
            assert_eq!(
                words_of(word),
                vec![word.to_string()],
                "write '{}' folded",
                word
            );
        }
    }
}

#[test]
fn test_words_match_in_both_languages() {
    assert_eq!(word_folder("Tarea 3.docx").as_deref(), Some("Homework"));
    assert_eq!(
        word_folder("homework_week2.txt").as_deref(),
        Some("Homework")
    );
    assert_eq!(word_folder("Mi-CV.docx").as_deref(), Some("Resumes"));
    assert_eq!(word_folder("Résumé 2024.pages").as_deref(), Some("Resumes"));
    assert_eq!(
        word_folder("website-final.zip").as_deref(),
        Some("Websites")
    );
}

#[test]
fn test_words_match_whole_not_inside_other_words() {
    assert_eq!(word_folder("webcam.jpg"), None);
    assert_eq!(word_folder("projection.txt"), None);
    // Spanish "resumen" is a summary, not a resume
    assert_eq!(word_folder("resumen clase.txt"), None);
}

#[test]
fn test_accents_in_either_unicode_form() {
    assert_eq!(word_folder("Solución.txt").as_deref(), Some("Solutions"));
    // "o" followed by a combining acute accent, as macOS often stores names
    assert_eq!(
        word_folder("Solucio\u{301}n.txt").as_deref(),
        Some("Solutions")
    );
}

#[test]
fn test_first_rule_wins_and_formats_come_last() {
    assert_eq!(
        word_folder("tarea_proyecto_web.pdf").as_deref(),
        Some("Projects")
    );
    assert_eq!(word_folder("tarea.pdf").as_deref(), Some("Homework"));
    assert_eq!(word_folder("informe.PDF").as_deref(), Some("PDF"));
    assert_eq!(word_folder("ventas.csv").as_deref(), Some("CSV"));
    // By the real extension, not the word
    assert_eq!(word_folder("guia pdf.docx"), None);
    assert_eq!(word_folder("export_csv.xlsx"), None);
}

#[test]
fn test_files_without_words_stay_where_they_are() {
    let dir = temp_dir("org_words");
    write(&dir, "notas.txt", 10);
    write(&dir, "proyecto final.key", 10);
    let plan = plan(&dir, OrganizeBy::Words);
    let _ = fs::remove_dir_all(&dir);
    let planned: Vec<_> = plan.moves.iter().map(|m| m.to.clone()).collect();
    assert_eq!(planned, vec![dir.join("Projects/proyecto final.key")]);
    assert!(plan.skipped.is_empty());
}

#[test]
fn test_log_appends_every_run_relative_to_the_folder() {
    let dir = temp_dir("org_log");
    append_log(&dir, &[(dir.join("a.pdf"), dir.join("PDF/a.pdf"))]).unwrap();
    append_log(&dir, &[(dir.join("b.csv"), dir.join("CSV/b.csv"))]).unwrap();
    let text = fs::read_to_string(dir.join(LOG_NAME)).unwrap();
    let count = logged_moves(&dir).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(text, "a.pdf\tPDF/a.pdf\nb.csv\tCSV/b.csv\n");
    assert_eq!(count, 2);
}

/// Organizes `dir` by type for real and logs it, as `--apply` does.
fn organize(dir: &Path) -> MoveReport {
    let report = apply_plan(&plan(dir, OrganizeBy::Type), later(), &no_progress);
    append_log(dir, &report.moved).unwrap();
    report
}

#[test]
fn test_undo_puts_everything_back_and_tidies_up() {
    let dir = temp_dir("org_undo");
    fs::write(dir.join("notes.txt"), b"keep me").unwrap();
    write(&dir, "song.mp3", 10);
    fs::create_dir(dir.join("Audio")).unwrap();
    write(&dir, "Audio/mine.mp3", 10); // was there before: Audio/ must stay
    organize(&dir);
    let report = undo_organize(&dir, &no_progress).unwrap();
    let content = fs::read(dir.join("notes.txt")).unwrap();
    let state = (
        dir.join("song.mp3").exists(),
        dir.join("Documents").exists(),
        dir.join("Audio/mine.mp3").exists(),
        dir.join(LOG_NAME).exists(),
    );
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(report.moved.len(), 2);
    assert!(report.skipped.is_empty());
    assert_eq!(content, b"keep me");
    assert_eq!(state, (true, false, true, false));
}

#[test]
fn test_undo_never_overwrites_and_keeps_what_it_could_not_undo() {
    let dir = temp_dir("org_undo_keep");
    fs::write(dir.join("a.txt"), b"organized").unwrap();
    write(&dir, "b.txt", 10);
    write(&dir, "c.txt", 10);
    organize(&dir);
    fs::write(dir.join("a.txt"), b"new file, same name").unwrap();
    fs::remove_file(dir.join("Documents/c.txt")).unwrap();
    let report = undo_organize(&dir, &no_progress).unwrap();
    let a = (
        fs::read(dir.join("a.txt")).unwrap(),
        fs::read(dir.join("Documents/a.txt")).unwrap(),
    );
    let log = fs::read_to_string(dir.join(LOG_NAME)).unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        report.moved,
        vec![(dir.join("Documents/b.txt"), dir.join("b.txt"))]
    );
    assert_eq!(
        skip_of_report(&report, &dir.join("Documents/a.txt")),
        Some(MoveSkip::DestinationExists)
    );
    assert_eq!(
        skip_of_report(&report, &dir.join("Documents/c.txt")),
        Some(MoveSkip::Gone)
    );
    assert_eq!(a, (b"new file, same name".to_vec(), b"organized".to_vec()));
    assert_eq!(log, "a.txt\tDocuments/a.txt\nc.txt\tDocuments/c.txt\n");
}

fn skip_of_report(report: &MoveReport, path: &Path) -> Option<MoveSkip> {
    report
        .skipped
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, s)| s.clone())
}

#[test]
fn test_undo_ignores_edited_lines_that_leave_the_folder() {
    let dir = temp_dir("org_undo_evil");
    let outside = temp_dir("org_undo_outside");
    write(&outside, "secret.txt", 10);
    write(&dir, "Docs/x.txt", 10);
    let lines = [
        "../org_undo_outside_x/secret.txt\tDocs/secret.txt",
        "x.txt\t../x.txt",
        "y.txt\t/etc/hosts",
        "renamed.txt\tDocs/x.txt",
        "no tab here",
    ];
    fs::write(dir.join(LOG_NAME), lines.join("\n")).unwrap();
    let report = undo_organize(&dir, &no_progress).unwrap();
    let untouched = dir.join("Docs/x.txt").exists() && outside.join("secret.txt").exists();
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&outside);
    assert!(report.moved.is_empty());
    assert_eq!(report.skipped.len(), lines.len());
    assert!(
        report
            .skipped
            .iter()
            .all(|(_, s)| *s == MoveSkip::NotFromThisFolder)
    );
    assert!(untouched);
}

#[test]
fn test_no_log_means_nothing_to_undo() {
    let dir = temp_dir("org_undo_none");
    let count = logged_moves(&dir).unwrap();
    let undo = undo_organize(&dir, &no_progress);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(count, 0);
    assert_eq!(undo.unwrap_err().kind(), io::ErrorKind::NotFound);
}
