//! Unit tests for `src/core/screenshots.rs`, loaded there as its `tests`
//! module. Real files can't look old (their ctime can't be set back), so
//! they are judged from a `now` far in the future, and the Trash is a fake.

use super::*;
use crate::core::scanner::scan_directory;
use crate::core::test_support::{fake_trash, no_progress, temp_dir, write};
use std::fs::File;

const DAY: Duration = Duration::from_secs(86_400);

fn later() -> SystemTime {
    SystemTime::now() + DAY * 400
}

/// A file that only exists in memory, last changed `days_ago` before `now`.
fn fake(name: &str, days_ago: u64, now: SystemTime) -> FileInfo {
    let when = now - DAY * days_ago as u32;
    FileInfo {
        modified: Some(when),
        changed: Some(when),
        ..FileInfo::from_path(PathBuf::from("/nonexistent_oxide").join(name), 100)
    }
}

fn names(found: &[Screenshot]) -> Vec<String> {
    found.iter().map(|s| s.file.name().to_string()).collect()
}

#[test]
fn test_names_macos_gives_captures() {
    // macOS puts a narrow no-break space (U+202F) before AM/PM
    assert!(named_like_a_capture(
        "Screenshot 2026-09-25 at 4.11.23\u{202f}PM.png"
    ));
    assert!(named_like_a_capture(
        "Screen Shot 2021-03-01 at 10.00.00 AM.png"
    ));
    assert!(named_like_a_capture(
        "Captura de pantalla 2026-09-25 a las 16.11.23.png"
    ));
    assert!(named_like_a_capture(
        "Screen Recording 2026-01-02 at 9.00.00 AM.mov"
    ));
    assert!(named_like_a_capture(
        "Grabación de pantalla 2026-01-02 a las 9.00.00.mov"
    ));
    // Accent stored decomposed, as macOS often does
    assert!(named_like_a_capture(
        "Grabacio\u{301}n de pantalla 2026-01-02.mov"
    ));
}

#[test]
fn test_other_names_are_not_captures() {
    assert!(!named_like_a_capture("Screenshot ideas.png"));
    assert!(!named_like_a_capture(
        "my Screenshot 2026-09-25 at 4.11.23 PM.png"
    ));
    assert!(!named_like_a_capture("Screenshot 2026-9-25.png"));
    assert!(!named_like_a_capture("Screenshot.png"));
    assert!(!named_like_a_capture("IMG_1234.PNG"));
}

#[test]
fn test_only_old_captures_in_capture_formats_biggest_first() {
    let now = later();
    let mut big = fake("Screenshot 2026-01-01 at 1.00.00 PM.png", 5, now);
    big.size_bytes = 900;
    let files = [
        fake("Screenshot 2026-01-02 at 1.00.00 PM.png", 5, now),
        big,
        fake("Screenshot 2026-01-03 at 1.00.00 PM.png", 0, now), // too recent
        fake("Screenshot 2026-01-04 at 1.00.00 PM.txt", 5, now), // not an image
        fake("holiday.png", 5, now),                             // no mark, other name
    ];
    let found = find_screenshots(&files, now, DAY);
    assert_eq!(
        names(&found),
        vec![
            "Screenshot 2026-01-01 at 1.00.00 PM.png",
            "Screenshot 2026-01-02 at 1.00.00 PM.png"
        ]
    );
    assert!(found.iter().all(|s| s.detected == Detected::Name));
}

#[test]
fn test_recently_arrived_or_undated_files_are_not_listed() {
    let now = later();
    let mut arrived = fake("Screenshot 2026-01-01 at 1.00.00 PM.png", 30, now);
    arrived.changed = Some(now - DAY / 2); // copied here hours ago
    let mut undated = fake("Screenshot 2026-01-02 at 1.00.00 PM.png", 30, now);
    undated.modified = None;
    let mut future = fake("Screenshot 2026-01-03 at 1.00.00 PM.png", 30, now);
    future.modified = Some(now + DAY);
    assert!(find_screenshots(&[arrived, undated, future], now, DAY).is_empty());
}

#[test]
fn test_photo_libraries_are_never_entered() {
    let now = later();
    let mut inside = fake("x", 5, now);
    inside.path = PathBuf::from(
        "/u/Pictures/Photos Library.photoslibrary/originals/A/Screenshot 2026-01-01 at 1.00.00 PM.png",
    );
    inside.extension = Some("png".into());
    assert!(find_screenshots(&[inside], now, DAY).is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn test_mark_is_read_whatever_the_name() {
    let dir = temp_dir("shots_mark");
    write(&dir, "renamed.png", 10);
    write(&dir, "plain.png", 10);
    let status = std::process::Command::new("xattr")
        .args(["-w", "com.apple.metadata:kMDItemIsScreenCapture", "1"])
        .arg(dir.join("renamed.png"))
        .status()
        .unwrap();
    let marked = has_capture_mark(&dir.join("renamed.png"));
    let plain = has_capture_mark(&dir.join("plain.png"));
    let missing = has_capture_mark(&dir.join("missing.png"));
    let files = scan_directory(&dir, &no_progress).unwrap();
    let found = find_screenshots(&files, later(), DAY);
    let _ = fs::remove_dir_all(&dir);
    assert!(status.success());
    assert!(marked && !plain && !missing);
    assert_eq!(names(&found), vec!["renamed.png"]);
    assert_eq!(found[0].detected, Detected::Mark);
}

#[test]
fn test_trash_moves_only_unchanged_screenshots() {
    let dir = temp_dir("shots_trash");
    write(&dir, "Screenshot 2026-01-01 at 1.00.00 PM.png", 10);
    write(&dir, "Screenshot 2026-01-02 at 1.00.00 PM.png", 10);
    write(&dir, "Screenshot 2026-01-03 at 1.00.00 PM.png", 10);
    write(&dir, "Screenshot 2026-01-04 at 1.00.00 PM.png", 10);
    let files = scan_directory(&dir, &no_progress).unwrap();
    let found = find_screenshots(&files, later(), DAY);

    write(&dir, "Screenshot 2026-01-02 at 1.00.00 PM.png", 20); // edited
    fs::remove_file(dir.join("Screenshot 2026-01-03 at 1.00.00 PM.png")).unwrap();
    File::options()
        .write(true)
        .open(dir.join("Screenshot 2026-01-04 at 1.00.00 PM.png"))
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + DAY * 20_000)
        .unwrap();

    let report = remove_screenshots(&found, later(), DAY, &no_progress, &fake_trash(&dir));
    let first_gone = !dir.join("Screenshot 2026-01-01 at 1.00.00 PM.png").exists();
    let edited_kept = dir.join("Screenshot 2026-01-02 at 1.00.00 PM.png").exists();
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(found.len(), 4);
    assert_eq!(report.moved, 1);
    assert_eq!(report.moved_bytes, 10);
    assert_eq!(report.skipped.len(), 3);
    assert!(
        report
            .skipped
            .iter()
            .all(|(_, s)| *s == ScreenshotSkip::ChangedSinceScan)
    );
    assert!(first_gone && edited_kept);
}

#[test]
fn test_a_refused_move_does_not_stop_the_rest() {
    let dir = temp_dir("shots_refused");
    write(&dir, "Screenshot 2026-01-01 at 1.00.00 PM.png", 10);
    write(&dir, "Screenshot 2026-01-02 at 1.00.00 PM.png", 20);
    let files = scan_directory(&dir, &no_progress).unwrap();
    let found = find_screenshots(&files, later(), DAY);
    let trash = fake_trash(&dir);
    let report = remove_screenshots(&found, later(), DAY, &no_progress, &|path| {
        if path.ends_with("Screenshot 2026-01-02 at 1.00.00 PM.png") {
            Err("denied".into())
        } else {
            trash(path)
        }
    });
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(report.moved, 1);
    assert_eq!(
        report.skipped,
        vec![(
            dir.join("Screenshot 2026-01-02 at 1.00.00 PM.png"),
            ScreenshotSkip::CouldNotMove("denied".into())
        )]
    );
}
