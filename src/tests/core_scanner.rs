//! Unit tests for `src/core/scanner.rs`, loaded there as its `tests` module.

use super::*;
use crate::core::test_support::{no_progress, temp_dir};
use std::fs::{File, FileTimes};
use std::time::{Duration, SystemTime};

#[test]
fn test_scan_records_modified_and_accessed_times() {
    let dir = temp_dir("scan_times");
    let file = dir.join("old.bin");
    fs::write(&file, "data").unwrap();
    let day = Duration::from_secs(86_400);
    let modified = SystemTime::now() - 400 * day;
    let accessed = SystemTime::now() - 200 * day;
    File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(
            FileTimes::new()
                .set_modified(modified)
                .set_accessed(accessed),
        )
        .unwrap();

    let files = scan_directory(&dir, &no_progress).unwrap();
    fs::remove_dir_all(&dir).unwrap();

    // Within a second: some file systems store coarser timestamps
    let close = |a: Option<SystemTime>, b: SystemTime| {
        let a = a.expect("timestamp recorded");
        a.duration_since(b).unwrap_or_else(|e| e.duration()) < Duration::from_secs(1)
    };
    assert_eq!(files.len(), 1);
    assert!(close(files[0].modified, modified), "modified time");
    assert!(close(files[0].accessed, accessed), "accessed time");
}

#[test]
fn test_should_ignore_directory() {
    assert!(should_ignore_directory("node_modules"));
    assert!(should_ignore_directory("target"));
    assert!(should_ignore_directory(".git"));
    assert!(should_ignore_directory(".cache"));
    assert!(!should_ignore_directory("photos"));
    assert!(!should_ignore_directory("documents"));
}

#[test]
fn test_scan_loose_files_stays_in_the_folder() {
    let dir = crate::core::test_support::temp_dir("scan_loose");
    crate::core::test_support::write(&dir, "a.txt", 1);
    crate::core::test_support::write(&dir, ".DS_Store", 1);
    crate::core::test_support::write(&dir, "sub/b.txt", 1);
    let names: Vec<_> = scan_loose_files(&dir)
        .unwrap()
        .into_iter()
        .map(|f| f.path)
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(names, vec![dir.join("a.txt")]);
}

#[test]
fn test_files_are_ignored_by_extension_not_by_name() {
    assert!(should_ignore_file("main.o"));
    assert!(should_ignore_file("App.class"));
    assert!(should_ignore_file(".DS_Store"));
    // No extension: the whole name must not be taken for one
    for name in ["o", "class", "pyc", "pyo"] {
        assert!(!should_ignore_file(name), "'{}' was ignored", name);
    }
}
