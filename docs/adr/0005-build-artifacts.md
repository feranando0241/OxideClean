# ADR-0005: Finding build artifacts in forgotten projects

**Status:** Accepted
**Date:** 2026-11-05
**Deciders:** M4DEN5K (project owner)

## Context

Compilers and package managers leave folders that dwarf the code they came from. This project's `target/` was 1.5 GB next to 260 KB of source. A search of the owner's home folder found about 2.9 GB in the first few projects.

The obvious risk is flagging the wrong folder. That same search found 11 `node_modules` inside `.vscode` and `.antigravity-ide`: those are installed editor extensions, and removing them breaks the editor.

The less obvious risk is that "regenerable" is an assumption, and it's weakest for old projects: packages disappear, toolchains move on, unpinned versions change, and `target/release` may hold the only copy of a binary.

## Decision

`--artifacts [DAYS]` (default **90 days**) lists and measures. On its own it changes nothing. Moving to the Trash needs `--trash` (see below).

`--artifacts --all` lists every project, with its state (forgotten, active or unknown), because active projects pile up build files too, and they are the likeliest to rebuild cleanly. Their risk is interrupting work rather than losing data. It drops only the activity rule; the closed list still applies. It isn't the default because the default list should be what can be cleaned with little thought.

**A closed list.** A folder is an artifact only if it matches a known combination. Anything else, including tools that don't exist yet, is ignored, so the tool fails on the safe side.

| Type | Folder | Must also have | Rated reliable when |
|---|---|---|---|
| Rust | `target/` | `Cargo.toml` beside it and `CACHEDIR.TAG` inside it | `Cargo.lock` exists |
| Node | `node_modules/` | `package.json` beside it | A lockfile exists |
| Python | `.venv/`, `venv/` | `pyvenv.cfg` inside it | Never |

- **Python is never rated reliable.** Few projects record exact versions, and without `requirements.txt` or `pyproject.toml` nothing says which packages to reinstall.
- **A `target/` without `CACHEDIR.TAG`** could be anyone's folder. It isn't listed, and the screen says how many there were.
- **Symlinks are never artifacts**, and they aren't followed when measuring.

**Only forgotten projects.** It reuses `judge` from ADR-0004: every scanned file of the project must be forgotten at the threshold. One file in use, or one that just arrived, keeps the project off the list. A file with no date or a future date does too, because the project can't be judged. The scanner skips hidden folders, which keeps editor extensions out.

**Nothing from the project is run.** No `cargo clean`, no `npm`: that would execute the project's own code. The rebuild command is only shown.

**Hardlinks count once** when measuring. Cargo hardlinks heavily inside `target/`.

## Moving to the Trash

`--artifacts --trash` asks which folders to move; the menu offers the same after the list. Everything goes to the Trash, never straight to deletion, and only the folder moves.

| Case | Can it move? |
|---|---|
| Forgotten project, Rust or Node with a lockfile, no programs in `target/release` | Yes. It's the only case `y` and `--yes` reach |
| Active project, a project that can't be judged, or programs in `target/release` | Only by number, and after typing `yes` |
| Python environment, or no lockfile | No, not even by number |

`--all --trash --yes` is rejected: an unattended run must not be able to reach an active project.

**Right before each move, from disk:**
- The folder still matches the closed list, with the same lockfile, and is still a real folder, not a symlink.
- If it was listed as forgotten, a fresh scan of the whole project says it still is. This scan ignores `--exclude`, so work in an excluded subfolder counts.
- Nothing inside changed in the last 10 minutes, and no date is in the future: a build may be running. This applies even to folders chosen knowingly.

**Git counts as activity.** A recent change to `.git/index`, `HEAD`, `FETCH_HEAD` or `ORIG_HEAD`, in the project or a folder above it, makes the project active. Commits and checkouts can happen without a scanned file changing.

## Options Considered

| Option | Verdict |
|---|---|
| Match folder names alone | Rejected: a folder called `target` may hold anything |
| A marker beside the folder, without the activity rule, as the default | Rejected: it would flag projects in daily use. Available on request with `--all` |
| Moving to the Trash in the first version | Postponed until the list had been tried on a real disk, then added with `--trash` |
| Moving active projects with `y` or `--yes` | Rejected: a quick answer or a script shouldn't interrupt work in progress |
| Moving Python environments | Rejected: nothing reliably records their exact versions |
| Running `cargo clean` or `npm` | Rejected: runs code from the project |
| Global caches (`~/.cargo/registry`, `~/.npm`, Xcode `DerivedData`) | Out of scope: they live outside projects and follow other rules |

## Consequences

- Sizes are logical file sizes, like everywhere else in the tool. Measured against `du` on the owner's disk, they came out 8–17% lower, probably because many small files round up to whole disk blocks. The tool promises less space than it can free, which is the safe side.
- With pnpm, or anything linking to a shared store, less space may come back than is shown. The owner has one such project.
- Activity is judged on scanned files and Git. Ignored files (such as lockfiles, or anything in `dist/`) don't count, so a project can still look more forgotten than it is.
- The 10-minute check is a heuristic. A dev server that isn't writing files won't trip it, which is why active projects also take an explicit `yes`.
- Any read recorded in atime keeps a project active, and that includes reads by other tools, such as `--dupes` hashing every file. This errs on the safe side, but can hide a forgotten project for a while.
- **What the tests prove:** moving everything movable leaves every other file identical, on the real code path except the Trash itself (property test), and small real Rust and Node projects rebuild and run afterwards (end-to-end, offline). **What they don't:** that any real project with old dependencies still rebuilds, or that a running build survives. The first needs the network and depends on the outside world; the second is what the checks above and the `yes` are for. The automated tests never touch the real Trash, and they can't reach `y`, numbers or `--yes` moving a forgotten project end to end: a real file can't be made to look forgotten, because its ctime can't be set back.
