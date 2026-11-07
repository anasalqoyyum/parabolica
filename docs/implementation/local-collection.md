# Local collection evidence

Issue [#6](https://github.com/anasalqoyyum/parabolica/issues/6) adds the collection implementation on top of master `3b6924b3035ae9391d4c26413503e9cad4dd1ab6`. Its playback prerequisite merged in #40 as `8f9d69da5b18712b2fd370060d4658973bef3b1b`. The collection merged in #43 as `be5495285a9c040d2db80268785b52962cdc9f8b`. Native Windows evidence and the user's subsequent acceptance decision are recorded below.

## Application entry point

Run `cargo run -p parabolica-desktop --locked` on Windows. Albums offers Add music folder. Metadata appears incrementally, and Play album dispatches the actual indexed file IDs in disc/track order. Songs, Artists and Folders browse the stored collection. Search uses stored FTS data. The next/previous page controls keep requests bounded, and the grid and song list virtualize visible rows.

Sources & output and Library settings list folder health. Locate folder reconnects an existing root without replacing its IDs. Locate file on an unavailable song recovers an explicitly selected matching file inside an indexed root. Add its containing folder first if necessary. Removing a folder requires confirmation and removes library records, while files on disk stay intact.

To override grouping, select songs in Songs or album detail. Selections survive page changes. Open the destination album and use Group into its title, or choose Create album group for a separate group. Reset grouping returns the selection to its current automatic groups. No action writes tags or PCM.

## Automated Linux evidence

Developed on Linux x86_64 with pinned Rust 1.99.0 and GPUI Kit 0.9.0. The application was built but was not launched as a Linux desktop port. No Linux audio output was added.

Formatting, Clippy, all 199 workspace tests and the desktop build passed on Linux. The implementation-plan generator validated 35 issues and left generated outputs current. Checks ran from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
python3 scripts/render_issue_plan.py
```

The integration fixtures use temporary SQLite databases and synthetic audio from `crates/audio/tests/fixtures`. The collection test constructs bounded FLAC comments around that fixture's synthetic PCM. Pictures are synthetic PNGs or deliberately oversized invalid payloads. Reference catalog artwork is not packaged.

Coverage in `crates/library/tests/collection.rs` includes incremental visibility, unchanged-file skipping, database reopen, FTS accents/CJK, sorting and pagination, two-disc albums, compilation artists, distinct release IDs, explicit regroup/reset, root relocation, offline preservation, missing-file recovery, cancellation before and during a pass, corrupt tags, oversized pictures, thumbnail bounds, Unicode and long paths, Unix symlink cycles, real Linux watcher updates, interrupted-job restart, and queue/source-link/custom-tag preservation. Review regressions cover raw MP3 ratings/ReplayGain/custom tags on changed-file rescans, destination-first moves between roots, invalid relocation interrupting an active scan, inaccessible child-directory recovery details, and WAV/MP4 header-count and total-metadata limits. Later regressions verify that a file-level digest read failure leaves the pass incomplete while readable files continue indexing, and that top-level navigation clears a previous folder-root filter. An interrupted metadata projection is invalidated in SQLite and retried even if the core file signature is current.

The service unit test injects a rescan flag and a watcher error to verify full reconciliation requests. These simulate overflow signals; they do not prove native Windows watcher overflow behavior. A Windows-only fixture creates and removes a junction cycle for CI.

`apps/desktop/src/ui/library/tests.rs` drives the GPUI Kit headless window through Add music folder, album detail, Play album, empty search, relocation and confirmed removal. It uses the real filesystem, indexer and storage. Another headless test verifies failure paths and the Rescan action in both collection and folder settings. A 100-album fixture checks a 96-item page, visible-row virtualization, the final page and replacement of a superseded search. Playback dispatch uses `NoOutput`, so this proves command wiring rather than audible playback. The existing shell and player tests also pass.

No 100,000-track search or startup benchmark has been measured. The budgets in TECH_SPEC remain targets for #26 and integrated acceptance.

## Windows CI

The existing Windows MSVC job runs lockfile validation, the domain dependency boundary, formatting, workspace Clippy/tests and the desktop build on each PR. Its actual result belongs to the PR's current head. A Linux result does not establish Windows success. The Windows junction fixture and long-path fixture are automated checks, separate from application and hardware acceptance.

## Native Windows evidence, 2026-11-07

The merged implementation ran on Windows 11 Pro 10.0.26300, x86_64, with Rust 1.99.0 MSVC and GPUI Kit 0.9.0. PowerShell launched the actual `parabolica.exe` from the WSL worktree using an isolated `LOCALAPPDATA` and a dedicated Windows Cargo target directory. Windows UI Automation drove the app and recorded its accessible controls. Disposable fixtures were removed after verification; the user's existing app database and source music were not modified.

- All 23 library tests passed locally on Windows, including native watcher updates, the junction cycle and long Unicode path fixtures. The native desktop build passed.
- Add music folder indexed both synthetic files and the user's 15-track Laur album, Memoria. Albums, Artists, Songs, Folders and album detail displayed stored results. Searches for Laur and Japanese text returned the expected records. Returning from one root's Browse view to Albums included the other root's album.
- Play album started the real album through WASAPI Shared while a separate 3,000-file synthetic pass was still indexing. One native snapshot simultaneously showed Pause and 412 indexed files. Windows Core Audio reported an active session for the application process with a nonzero signal. The player reported a 44.1 kHz FLAC source and 48 kHz output on LG ULTRAGEAR, NVIDIA High Definition Audio. This establishes Windows output-session activity, not listening-confirmed sound or bit-perfect output.
- Cancelling indexing stopped the pass after 2,937 files while the Windows playback session remained active. Removing that root required the native confirmation and kept all 3,000 source files on disk.
- Holding a changed file with Windows `FileShare.None` produced an error naming its path and Windows error 32. A readable file still indexed, and the pass remained incomplete. Releasing the handle and rescanning completed the pass. This is evidence for a sharing violation, not NTFS permission loss.
- A Japanese path over 330 characters indexed through the real app. A case-only file rename preserved file and album IDs. Moving a disposable root made it offline without deleting its collection; native Locate folder restored it at its new path with the same root, file and album IDs. A directory move does not establish physical drive unplug/reconnect behavior.
- Creating an explicit album group preserved the file ID and kept its new album ID through a rescan. Reset grouping restored its original album ID. Source-content hashes stayed unchanged.
- Light and Dark at 100% scaling displayed the real album cover and a synthetic sidecar cover. Truncated FLAC metadata produced a visible path-specific error; readable songs continued indexing. A sidecar over 9 MiB did not prevent the valid tagged file from appearing.

Native verification exposed a song-row layout bug: the main button took the full row width before the Select and Locate file controls. The body now takes the remaining flexible width and can shrink around those controls. The regression `song_selection_and_recovery_controls_fit_and_accept_pointer_clicks` failed with "Select is clipped" before that change and passed afterward. It checks bounds and pointer selection at 1000, 640 and 400 logical pixels, then removes the fixture and verifies that Locate file is visible and opens its file picker at each width.

The corrected native Windows build also displayed Select at both the normal window size and the minimum width. UI Automation Invoke changed the selection in each window. After removing the disposable file and rescanning, Locate file stayed visible at the minimum width and Invoke opened the Windows file picker; cancelling returned to the collection. Screenshots captured the corrected controls at 96 DPI. These native checks used the real application, separately from the headless regression.

The follow-up Windows MSVC checks passed: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked` and `cargo build -p parabolica-desktop --locked`. The workspace reported 202 passed and one ignored credential child-process helper, which its parent test invokes separately. The first full run rejected an overly long fixture-path literal as a possible embedded credential; splitting the path into the existing fixture-directory and filename joins fixed that test without weakening the credential scanner. The complete workspace rerun passed. Credential Manager reported no remaining `Parabolica` test entries, the test app was closed and its disposable files were removed.

## Acceptance decision and remaining limits

On 2026-11-07, the user removed physical drive unplug/reconnect and manual Narrator evidence from #6's completion requirements, asking that the app handle errors gracefully when they occur. Those checks were not performed and are not reported as passed. Offline preservation, reconnect/move identity, cancellation, per-file read failures and reconciliation remain covered by the filesystem fixtures and the native checks above.

The broader delivery retains its existing verification work: cross-feature recovery and data integrity in #24; complete keyboard, Narrator, DPI and localization acceptance in #25; large-library and audio reliability measurement in #26; and final integrated acceptance in #35. Actual NTFS permission loss/recovery, concurrent tag edits, native watcher overflow, physical removable-drive behavior, Narrator listening and 150/200% Windows scaling remain unverified here. Ordinary Windows watcher events and injected overflow handling have separate automated coverage. Narrow logical-width tests do not establish actual display-scaling results.

The local album sequence is intentionally separate from #7's full queue, restoration, prefetch and gapless transition work. Real audio-device failures and the remaining #5 hardware checks stay with that issue. Provider accounts are not required for local indexing.
