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

The single draft PR is [#46](https://github.com/anasalqoyyum/parabolica/pull/46), initially opened at `a552ccd3fc08a5bbf705dec365e59d9be79b3c04`. Its current head and Windows MSVC result are in the PR. The user instructed that the PR and #7 stay open and unmerged. Native Windows results are recorded below.

## Windows acceptance gate

Keep #7 and its PR open and unmerged until the final integrated source commit has passed Windows MSVC CI and actual Windows runtime verification. A Linux capture test, a cross-check or a green Windows compile does not prove WASAPI gapless playback. The user did not waive this requirement for #7.

On the linked Windows WSL/PowerShell machine, use the same branch/PR and record `git rev-parse HEAD` before checking. Use `cargo.exe` through PowerShell on the worktree's UNC path, a dedicated Windows-drive Cargo target directory and disposable app data. Close `parabolica.exe` before rebuilding. Preserve the user's real library and queue.

Required native evidence:

- Drive the actual queue/playlist/library controls through UI Automation. Include duplicate IDs, Play now/next/later, reorder/remove current, history/previous, repeat off/all/one, shuffle next, likes, Unicode M3U8 and unavailable Retry/Skip.
- Save a paused seek and queue/shuffle/repeat state, exit normally, and confirm the same IDs/order/current/position restore paused. Repeat after forcibly terminating the app while playing, comparing with the last committed checkpoint and confirming no duplicate entries.
- Exercise an actual continuous split album through WASAPI Shared on a real endpoint. Record device and negotiated/source formats, rendered boundary evidence, underruns and listening observations. Include compatible repeat/shuffle, short tracks and explicit incompatible rate/layout changes. Keep exact PCM capture proof separate from native hardware observations.
- Verify failure recovery, narrow-window controls, cleanup and the exact source commit tested. Add evidence to this document and the issue before requesting merge authority.

The Linux handoff above is supplemented by the native Windows verification below. #7 and PR #46 remain open as requested.

## Native Windows verification, 2026-11-07

The continuation started from PR head `860ab6a34662313877cb2b256a66d04c7aeb40ca`. The final application source tested on this machine is `7dcacd78c30c1cfe360572a3305ad7f86b52c745`. Later evidence-only commits leave that application source unchanged. Windows 11 Pro `10.0.26300`, x86_64, Intel Core i5-12400F, about 32 GiB RAM, Rust/Cargo 1.99.0 MSVC and GPUI Kit 0.9.0 were used.

PowerShell ran `cargo.exe` from `\\wsl.localhost\Ubuntu\home\real\.t3\worktrees\parabolica\t3-e17815a6`, with `CARGO_TARGET_DIR=C:\Users\doubl\.cargo-targets\parabolica-t3-e17815a6` and six build jobs. The app was closed before each rebuild. Its executable SHA-256 was `BACB5511E29717570993E599F75AF977149B3B0B18106184555D1AC7268B2FFD`.

All app data and fixtures were under `C:\Users\doubl\AppData\Local\Temp\Parabolica-Issue7-t3-e17815a6`; only the launched test app inherited its isolated `appdata` as `LOCALAPPDATA`. Music was synthetic PCM WAV and FLAC, with relative Japanese M3U8 paths, duplicates and a deliberately missing file. No real library, queue, account or driver setting was changed.

### Native UI and restoration

Windows UI Automation drove the actual `parabolica.exe`, its accessible controls and native file/folder dialogs. Pointer actions used bounds obtained from UIA. Seek used the native slider's UIA bounds and pointer input. It did not rely on the previously documented ineffective accessibility Increment/Decrement action. SQLite observations used read-only Windows Python connections, including the live Windows WAL, rather than WSL SQLite access.

The final native matrix passed:

- Album Play and Play next/later preserved disc/track order. Selected Songs supported next/later and app likes. Repeated insertion produced distinct stable queue-entry IDs for duplicate sources.
- Moving the playing current entry retained its identity and playback. Removing it selected its effective successor while playback continued. Next/Previous followed actual starts, including explicit navigation under repeat-one. History exposed its source names, and shuffle produced a full permutation with deterministic next/previous.
- Playlist create, rename, append queue, reorder, remove, play now/next/later and confirmed delete worked. Cancel preserved the playlist. Play now inserted fresh entries and selected the first, preserving the existing queue. App likes stayed separate from playlists and remote likes.
- Native M3U8 import/export preserved two relative Japanese paths to the same source and a missing path. The two queue entries had different IDs. The missing entry stayed visible; three failed Retry attempts disabled Retry. Skip at the end stopped. A new entry's Retry succeeded after the disposable file returned.
- At the 640-logical-pixel minimum client width, UIA bounds and a screenshot confirmed Play, Retry, move and Remove controls stayed within the content. Queue navigation and Remove icons were visible after the asset fix.

Clean exit restored the same six IDs, insertion order, shuffle positions/seed, repeat-all, current entry, navigation and listening history, paused at exactly `27.630000 s`. The paused position stayed still after restart. Forced termination while playing restored the same state and history without duplicate entries or an added listening start. Its last committed position was `42.830958 s`; the restored output-clock position was `42.830952 s`.

That 6 microsecond difference follows the engine's existing frame conversion. A stored time seeks to the first source frame at or after that time, then the UI position uses the floored output frame. The restoration assertion checks that exact conversion and a maximum one output frame difference, 23 microseconds for the tested 48 kHz source and 44.1 kHz endpoint. It does not require arbitrary sub-frame timestamps to survive unchanged. Clean and forced restoration snapshots are `final-clean-{before,restored}-db.json` and `final-crash-{committed,restored}-db.json` in the local evidence directory.

### Actual WASAPI endpoint and boundary captures

The default console render endpoint was `DAC (FiiO Q series)`, ID `{0.0.0.00000000}.{79d5e961-7833-43c0-b061-a8e716780e11}`. Windows reported the TUSBAUDIO driver `4.47.0.44597`. `GetMixFormat` returned `WAVE_FORMAT_EXTENSIBLE`, IEEE float, 32-bit, stereo, 44,100 Hz. The app reported WASAPI Shared on that same device. No endpoint, driver or Windows mix-format setting was changed. The device was enumerated as Q series; this evidence does not identify it as a K3 or establish ASIO behavior.

A separate native `IAudioClient`/`IAudioCaptureClient` loopback recorder captured the real endpoint while UI Automation started playback in the real app. It retained every packet, silent packets, device-frame positions and discontinuity flags. The app's own Windows audio session was active with a nonzero peak. The recorder did not replace the app's WASAPI backend.

Matching-rate fixtures used deterministic independent stereo PCM16 samples. Their split lengths were `88200, 1, 7, 255, 1, 4709, 22050, 1, 5, 33021, 88200` frames. Their total was 236,450 frames. Ordered playback was compared with the unsplit source. Repeat-all and repeat-one used tiled source PCM. The shuffle comparison concatenated files in the actual persisted shuffled order, then checked its repeat-all wraps. Comparisons used the full captured signal, including the short-track boundaries and wraps, with no boundary trimming or fitted time warping.

For conversion, six continuous two-second 48 kHz stereo WAV tracks and their FLAC equivalents were compared with the same app playing the unsplit WAV reference through the same endpoint. A second 48 kHz fixture used lengths `48000, 1, 7, 255, 1, 4709, 24000, 1, 5, 33021, 48000`, totaling 158,000 source frames. Comparisons checked exact float samples and signal extent, so periodic tone matching alone could not hide extra or missing output duration. No gain correction was needed.

| Native app sequence | Output frames checked | Complete cycles | Comparison |
| --- | ---: | ---: | --- |
| 44.1 kHz ordered split | 236,450 | 1 | Exact source PCM |
| 44.1 kHz repeat-all | 762,842 | 3 | Exact source PCM |
| 44.1 kHz repeat-one | 409,160 | 4 | Exact source PCM |
| 44.1 kHz shuffled repeat-all | 761,960 | 3 | Exact source PCM |
| 48 kHz WAV split to 44.1 kHz | 529,200 | 1 | Exact unsplit endpoint capture |
| 48 kHz FLAC split to 44.1 kHz | 529,200 | 1 | Exact unsplit endpoint capture |
| 48 kHz short split to 44.1 kHz | 145,163 | 1 | Exact unsplit endpoint capture |

All listed comparisons were exact, with zero differing samples, zero capture discontinuities during the signal and zero gaps in capture device-frame positions. This is native Windows endpoint evidence, separate from the portable renderer-boundary tests above. The player does not expose its internal cumulative underrun counter, so that counter was not read. The complete exact captures establish no inserted silence, dropped or repeated frames in the measured compatible output. They do not assign a numeric internal underrun total to startup or idle silence.

A separate native sequence changed `48000 Hz / 2 ch` to `44100 Hz / 1 ch` and back to `48000 Hz / 2 ch`. Both explicit Format change announcements appeared in UIA. The shared endpoint stayed at 44.1 kHz stereo. Continuous output across those incompatible boundaries is not claimed.

### Fixes, checks and review

Native inspection found that source titles and unavailable reasons were absent from the accessibility tree, and that Queue navigation and Remove icons were absent from the production asset bundle. Commits `5575393`, `d2fbcb0` and `7dcacd7` add the semantic labels, give repeated History rows independent ID scopes and embed `ListMusic`/`X`. The first label change exposed a duplicate History accessibility-node crash; the scoped-row correction and repeated-source regression cover it. Native UIA and screenshots verified the corrected behavior.

Windows MSVC checks passed on the final source:

```text
cargo.exe fmt --all --check
cargo.exe clippy --workspace --all-targets --locked -- -D warnings
cargo.exe test --workspace --locked
cargo.exe build -p parabolica-desktop --locked
```

All 254 Windows workspace tests passed. The queue-label, repeated-History and icon checks have failing and passing evidence from this continuation. An independent GPT-6.1 Sol High review approved the four-file continuation diff from `860ab6a` through `7dcacd7` in one round, with no findings. It did not claim the then-pending final native rerun had passed. The completed native result is recorded separately in `final-suite-result.json`.

### Artifacts, cleanup and limits

Local evidence is retained in `/tmp/parabolica-issue7-windows-20261107`: fixture generators/manifests, the UIA driver and action log, read-only database snapshots, final database backup, native loopback recorder/raw PCM/packet reports/comparisons, build/test logs, screenshots' paths and the append-only decision log. These are verification artifacts, not production assets. Screenshot captures remain in `F:\Libraries\Pictures\Screenshots`, including `parabolica-issue7-narrow-unavailable-20261107-234101-032.png` and `parabolica-issue7-native-session-before-crash-20261107-234116-349.png`.

The test app and recorder processes were closed. All 37 original fixture-file hashes stayed unchanged, and the native split PCM still matched its reference. Synthetic fixtures were archived as `synthetic-fixtures.zip`, and the final SQLite backup passed `PRAGMA quick_check`. The complete disposable Windows app-data/fixture directory was removed and its absence confirmed. `cmdkey /list:Parabolica-test*` reported no test credentials. The dedicated Cargo cache and evidence archive remain.

No human acoustic listening or analog capture was performed in that run. Loopback establishes the measured Windows render path, not DAC analog output, hardware DSP or bit-perfect delivery. Other endpoints, physical device removal, integer shared formats, ASIO, Narrator speech and other display scaling were not checked here. The separate #5/#9 hardware requirements remain separate. MP3/AAC native gapless behavior was not added to these fixtures; AAC edit-list priming was unresolved at that checkpoint and is addressed by the continuation below.

The requested native queue, restoration and compatible WASAPI checks passed on this endpoint. Human listening confirmation was pending at that checkpoint; the continuation below records it. Keep PR #46 unmerged and #7 open as requested; these results do not authorize merge or close unrelated hardware gates.


## AAC presentation timing and listening, 2026-11-08

Source `26aab5ce58b7106e81dcf7b4da266875aa803c5e` fixes the remaining AAC-LC presentation gap. A Windows regression on the existing encoded `lossy/sine.m4a` failed because delay was unknown even though its edit list declares 1024 priming frames. The wrapper now reads the selected track's presentation edit, removes the leading delay and stops at its declared audible end. The one-second fixture produces 44,100 frames, rather than including priming or the last packet's padded tail.

The metadata reader is bounded to 4096 atoms, skips media payloads, validates atom lengths and supports version 0/1 timing. It uses the movie duration as a bound and precise media timing to retain sample precision. Single, unit-rate edits are supported. Empty, dwell, rate-changing, multiple-segment and fragmented edits fail explicitly. Delay is still unknown for files without usable edit-list metadata; the decoder does not invent a priming value. A presentation end can trim padding without an independently reported padding-frame count, so that count remains unknown in `EncoderDelay` when absent.

AAC seeks now decode preceding packets to rebuild overlap state before returning the requested audible frame. Tests cover frame zero, either side of the 1024-frame priming boundary, the final frame, end clamping and a middle seek. Parser tests cover both timing versions, the selected audio-track ID, different timescales, rounded movie durations, malformed lengths, excessive atom counts and unsupported edits. MP3 and AAC sequences both match concatenated decoder PCM exactly at the backend boundary, with no double trimming.

Windows MSVC formatting, workspace Clippy with warnings denied, all 257 workspace tests and the desktop build passed. The new encoded-fixture regression and audible-zero seek check have failing and passing logs. The fixture files already in the repository were not edited or regenerated.

Native verification used fresh disposable data at `C:\Users\doubl\AppData\Local\Temp\Parabolica-Issue7-AAC-t3-e17815a6`, the same worktree-specific Cargo target and the default `DAC (FiiO Q series)` WASAPI Shared endpoint, still 44.1 kHz stereo float32. New synthetic WAV files were encoded with FFmpeg AAC-LC at 192 kbit/s and a 44.1 kHz movie timescale. Three four-second files yielded exactly 529,200 frames. Short files with 1, 7, 255, 1023, 1024, 1025 and 44,117 audible frames yielded exactly 47,452 frames. Actual endpoint captures matched the complete independently exported Windows decoder PCM byte for byte. Packet discontinuities and device-frame gaps were checked over the compared source intervals. Evidence is in `/tmp/parabolica-issue7-aac-20261108`.

The actual app then played the three-section AAC album under repeat-all, crossing a section boundary every four seconds and wrapping after twelve seconds. The user reported: "Smooth, no gaps or clicks". Playback was paused after that response. This supplies the previously missing human listening observation on the tested endpoint. It does not establish analog bit-perfect output or coverage of other devices and codecs.


An independent GPT-6.1 Sol High review approved the AAC continuation and its architecture/evidence documentation in one round, with no findings. The tested executable SHA-256 was `9634257C67A594893CF496CABA9A1775E4DBE21499A44E3C3E14B134701F1AA4`.

The test app and recorder processes were closed. All 24 synthetic fixture-file hashes remained unchanged; the files were archived with the local evidence. The final SQLite backup passed `PRAGMA quick_check`, and the disposable AAC app-data/fixture directory was removed. No `Parabolica-test*` credentials remained. The source fixtures and temporary PCM export helper are retained only in the evidence directory; the helper is not part of the application or repository.
