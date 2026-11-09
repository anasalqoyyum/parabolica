# ReplayGain analysis

Issue #10 implements ReplayGain scanning against `ce428813a9590bc5ee1b1a4cd2869ecb89694f2e`. This document separates automated measurements from native Windows evidence.

## Entry point and scope

Open ReplayGain in the sidebar, or choose Scan ReplayGain from album detail or selected Songs. Select tracks or whole albums, choose Scan tracks, Scan as one album, Scan albums by tags, or Group by disc. Review the proposed membership and any suspected missing tracks/discs, then accept the groups and scan. Results remain a preview until Save to app library. Scan history and complete track checkpoints survive restart. Review and resume creates a new reviewed run and reuses matching complete measurements.

This scanner reads files and stores results in SQLite. It never changes tags, artwork, or audio samples. Playback remains unchanged and ReplayGain remains off. Gain application and peak protection belong to #11; safe file writes belong to #12 and tag interoperability to #11. Source selection between app analysis and existing file tags is part of that playback contract.

## Measurement contract

`parabolica-replaygain::analyze::Analyzer` consumes interleaved f64 PCM. The desktop worker uses the same read-only Symphonia decoder as playback and reads until the decoder reports EOF, including its truncation/corruption checks. If the lossy decoder recovered by skipping damaged packets, the scanner refuses complete analysis of that file. Supported files are those of the existing decoder. Positioned mono, stereo and supported surround layouts map to BS.1770 channels. LFE contributes to peak measurement but not loudness. Unknown, discrete, ambisonic and unsupported positioned layouts fail explicitly. Different rates and layouts retain independent filters and block boundaries.

The pinned Rust `ebur128` 0.1.10 performs K weighting and optional true-peak interpolation. The analyzer records complete 400 ms block energies at 100 ms steps, using the library's rounded sample boundaries. Track loudness uses the -70 LUFS absolute gate and a relative gate 10 LU below the absolute-gated mean. Album loudness combines all members' block distributions before calculating the relative gate. It never averages track gains. Gain is `-18.0 - integrated_loudness_lufs`. Album peak is the largest member peak. No rounding occurs before presentation.

Sample peak is the maximum decoded linear amplitude. True peak uses ebur128's interpolation estimator, 4x below 96 kHz, 2x below 192 kHz and source-rate peak detection at 192 kHz or higher, with a 192-frame zero tail to drain interpolation delay after the last source sample. Padding never enters the stored gating distribution or frame count. Results identify the peak method; a true-peak result also retains the sample peak. This is a measured source estimate, not a guarantee about later processing or hardware.

Silence, content below the absolute gate, and files shorter than 400 ms have no finite gated loudness. Their fully measured frame count and peak remain available; gain is Unavailable, never infinite. Non-finite PCM or measurements fail. An album containing silent members can still have finite aggregate loudness. An entirely unmeasurable album has no album gain, but retains its measured album peak, including zero for silence.

## Grouping and validity

Automatic groups use the library's persistent release identity. Its grouping includes album artist, title, edition/release data and root/folder fallback, preserves explicit library overrides, and joins numbered disc folders. A matching title alone cannot merge two editions. The scanner can deliberately combine a selected set into one album or split by disc. Review warns about unavailable files, selected subsets, gaps/duplicate track numbers and declared totals. Totals flag suspicion and do not certify completeness. The user must accept the selected membership, including any warning.

Migration `0010_01_replaygain.sql` owns scanner runs, group snapshots, album associations, members and immutable complete measurement caches. Album review-to-start checks the full library membership transactionally, rejecting a concurrent regrouping. Insert/delete/regroup triggers invalidate every associated album group, including changes during scanning. Track-only groups do not depend on album membership. Finalization chooses Ready, Incomplete or Stale in one transaction, so a concurrent invalidation cannot publish a complete album. File-signature/presence changes and removal invalidate affected groups. Album publication requires every accepted member to finish and no invalidation. Failed or absent members leave the run Incomplete and have no album result. Cancellation keeps completed track checkpoints and leaves the run Cancelled. A failed worker operation becomes Failed when storage remains available to record it. Interrupted Queued/Scanning jobs become Cancelled on restart with a resume explanation.

Each measurement records stable file identity, full-file SHA-256, size and mtime, analyzer/algorithm version, -18 LUFS target, peak method, source sample rate/channel mask/frame count, measurement time, peaks, loudness and gating distribution. Cache reuse requires the same file ID, full digest, analyzer and peak method. The analyzer stamp must change when measurement behavior or decoded-PCM interpretation changes. A new decode fingerprints the file before and after processing. Saving fingerprints every member again and atomically refuses stale/incomplete groups. Metadata-only edits conservatively require revalidation because the fingerprint covers the whole file. A future #12 writer must preserve or retarget analysis only after its separate decoded-PCM equivalence check; an mtime guess is insufficient.

History reads scalar measurement summaries and file signatures without loading gating-energy distributions. Complete cache reads still validate and load those distributions for aggregation. The storage API exposes typed history, complete cached measurements and `published_analysis(file)` for the latest explicitly saved app selection. Consumers must honor the explicit saved flag and group staleness; an unsaved checkpoint is not the user's published analysis. File tags and app measurements remain separate sources. #11 must check current source identity/signature with the scanner's full fingerprint method before using app analysis. The indexer's sampled-content digest is a different fingerprint. Playback must not silently fall back to a different analysis source.

The worker is serial and all storage, file hashing and decoding happen off the UI thread. Cancellation and playback scheduling run between 64 KiB hash reads and at most 4096 decoded frames of analysis. When the player reports Loading or Playing, analysis waits and remains cancellable. It resumes when playback pauses/stops. No scanner operation runs on the audio callback. UI snapshots share their collection/history data through `Arc`; the track chooser renders visible rows with `uniform_list`.

## Independent reference fixtures

`tests/replaygain/generate.py` creates synthetic 16-bit FLAC fixtures. The signals and fixture files are dedicated to the public domain under CC0-1.0. No catalog recordings or artwork are bundled. Fixtures cover unequal-duration loud/quiet tracks at 44.1/96 kHz, gated 48 kHz audio, an intersample-peak signal, silence, sub-400 ms audio, mono, 5.1 with a loud LFE channel, and 192 kHz audio. Their exact SHA-256 values and independent measurements are in `tests/replaygain/references.tsv`.

The reference runner compiles the C implementation libebur128 1.2.6 from its pinned release archive, SHA-256 `baa7fc293a3d4651e244d8022ad03ab797ca3c2ad8442c43199afe8059faa613`. It receives PCM decoded by FFmpeg 8.0.1 and calls C `ebur128_loudness_global_multiple` for album aggregation. The application's tests decode the pinned FLAC with Symphonia and use the Rust analyzer, then compare against those recorded C measurements. This checks an independent implementation and decoder; the additional Rust multi-state test only checks internal consistency and is not independent reference evidence.

Tolerances are 0.1 dB integrated loudness/gain, 1e-10 linear sample peak, and 1e-5 linear true peak. The true-peak tolerance allows floating-point differences in the C/Rust interpolation implementations. The mixed-rate three-track album reference is -8.104275638582 LUFS, gain -9.895724361418 dB, and true peak 0.400234699249. No foobar2000 comparison has been performed yet; the independent pinned RG2 measurement route is used here.

Regenerate with `uv run python tests/replaygain/generate.py`, using FFmpeg 8.0.1, curl, tar and a C compiler. The script verifies the reference source checksum and records the encoder build. Generated fixtures/reference output must be regenerated through that script.

## Verification record

Evidence and the append-only local decision log are under `/tmp/parabolica-issue10`. Windows commands run through PowerShell on this worktree's UNC path with `CARGO_TARGET_DIR=C:\Users\doubl\.cargo-targets\parabolica-issue10-replaygain-analysis`. Native checks use only the disposable `Parabolica-Issue10-20261108` namespace and copied synthetic fixtures. The checks ran on 2026-11-08/09, on Windows 11 Pro build 26300 and WSL Real. Rust/Cargo 1.99.0 and the x86_64 Windows MSVC target were used. These were local validation runs.


### Automated checks

- `cargo fmt --all --check`: passed on Linux and through Windows `cargo.exe`.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed on Linux and Windows.
- `cargo test --workspace --locked`: 285 passed on Linux; 292 passed on Windows, with one existing credential child-process helper ignored. Its parent invokes the helper separately.
- `cargo build -p parabolica-desktop --locked`: passed on Linux and Windows.
- Domain dependency inspection: only UUID, error and their supporting crates; no UI, Windows, storage, audio or network dependency.
- Nine pinned independent C reference fixtures passed the documented tolerances. Tests also cover multi-state aggregation, silence/short-file album peaks, non-finite PCM, relative gating, LFE exclusion, grouping modes/subsets, cancellation, playback waiting, restart interruption, full-content cache invalidation with unchanged size/mtime, publication guards, persisted results, missing members and regrouping. A fully measured group with no album aggregation is Incomplete and cannot be published.

One Windows workspace run failed two existing audio-engine tests during overlapping validation. `device_loss_retains_position_and_default_identity_until_explicit_retry` failed its retained-position assertion. `direct_compatible_short_entries_deliver_exact_pcm_on_one_stream` reported 257 synthetic-output underruns against an expected zero. The complete workspace rerun with compilation idle passed. The cause was not isolated and no engine code was changed to mask the failures. The failed output remains in `windows-tests-transient-audio-failures.log`. This is separate from #48's earlier native 307-underrun observation; neither is claimed resolved by #10.

### Native application checks

UI Automation exercised the Windows accessibility tree of the real GPUI application, using only the dedicated binary and disposable data paths.

1. Added the synthetic folder through the native picker, indexed the two-disc album, selected the album on ReplayGain, chose True peak, reviewed all three members, accepted, scanned and saved. Native result was gain `-9.895724361418488` dB and peak `0.40023472905158997`, against independent C reference gain `-9.895724361418` dB and peak `0.400234699249`.
2. Started actual WASAPI Shared playback, set app volume to its minimum, and repeated the album. The app reported the LG ULTRAGEAR NVIDIA endpoint at 48 kHz. Analysis displayed Waiting for playback at 0/3 tracks and remained cancellable. Cancellation produced Cancelled with no album result. After pausing playback, a reviewed resume reused all three complete measurements and reproduced the album result.
3. Closed and reopened the app. Saved results, provenance and Ready/Cancelled history persisted. The native by-disc review produced one Disc 1 group and one Disc 2 group; Sample peak scanning showed their separate gains and linear sample peaks.
4. Moved one disposable fixture out of its library path. Existing affected results became Stale. Review showed Missing, scanning finished Incomplete, album result was unavailable and Save was disabled. Restoring the fixture allowed an accepted rescan to reuse unchanged measurements and save a complete result.
5. Added a fourth matching-album member. The previous saved album became Stale and its album gain/peak were withheld. Removed that temporary member afterward. Before any deliberate fixture mutation, and after restoring the original set, the three source-file SHA-256 values matched their initial values exactly.
6. In a second fresh namespace, scanned a selected silent fixture as one album. The app showed gain Unavailable, peak `0.000000` and No gated loudness. Save succeeded. Reopening, reviewing/resuming and saving on the initial validation binary retained the zero album peak and reused the track. The closed database records contain a null album gain and peak 0.0. All four copied fixture hashes remained unchanged.

Native accessibility activation exposed duplicate label IDs during development. The original ReplayGain navigation reproduced the panic twice. Stable role-specific label IDs and scoped track/group IDs fixed it; the same UI Automation navigation, review and result flows then passed. The headless platform does not activate the native AccessKit tree, so headless tests are not presented as coverage of that native failure.

Light and Dark result pages were visually reviewed at the normal window size, and the stacked layout was reviewed at 1000 × 780. Screenshots were saved only in the Windows Pictures known folder, resolved as `F:\Libraries\Pictures\Screenshots`:

- `parabolica-issue10-scanner-dark-final-20261108-235958-102.png`
- `parabolica-issue10-scanner-light-final-20261109-000001-028.png`
- `parabolica-issue10-scanner-narrow-final-20261109-000232-219.png`

Those captures record the result layout. Later changes corrected the progress counter label from albums to groups and changed backend album-result handling and tests; the silent-album flow was rerun on the initial validation binary. `native-actions.jsonl`, the named `*-uia.json` captures and the closed `*-db.json` snapshots distinguish native observations from fixtures. Live cross-OS SQLite inspection returned an I/O error once; database snapshots were subsequently taken only after closing the Windows app. Live state evidence comes from UI Automation.

The device refresh found `VID_2972/PID_0047` under the current Windows label FiiO Q series. No FiiO/ASIO stream, new listening result, driver change or hardware success is claimed here. Manual Narrator, 150/200% DPI and sustained scanner performance were not performed; broader acceptance remains under #25, #26 and #35. No foobar2000 comparison was performed. The independent pinned RG2 measurement route was verified for #10. Issue closure is separate from this implementation and its validation record.

### Pre-publication review corrections

Independent review with `gpt-6-luna` at max effort identified two gaps. R1 compared a by-disc selection only with optional tag totals, so omitting known tracks could pass without a warning. Disc review now compares against every library member of the same album/disc. R2 returned from failed save-time validation without persisting staleness. Save now invalidates the run when a member cannot be validated, while cancellation leaves it retryable.

Both regression tests failed before the corrections and passed afterward. All 68 desktop tests passed on Linux and Windows MSVC. Formatting, workspace Clippy with warnings denied on both platforms, and the Windows desktop build passed again. These are automated filesystem/storage checks; the earlier real-app evidence above remains separate.

`review-round2-source.json` records the corrected source fingerprint `d0c33776f3c6ebc83870cd6ad6811539a320b0647c9922ce8e4c14496e526a10`. `review-round2-binary.json` records Windows binary SHA-256 `EB0C891698FA0D6603FF73B094221303DB2E2C3BB90586C826220335E7816A33`.

The second review confirmed R1/R2 and identified R3, which loaded full gating distributions for every history member. History now uses a separate scalar/signature projection; full cache reads remain strict. Its regression failed before the change and now passes even when the unused aggregation payload is malformed. R4 removed the unsupported publication/CI claim. R5 renamed the disc-capable progress counter to groups.

After R3–R5, all 68 desktop tests, 14 storage unit tests and 25 storage integration tests passed on both Linux and Windows MSVC. Formatting and workspace Clippy with warnings denied passed on both platforms; the Windows desktop build passed. `review-round3-source.json` records source fingerprint `5cb3a18bb226671984c07d970e093f7e6a03e567932135eea244f5ec2a624ee0`; `review-round3-binary.json` records binary SHA-256 `7FDD0D92FA4D87BA16120536D905731DA15DC875C36F7D3E47606E1E0A6875C1`.

The round-3 binary was also exercised through native UI Automation in `Parabolica-Issue10-round3-20261109`. A fresh two-disc true-peak album scan displayed gain -9.90 dB and peak 0.400235, saved, and reopened with the saved history and per-track values intact. Reviewing/resuming reused all three tracks; saving succeeded again. Copied fixture fingerprints were unchanged. `review3-native-actions.jsonl`, the named `review3-native-*-uia.json` captures and `review3-native-result.log` record this check. Initial script attempts used insufficient picker timing and a case-sensitive label mismatch; those attempts are retained as failures. The corrected run passed. Cleanup confirmed the disposable namespace absent, no owned process and no `Parabolica-Issue10*` credentials.

The third review found R6, repeated library scans per output group, and R7, a cancellation button during refresh/review operations that do not observe cancellation. R6 now indexes file IDs, album membership and disc counts once; track-only groups carry no album snapshots. The grouping regression checks empty track snapshots and complete album/disc snapshots, and failed before this correction. R7 now publishes cancellation availability for scanning/file validation only, clearing it before history reload. Its rendered GPUI regression failed during refresh before the UI fix, then checked all four operation types successfully. The playback-priority service test also checks cancellation availability and its completion reset.

After R6/R7, all 69 desktop tests passed on Linux and Windows MSVC, including independent fixture comparisons. Formatting and workspace Clippy with warnings denied passed on both platforms; the Windows desktop build passed. Logs are `review-r6-r7-*.log`; source fingerprint is `4571bdef6c8a94eb572727025550d64c39e4dc15089cd5a58790eec64b1e4885` in `review-round4-source.json`. Windows binary SHA-256 is `743F71DF7155B5B3C60D4E1866EAE6C1E9BE6DA560D3FABB8C0D1D01168E5427` in `review-round4-binary.json`. No timing benchmark was performed.

The round-4 binary passed a fresh native UI Automation scan/save/reopen/resume/reuse/save run in `Parabolica-Issue10-round4-20261109`. All three track measurements were reused after restart, fixture fingerprints stayed unchanged, and cleanup confirmed no namespace, owned process or `Parabolica-Issue10*` credentials remained. `review4-native-result.log`, `review4-native-actions.jsonl` and the `review4-native-*-uia.json` captures record this separately from headless checks. An initial driver attempt ran before folder selection/indexing had completed; `review4-native-attempt1.log` preserves that failed attempt. The passing driver checks for a remaining owned folder dialog and waits for indexed candidates.

### Source identity and cleanup

Base commit is `ce428813a9590bc5ee1b1a4cd2869ecb89694f2e`. The initial validation manifest `final-source.json` contains per-file SHA-256 values and the aggregate source fingerprint `bfd7c34d3f28517a2a73cb2872ac213972d461981ea95ad7a3003485a40e180f`. The aggregate hashes sorted source/manifest/lock/migration/fixture/generator paths and their hashes; Markdown is excluded. The initial native-validation `parabolica.exe` SHA-256 is `1698E82184BA14A74589C5710817B5AE174DBC586EBED98CB4E7C5FC461D680C`. Earlier source manifests and the earlier UI binary fingerprint are retained with the evidence.

Both `Parabolica-Issue10-20261108` and `Parabolica-Issue10-silence-20261109` were removed after closing their owned test apps. Cleanup confirms both namespaces absent, no owned process remains and `cmdkey /list:Parabolica-Issue10*` is empty. Two synthetic Credential Manager entries from the workspace tests remained and were deleted by their exact process/timestamp targets. Entries predating this assignment were preserved. `credential-owned-cleanup-confirmation.txt` records the OS confirmation.

The dedicated Cargo target directories, reference source archive, local evidence and Pictures screenshots remain for review. No real library, app settings, credentials or Windows settings were changed. The implementation and native checks described here ran locally. This local record does not include publication or CI evidence.
