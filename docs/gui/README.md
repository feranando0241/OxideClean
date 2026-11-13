# Desktop GUI — groundwork

Everything gathered before writing GUI code. The decision itself will be
recorded in `docs/adr/0006-gui.md`; this folder holds the working material
(research, design tokens, wireframes) that the ADR points to.

## Decisions so far (2026-11-07)

| Topic | Decision |
|---|---|
| First user | The owner, on their own Mac. No signing, notarization or `.app` bundle yet |
| Framework | Dioxus desktop, pinned to **0.7.x** stable (0.8 is alpha with breaking changes) |
| CLI | Stays as it is. The GUI is a third front-end over `core`, next to `ui/` and `cli` |
| Style | Mac conventions for controls and navigation, own identity in an "oxide" accent color and title type |
| UI language | Spanish. The CLI stays in English |
| Start screen | Pick or drop a folder → scan with progress → summary. Sidebar to Duplicados, Olvidados, Build folders |
| Phase 1 | Duplicados screen only. The rest once it is useful day to day |

Rejected: egui (look), Tauri (npm + a JS bridge for no gain on one Mac),
Electron / SwiftUI / Flutter (cost of the Rust bridge), mobile (sandboxing
blocks scanning the disk; no Trash).

## Project rules the GUI must keep

| Rule (source) | What it means for the GUI |
|---|---|
| `core` never prints or imports front-ends (ADR-0002, `tests/architecture.rs`) | Add `gui` to the forbidden imports. The GUI only consumes `core` |
| No source file over 500 lines | One file per screen in `src/gui/` |
| Unit tests in `src/tests/`, loaded with `#[path]` | Same for any testable GUI logic |
| Listing duplicates is read-only; moving asks first (ADR-0001) | Moving is an explicit step with one confirmation that says what moves and how much space comes back |
| Trash never trusts the cache (ADR-0003) | Always go through `trash_duplicates` / `trash_artifacts`, which re-verify on disk |
| Closed list, Python never "reliable" (ADR-0005) | Show the same states as the CLI; no new categories |
| `--old` is read-only (ADR-0004) | No delete button on Olvidados |
| Defaults live in `core` | Import `DEFAULT_*`; never repeat a number |
| CI: fmt, clippy `-D warnings`, `cargo audit` | Build the `gui` feature in CI too, or nothing checks it. Expect `cargo audit` noise from Dioxus's dependency tree |
| Trash rules in one place (ADR-0002) | **Prerequisite:** move the rules in `app::handle_duplicates` out of the terminal prompts into I/O-free functions shared by CLI and GUI |

## Design guidance (Apple HIG, applied)

- **Navigation:** sidebar with the sections, at most two levels, can be hidden, nothing important at its bottom edge.
- **Progress:** determinate bar when the total is known, indeterminate when `total == 0`; Cancel when stopping is harmless.
- **Confirming a move to the Trash:** button says "Mover a la Papelera", never "OK"; a "Cancelar" button that is not the default; one confirmation per operation, not per file.
- **Dark mode:** follow the system, no in-app switch. Colors as CSS custom properties by role (surface, text, accent, danger), each with light and dark values.
- **Mac desktop:** menu bar with every command, standard shortcuts (⌘O open folder, ⌘R rescan), hover feedback on everything clickable.
- **Accessibility:** body 13 pt (min 10 pt), contrast 4.5:1 for text, controls at least 28×28 pt, nothing conveyed by color alone (a kept copy has an icon and a label, not just a green tint).

## Open before coding

1. Check how a long blocking job (scan, hashing) runs off the UI thread in Dioxus 0.7 and feeds `Progress(done, total)` back to the window. Answer it with a tiny spike, not by guessing.
2. Design tokens: colors (light/dark, with measured contrast), type scale, the signature element.
3. ASCII wireframes of the summary and Duplicados screens.
4. Write ADR-0006.

## Sources

- Dioxus releases: https://github.com/DioxusLabs/dioxus/releases
- dioxus on docs.rs: https://docs.rs/crate/dioxus/latest
- dioxus-desktop `Config`: https://docs.rs/dioxus-desktop/latest/dioxus_desktop/struct.Config.html
