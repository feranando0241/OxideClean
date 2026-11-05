# OxideClean

A fast, modular file scanner and organizer built in Rust. OxideClean helps you understand what is taking up space on your disk, find duplicate files, forgotten files and leftover build folders, detect similar documents, and keep your folders clean — all from the terminal.

---

## Features

| # | Feature | Description |
|---|---------|-------------|
| 1 | **File tree view** | Recursive scan grouped by folder, with sizes and extensions |
| 2 | **Top 10 largest files** | Quickly spot what is eating your disk space |
| 3 | **Filter by extension** | Find all `.pdf`, `.mp4`, `.zip`, or any other type |
| 4 | **Folder listing** | List the folders inside the scanned one (a folder browser is also available before scanning) |
| 5 | **Identical copies** | SHA-256 hash comparison — finds byte-for-byte identical files and moves the extra copies to Trash |
| 6 | **Same-name file finder** | Groups files that share a filename across different folders |
| 7 | **Document similarity** | Composite content analysis (vocabulary + bigrams + length) to find similar PDFs, text files, and source code |
| 8 | **Forgotten files** | Big files neither modified nor read in months, and where that space is (read-only) |
| 9 | **Build folders** | `target`, `node_modules` and Python environments of projects you stopped working on, with how to rebuild them (read-only) |

---

## Installation

### Requirements

- **Rust** (1.88 or later) — installed via [Homebrew](https://brew.sh) or [rustup](https://rustup.rs)
- **macOS** (tested on Apple Silicon and Intel). CI also runs the tests on Linux.

### Build and install

```bash
# Clone or open the project folder
cd ~/path/to/Rust\ project

# Build the optimized binary
cargo build --release

# Install to your local bin (make sure ~/.local/bin is in your PATH).
# Remove the old copy first: on macOS, overwriting a binary in place makes
# the system kill it on launch (exit code 137).
rm -f ~/.local/bin/oxide_clean
cp target/release/oxide_clean ~/.local/bin/oxide_clean
```

Add to your shell if not already there:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

### Run

```bash
oxide_clean
```

---

## Command-line usage

Run without arguments for the interactive menus, or pass a path and one action to run it directly and exit:

```bash
oxide_clean ~/Downloads                      # scan, show summary, open the action menu
oxide_clean ~/Downloads --largest 20         # top 20 largest files (default 10)
oxide_clean ~/Downloads --ext pdf            # all PDF files
oxide_clean ~/Downloads --dupes              # list exact duplicates (read-only)
oxide_clean ~/Downloads --dupes --trash      # move duplicate copies to Trash (asks first)
oxide_clean ~/Downloads --dupes --trash -y   # same, without asking
oxide_clean ~/Pictures --dupes --keep-in Albums --trash   # keep the copies inside Albums/
oxide_clean ~ --dupes --exclude Library --exclude Backups  # leave folders out of the scan
oxide_clean ~/Downloads --same-name          # files that share a name
oxide_clean ~/Downloads --old                # untouched for 180+ days, biggest first
oxide_clean ~/Downloads --old 365            # untouched for more than a year
oxide_clean ~ --artifacts                    # build folders of projects untouched for 90+ days
oxide_clean ~ --artifacts 30 --exclude Work  # 30 days, leaving a folder out
oxide_clean ~ --artifacts --all              # every project, marking the active ones
oxide_clean ~ --artifacts --trash            # choose which to move to the Trash
oxide_clean ~ --artifacts --all --trash      # same, with active projects to choose from
oxide_clean ~ --artifacts --trash -y         # move the clear-cut ones without asking
oxide_clean ~/Documents --similar thesis.pdf --threshold 50
oxide_clean --help
```

- `--dupes` on its own never deletes anything.
- `--old [DAYS]` only lists; it never moves or changes anything. See [How forgotten files are found](#how-forgotten-files-are-found).
- `--artifacts [DAYS]` only lists too; `--all` adds the projects in use, and `--trash` moves the ones you choose. See [How build folders are found](#how-build-folders-are-found).
- `--keep-in <FOLDER>` keeps, in each group, every copy inside that folder instead of the oldest, so nothing inside it is ever moved (groups without a copy there keep the oldest). If the folder doesn't exist or holds no copy at all, the run stops with an error and nothing is moved, so a typo can't make `--trash --yes` remove the copies you meant to keep.
- `--exclude <FOLDER>` leaves a folder out of the scan; repeat it for several. Folders are relative to the scanned path, or full paths; `..` and symlinks to a folder inside the scan work too.
- `--no-cache` doesn't reuse or save file hashes between runs.
- `--trash` always asks for confirmation unless `--yes`/`-y` is given. Without a terminal to ask on (e.g. in a script), it refuses and exits with an error.
- Output can be piped or redirected (`> report.txt`); colors turn off automatically.
- Exit codes: `0` success, `1` runtime error, `2` invalid arguments.

See [ADR-0001](docs/adr/0001-cli-arguments.md) for the design rationale.

---

## Usage (interactive)

When launched, OxideClean presents a startup prompt:

```
OxideClean
See what's using space, find identical copies and similar documents.

Which folder do you want to look at?
  • Type its path       e.g. ~/Downloads, or . for the current folder
  • Type b              to browse your folders
  • Type q              to quit
›
```

After scanning, you get a short summary of where the space goes, then the action menu:

```
What would you like to do?

  Look around
    1  See every file, folder by folder
    2  See the 10 biggest files
    3  See only one type of file (pdf, jpg, mp4…)
    4  List the folders inside this one

  Clean up
    5  Find identical copies of files (and free up space)
    6  Find files with the same name in different places
    8  Find big files nobody has touched in months
    9  Find build folders of projects you stopped working on

  Compare documents
    7  Find documents with similar content

    0  Choose another folder        ?  Show this menu again
```

After the first action, the menu shrinks to a two-line reminder; type `?` to see the full one again.

---

## How the similarity engine works

Option **[7]** compares documents using a **Composite Score**:

| Weight | Metric | What it measures |
|--------|--------|-----------------|
| 50% | Vocabulary (Jaccard) | Shared unique keywords between documents |
| 40% | Bigrams (Jaccard) | Shared 2-word phrases — indicates shared structure |
| 10% | Length ratio | Penalizes comparing a 10-word file to a 10,000-word file |

**Supported file types:** PDF, TXT, MD, JSON, CSV, RS, PY, JS, C, CPP

**Threshold guide:**

| Range | Meaning |
|-------|---------|
| >= 70% | Very similar — same topic, structure, and phrasing |
| 40-69% | Related — shared vocabulary, different focus |
| 15-39% | Somewhat related — share some terms in the field |
| < 15% | Distant — very little content overlap |

---

## How duplicate detection works

Option **[5]** (or `--dupes`) narrows the candidates in three steps, so most files are never read in full:

1. **Group by file size.** Files with different sizes can't be identical. This needs no reading at all.
2. **Compare the first 4 KB.** Same-size files that start differently are ruled out after a tiny read.
3. **Full SHA-256.** Only the files left are hashed in full, on up to 4 threads. Identical hashes mean byte-for-byte duplicates.

Hardlinks (several paths to the same data) count as one file. They are never reported as copies of each other, and when an extra copy has several paths, all of them are moved, because the space only comes back once every path is gone.

**Hash cache.** Hashes are saved between runs (`~/Library/Caches/oxide_clean/hashes.tsv` on macOS), so scanning the same folder again skips files that haven't changed. A hash is reused only if the file's size, modification time, change time (ctime) and inode all match, which catches edits even when tools preserve the modification time. A corrupt or outdated cache is ignored, entries unused for 90 days are dropped, and `--no-cache` turns it off. See [ADR-0003](docs/adr/0003-hash-cache.md).

**Moving to the Trash never trusts the cache.** Right before acting, the kept copy and every extra copy are re-read from disk. If anything changed, that file (or the whole group, if the kept copy changed) is skipped. Duplicates are listed with the space that would be recovered. In the menu you then choose what to do:

- `y` moves every extra copy to the Trash.
- Group numbers (`1,3,5-7`) move only those groups.
- `k <folder>` keeps the copies inside a folder (e.g. `k Photos`) and shows the groups again.
- Enter leaves everything as it is.

Everything goes to the macOS Trash, where it can be restored.

---

## How forgotten files are found

A file is listed by `--old` (menu option 8) when **all three** hold, with the default of 180 days:

1. It wasn't **modified** in the last 180 days.
2. It wasn't **read** in the last 180 days, as far as the file system records reads.
3. It didn't **arrive** on this disk, or have its details changed, in the last **30 days**.

Exactly 180 days doesn't count; it must be older. The list shows the 20 biggest with how long each has been untouched, the total for all of them, and which folders hold that space. Hardlinks count once.

**Why it errs towards listing less.** Unlike a duplicate, an old file may be the only copy, so a wrong "forgotten" is worse than a missed one. Every rule can only take files *off* the list:

- Read times alone prove nothing: on APFS, reading a file often doesn't update them. So a recent read keeps a file off the list, but an old read time isn't trusted to mean "unread", and the screen says "untouched", never "unread".
- Unpacking an archive or copying with `cp -p` keeps old modification dates, and on macOS it even moves the creation date back. The 30-day rule therefore uses the change time (ctime), which no tool can set back.
- Files without a modification date, or with a date in the future, aren't judged; the screen says how many there were.

Details and measurements: [ADR-0004](docs/adr/0004-forgotten-files.md).

---

## How build folders are found

`--artifacts` (menu option 9) lists folders that compilers and package managers generate, in projects you haven't touched for 90 days. On its own it only lists and measures. No project tool such as `cargo` or `npm` is ever run, because that would execute the project's own code.

| Type | Folder | Must also have | Rated reliable when |
|---|---|---|---|
| Rust | `target/` | `Cargo.toml` beside it, `CACHEDIR.TAG` inside it | `Cargo.lock` exists |
| Node | `node_modules/` | `package.json` beside it | A lockfile exists |
| Python | `.venv/`, `venv/` | `pyvenv.cfg` inside it | Never: few projects record exact versions |

- **A closed list:** any other folder is ignored, whatever its name. Hidden folders aren't searched, which keeps the `node_modules` of your editor's extensions out.
- **One recent file protects the whole project**, using the same rules as forgotten files, and so does recent Git activity. Projects in use, or that arrived in the last 30 days, are only counted.
- **`--all` shows every project** with its state, because active projects pile up build files too. In the menu, answer `all`. The closed list still applies: `--all` drops the activity rule, not the others.
- **`(!)` marks a folder whose rebuild may not give the same result**, and any old project can fail to rebuild if a package or tool has moved on.

Sizes are logical file sizes, usually a little below what `du` shows.

**Moving to the Trash.** With `--trash`, or from the menu, you choose what to move. Space comes back when you empty the Trash.

| Case | Can it move? |
|---|---|
| Forgotten project, Rust or Node with a lockfile, no programs in `target/release` | Yes: the only case `y` and `--yes` reach |
| Active project, one that can't be judged, or `(bin)`: programs in `target/release` | Only by number, and after typing `yes` |
| Python environment or no lockfile (`-` in the list) | No |

Right before each move, the tool checks from disk that the folder still qualifies, that a forgotten project wasn't used since the list, and that nothing inside changed in the last 10 minutes, which could mean a build is running. Anything that fails stays where it is, with the reason shown. `--all --trash --yes` is rejected, so a script can never reach an active project.

Details: [ADR-0005](docs/adr/0005-build-artifacts.md).

**Tested promise:** a property test builds random projects mixed with look-alikes (a `target` holding a thesis, a `venv` without `pyvenv.cfg`) and runs the real move on them, with a fake Trash: exactly the movable build folders must go, and every other file must stay identical byte for byte. End-to-end tests take a real Rust project and a real Node project through build, removal and rebuild, and run the result.

---

## Ignored directories and files

OxideClean automatically skips folders and files that generate noise:

**Ignored directories:** `.git`, `target`, `node_modules`, `.cache`, `.vscode`, `.idea`, `.Trash`, `venv`, `.venv`, `env`, `.env`, `__pycache__`, `Library`, `dist`, `build`, `.next`, `.nuxt`, `out`, `site-packages`, `.gem`, `.npm`, `.yarn`, `.pnpm-store`, `.mypy_cache`, `.pytest_cache`, `.ruff_cache`, `.tox` — plus every other hidden folder, and any folder ending in `_venv`, `-venv`, `-env`, `.app`, `.framework`, or `.bundle`.

**Ignored files:** `.DS_Store`, `.localized`, `Thumbs.db`, `desktop.ini`, `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Pipfile.lock`, `poetry.lock` — plus files with extensions `.pyc`, `.pyo`, `.class`, `.o`.

---

## Project structure

```
src/
├── main.rs            — Entry point: parse arguments, pick a mode
├── cli.rs             — Command-line argument parsing
├── app.rs             — Actions shared by the CLI and the menus (incl. Trash rules)
├── core/              — Logic only: never prints, never reads input
│   ├── models.rs      — FileInfo (size, dates) and FileId (identity of a file's data on disk)
│   ├── paths.rs       — Matching folders typed by the user against scanned paths
│   ├── scanner.rs     — Recursive directory scanning and ignore rules
│   ├── files.rs       — Biggest files, by type, same name, where the space goes
│   ├── old_files.rs   — Forgotten files: untouched for months (ADR-0004)
│   ├── artifacts.rs   — Finding build folders in projects (ADR-0005)
│   ├── artifact_trash.rs — Moving them to the Trash, re-verified from disk
│   ├── duplicates/    — Everything about identical files
│   │   ├── mod.rs        — Finding them (size → prefix → full hash)
│   │   ├── hashing.rs    — Full and 4 KB prefix SHA-256, parallel hashing (private to duplicates/)
│   │   ├── hash_cache.rs — Hashes saved between runs
│   │   └── trash.rs      — Moving extra copies to the Trash, re-verified from disk
│   └── similarity.rs  — Document similarity engine (Jaccard + bigrams)
├── ui/                — Everything printed or typed
│   ├── style.rs       — Colors, titles, prompts, text helpers
│   ├── report.rs      — Result screens
│   ├── duplicates.rs  — Choosing which duplicate groups to move and what to keep
│   ├── artifacts.rs   — The build folders screens: list, choosing, results
│   ├── input.rs       — Reading lines, paths and group selections
│   ├── menu.rs        — Start screen and action menu
│   ├── browser.rs     — Folder browser
│   ├── picker.rs      — Paginated document picker
│   └── progress.rs    — Loading spinner with a live counter
└── tests/             — Unit tests, one file per module (<layer>_<module>.rs)
    ├── core_duplicates_props.rs — Property tests (random folders vs. brute force)
    └── core_test_support.rs     — Helpers shared by core's tests
tests/
├── cli.rs             — End-to-end tests that run the real binary
└── architecture.rs    — Structure rules (see below)
```

**Structure rules, enforced by `cargo test` and CI** (`tests/architecture.rs`):

1. `core/` never imports `ui/`, `app` or `cli`.
2. `core/` never prints or reads input; it returns data or a typed error.
3. No source file grows past 500 lines; split it by responsibility instead.
4. Unit tests live in `src/tests/`, not inside the production files, and every file there must be loaded by a `#[path]` (otherwise its tests would silently never run).

On top of that, CI rejects unformatted code (`cargo fmt --check`) and any clippy warning. See [ADR-0002](docs/adr/0002-modular-architecture.md) for the reasoning and a short "how to add a feature" guide.

---

## Running the tests

```bash
cargo test
```

- **Unit tests** live in `src/tests/`, one file per module (e.g. `src/tests/core_duplicates_trash.rs` for `src/core/duplicates/trash.rs`). Each module loads its file with `#[cfg(test)] #[path = "…"] mod tests;`, so the tests can still reach private items. They cover ignore rules, duplicate detection (oldest kept, hardlinks, same prefix with different content, kept copy changed before trashing), matching folders typed by the user, forgotten files (every rule and edge case), build folders (what counts, what doesn't, symlinks, hardlinks), file queries, similarity scoring, size and age formatting, exit commands and argument parsing.
- **Property tests** in `src/tests/core_duplicates_props.rs` build 150 random folders per run (exact copies, same-prefix-different-tail files, hardlinks, empty files, sizes around the 4 KB boundary). They check that detection matches a brute-force byte comparison with no cache, a cold cache and a warm cache. After moving the copies (to a fake Trash), they check that no content was lost, no duplicates remain and the freed space is what was promised. A failure prints the seed that reproduces it.
- **More property tests:** forgotten files are checked against the rules written independently, and every file must land in exactly one group. Build folders are checked on random projects mixed with look-alikes: removing what is listed must remove exactly the build folders and leave every other file identical.
- **End-to-end tests** in `tests/cli.rs` run the compiled binary against a temporary folder and check what the user sees: output and exit codes for every CLI action, the refusal to trash without a terminal, and the interactive menus. Two of them use real tools, offline: a Rust project and a Node project are built, lose their build folder, are rebuilt, and must still run. `cargo` is always there; the Node one needs `npm`, and says it was skipped if it isn't installed.

**Continuous integration:** GitHub Actions (`.github/workflows/ci.yml`) runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` on macOS and Linux for every push to `master` and every pull request. Any new warning fails the build. A separate job runs `cargo audit` against the [RustSec](https://rustsec.org) advisory database, on every push and also every Monday, since new vulnerabilities get published even when the code doesn't change.

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `sha2` | 0.11 | SHA-256 hashing for duplicate detection |
| `trash` | 5.2.9 | Cross-platform safe move-to-Trash |
| `pdf-extract` | 0.12 | Text extraction from PDF files |

---

## License

This project is open source and available under the MIT License.
