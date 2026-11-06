# Local collection evidence

Issue [#6](https://github.com/anasalqoyyum/parabolica/issues/6) adds the collection implementation on top of master `3b6924b3035ae9391d4c26413503e9cad4dd1ab6`. Its playback prerequisite merged in #40 as `8f9d69da5b18712b2fd370060d4658973bef3b1b`. The issue remains open for Windows runtime acceptance. The implementation PR and its final source and merge commits are recorded in the live issue discussion.

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

## Outstanding Windows acceptance

Keep #6 open with `status:blocked` until these checks have actual evidence:

- Run the native app and use Add music folder, search, Albums, Artists, Songs, Folders, album detail and grouping overrides with real files. Confirm an album plays through the Windows backend before the full scan finishes.
- Unplug and reconnect a removable drive while browsing and indexing. Confirm the collection stays visible, playback reports unavailable files, reconnect restores availability, and Locate folder preserves file and album IDs after a move or drive-letter change.
- Exercise real Windows Unicode/long paths, case-only renames, junction cycles, permission loss and recovery, concurrent file/tag changes and native watcher overflow or dropped events. CI fixtures cover only their temporary test cases.
- Verify embedded and sidecar covers, corrupt metadata and huge artwork in the native application. Confirm indexing cancellation and recovery remain responsive during playback.
- Use keyboard-only navigation and Narrator, including virtualized rows, search, folder removal confirmation and grouping. Check Light/Dark themes and 100/150/200% scaling at the supported window sizes.

The local album sequence is intentionally separate from #7's full queue, restoration, prefetch and gapless transition work. Real audio-device failures and the remaining #5 hardware checks stay with that issue. Provider accounts are not required for local indexing.
