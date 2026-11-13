//! Terminal user interface: everything that is printed or typed.
//!
//! - `style`   colors, titles, prompts and text helpers
//! - `report`  result screens, and the report shared by moves to the Trash
//! - `input`   reading lines, paths, yes/no answers and selections
//! - `duplicates` the identical files screens: list, choosing, results
//! - `artifacts` the build folders screens: list, choosing, results
//! - `screenshots` the screenshots screens: list, choosing, results
//! - `organize` the plan and report for organizing files into folders
//! - `menu`    start screen and action menu
//! - `browser` folder browser
//! - `picker`  paginated document picker
//! - `progress` spinning loading indicator with a live counter

pub mod artifacts;
pub mod browser;
pub mod duplicates;
pub mod input;
pub mod menu;
pub mod organize;
pub mod picker;
pub mod progress;
pub mod report;
pub mod screenshots;
pub mod style;
