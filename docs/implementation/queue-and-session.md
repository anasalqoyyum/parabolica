# Queue and session evidence

Issue [#7](https://github.com/anasalqoyyum/parabolica/issues/7) builds on master `754984d2f8dad70dc664e6a8a4c85ce85a91faa2`. Prerequisite implementations merged in #40, #41, #43 and #44. #5's separate hardware acceptance does not block its merged engine contract.

## App entry points and behavior

Run `cargo run -p parabolica-desktop --locked` on Windows. Albums and album detail play an ordered album through the full queue. Album detail and selected Songs offer Play next/later. Selecting songs also offers Like in app. Open file registers a persistent file identity and adds a new queue entry.

Queue in the sidebar shows effective playback order and unavailable states. Its controls play, move up/down, remove, retry up to three times, skip, clear, refresh availability, shuffle and cycle repeat off/all/one. Every duplicate has a distinct stable entry ID. Play next inserts immediately after current even in shuffle. Moving current keeps it playing; removing current selects its effective successor, or stops at the end. Removing the last entry stops even with repeat-all. Explicit next/previous ignores repeat-one. Previous follows actual listening history, then preceding effective order. Automatic playback stops at an unavailable entry and retains Retry/Skip.

Playlists offers Liked in app and named playlists. Create/rename uses Playlist name; deletion requires Confirm delete playlist. Add queue preserves duplicates, and playlist rows move/remove independently of the live queue. Play now/next/later creates fresh queue entries from the playlist's source identities. M3U8 import accepts relative Unicode paths and keeps missing files visible. Export reports identities that M3U8 cannot represent and writes no partial file. Choose a new destination; existing files are not overwritten. App likes do not write remote likes.

A queue worker owns resolution, engine events and SQLite operations. Every command and crossed entry boundary checkpoints order, seed/order, repeat, current/position, navigation, failures and listening history in one transaction. Position also checkpoints every 500 ms. A clean quit waits off the UI thread for its final checkpoint; forced termination restores the last committed one. Restoration selects the same entry paused and does not add duplicate queue entries or a history start until playback resumes.

Queue edits replace the upcoming engine sequence and may briefly rebuffer. Normal compatible transitions keep one renderer, ring buffer, device stream and resampler history. Repeat/shuffle use the same engine path. Source rate/layout changes flush the old converter tail and explicitly report reconfiguration in the player.

## Portable verification

Development environment is Linux x86_64, Rust 1.99.0, GPUI Kit 0.9.0. No Linux audio backend or desktop port was added. Tests use temporary SQLite databases, synthetic audio and the existing capture output. Headless GPUI checks establish command wiring and layout, not native Windows interaction or sound.

`crates/audio/src/engine/sequence_tests.rs` generates continuous synthetic WAV and verbatim FLAC fixtures without an external encoder. A continuous album split into lengths of 1, 7, 255, 1, 4709, 24000, 1, 5 and 33021 frames is compared against its unsplit reference at the renderer/backend boundary. Captures include every callback from first audio through Finished, including inter-boundary silence. Matching counts alone cannot pass if a callback underruns. The same comparison exercises 48 kHz stereo, 44.1 kHz stereo with conversion and 96 kHz mono with conversion, plus ring pressure. Repeat-one/all and shuffled orders, changed layouts/rates, paused restoration, stale seeks/replacements, failed prefetch and device loss have separate tests.

MP3 LAME/Xing trimming is checked at the backend boundary. Simulated reader metadata tests verify reported untrimmed delay/padding and audible-frame seeking without double trimming. Symphonia ignores MP4 edit lists; AAC priming that the reader does not report remains unknown and untouched. These simulations do not establish gapless AAC for files with unreported priming.

Domain command sequences check shuffle permutations, stable IDs, insertion, moves/removals and navigation. Storage tests cover playlist/like source identities, duplicates, M3U8 bounds/injection/non-exportable entries, database reopen and all-or-nothing session rollback. Controller tests use an output clock, actual SQLite and a forcibly killed child process. Headless queue/playlist tests drive the visible controls and native-dialog requests. All 250 workspace tests passed on Linux after both independent reviews. Formatting, workspace Clippy with warnings denied and `git diff --check` passed. The issue-plan generator validated 35 issues and left its generated output current. The desktop build also passed. The reviewed source head is recorded with the PR.

Commands run from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
python3 scripts/render_issue_plan.py
```

UI verification found and fixed a list sizing error and a clipped Retry control at a 512-logical-pixel content width. The queue UI test asserts that playback/move/remove/Retry buttons stay inside those bounds. A separate regression keeps pause/resume at its saved position without adding a listening start. The same UI sequence exposed an old engine failure remaining attached to Stop, which prevented the queue from acknowledging the stop. Stop now clears that failure, and shutdown can finish even while an engine acknowledgement is pending.

## Independent review

The standards reviewer inspected source commit `3330a52dc3eb19bad848a002ed812871aefc45c5` and found three issues. Recovery's duplicate-reference guard now includes playlists, likes and listening history. Bulk Like sends one command and commits the whole selection transactionally; an inbox rejection stays visible until Dismiss error. Playlist navigation now uses the focusable list-item pattern with `aria_selected`, including Liked in app.

Regressions cover recovery with each new reference table, atomic bulk-like rollback, 256 likes in one command, retained inbox errors across worker publications and accessible playlist selection. The full 250-test workspace, formatting, Clippy with warnings denied and desktop build pass after these fixes. The independent correctness reviewer found three P1 races in the same reviewed source. Engine acknowledgements now carry request IDs and snapshots on the ordered engine-event channel; a queue edit no longer requires the old current entry to acknowledge. Boundary handling resets position/stamp before changing the saved current entry. The worker drains at most 256 events or 8 ms of event handling per batch, checkpoints the accumulated state once and returns to command/shutdown checks.

Regressions force an A-to-B boundary during an edit, reject a stale request acknowledgement, verify the database has only the old A checkpoint or a coherent B-at-zero checkpoint, and run Pause/Clear/shutdown during one-frame repeat with real SQLite. The audio engine also verifies acknowledgement ordering when output-open failure leaves no current entry. All three correctness findings are fixed locally; actual Windows evidence remains separate.

The verified Linux code checkpoint is `48620f4e89558baae0f6f2bd6066dc877e2daba4`. It contains both review-fix commits. Later evidence-only commits do not replace the requirement to record the final `git rev-parse HEAD` during Windows runtime verification.

The single draft PR is [#46](https://github.com/anasalqoyyum/parabolica/pull/46), initially opened at `a552ccd3fc08a5bbf705dec365e59d9be79b3c04`. Its current head and Windows MSVC result are in the PR. The PR and #7 remain open and unmerged for Windows runtime acceptance.

## Windows acceptance gate

Keep #7 and its PR open and unmerged until the final integrated source commit has passed Windows MSVC CI and actual Windows runtime verification. A Linux capture test, a cross-check or a green Windows compile does not prove WASAPI gapless playback. The user did not waive this requirement for #7.

On the linked Windows WSL/PowerShell machine, use the same branch/PR and record `git rev-parse HEAD` before checking. Use `cargo.exe` through PowerShell on the worktree's UNC path, a dedicated Windows-drive Cargo target directory and disposable app data. Close `parabolica.exe` before rebuilding. Preserve the user's real library and queue.

Required native evidence:

- Drive the actual queue/playlist/library controls through UI Automation. Include duplicate IDs, Play now/next/later, reorder/remove current, history/previous, repeat off/all/one, shuffle next, likes, Unicode M3U8 and unavailable Retry/Skip.
- Save a paused seek and queue/shuffle/repeat state, exit normally, and confirm the same IDs/order/current/position restore paused. Repeat after forcibly terminating the app while playing, comparing with the last committed checkpoint and confirming no duplicate entries.
- Exercise an actual continuous split album through WASAPI Shared on a real endpoint. Record device and negotiated/source formats, rendered boundary evidence, underruns and listening observations. Include compatible repeat/shuffle, short tracks and explicit incompatible rate/layout changes. Keep exact PCM capture proof separate from native hardware observations.
- Verify failure recovery, narrow-window controls, cleanup and the exact source commit tested. Add evidence to this document and the issue before requesting merge authority.

Windows native queue/playlist behavior, clean/crash session restoration and actual WASAPI gapless playback are not verified from Linux. #7 stays open for that phase.
