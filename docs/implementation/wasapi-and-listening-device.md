# WASAPI and Listening device

Issue [#8](https://github.com/anasalqoyyum/parabolica/issues/8) is implemented locally in the provisioned `feat-issue-8-wasapi-exclusive` worktree. The initial implementation and hardware verification made no Git or external changes. The user subsequently authorized PR creation, babysitting and conditional squash merge to master. Issue closure remains separate. Required physical-device and sleep observations are separate from the automated results below.

## Entry point and behavior

Run `cargo run -p parabolica-desktop --locked`, then open Settings > Listening device. Select WASAPI Shared or Exclusive and an active endpoint. Open file or the existing queue/playlist controls supply local audio. Output changes pause playback; Play opens the selected output at the retained position.

The initial default resolves to a stable Windows endpoint ID and is saved. Changing the Windows default later does not reroute playback. Shared opens the selected endpoint's current mix format. Exclusive Follow source requests the source rate; Fixed requests the chosen rate and resamples once when needed. Unsupported formats and busy endpoints block playback and offer explicit retry, fixed-rate, Shared and endpoint choices. Releasing another player's Exclusive stream does not automatically resume this app.

Current stream reports the opened backend, endpoint, sample representation, valid/container bits, rate, channels, buffer frames/duration, reported latency, mixer participation, resampling and underruns. An unavailable latency stays Unavailable. Buffer duration is not latency. The page makes no bit-perfect claim. Device capabilities refresh every three seconds and before retry; inactive endpoints retain their reported name for missing-selection recovery.

Compatible entries keep one stream and converter history. An Exclusive rate, channel-count or precision change drains the old segment, waits its reported latency plus one buffer duration, and opens a new stream. Incompatible transitions have no continuous-output guarantee. An unsupported next rate selects that next entry at zero and blocks rather than playing it at the previous rate. Source identities, queue positions and history remain intact through recovery. A first successful output retry records a listening start once; recovery of an already playing entry does not add a start.

`OutputSelection` separates backend, endpoint/driver identity and rate policy. ASIO identities remain distinct from WASAPI endpoint IDs, including in persisted preferences. ASIO is unavailable in this build and never falls back to WASAPI. Issue #9 owns its driver, mapping, buffer controls and implementation. The domain and native output changes, including the Windows-only audio-to-platform dependency, are documented in [ARCHITECTURE.md](../ARCHITECTURE.md#listening-device-and-wasapi-negotiation). No #15 provider files were changed.

## Source and environment

The worktree started clean at merged master commit `5f75aff7b9e3a95aa78786520972a2a18ec4c12e`. PR #40's engine commit `8f9d69da5b18712b2fd370060d4658973bef3b1b` is an ancestor. Live issue #8 had no comments. The latest #5 comment and merged #7 evidence were read before implementation.

The initial hardware verification used an uncommitted patch over that base. The SHA-256 fingerprint of the sorted Rust-source and Cargo/toolchain manifest hashes is `a4e6ceab3e9f8c82dbf5522c5b92b235c0cebed566e7dd716c0df267926ed3d6`. `source-manifest.json` records every path/hash and the fingerprint algorithm's input. The executable SHA-256 is `4CDE0FCB6FB20796BAE51308B932CD22A6C6DEA5C82D0550A74F4BDFFF6466BF`.

Windows 11 Pro `10.0.26300`, x86_64 MSVC, Rust/Cargo 1.99.0 and GPUI Kit 0.9.0 were used. PowerShell ran Cargo on `\\wsl.localhost\Ubuntu\home\real\.t3\worktrees\parabolica\feat-issue-8-wasapi-exclusive`, with `CARGO_TARGET_DIR=C:\Users\doubl\.cargo-targets\parabolica-issue8-wasapi-exclusive` and six build jobs. Only this worktree's test app was closed before rebuilds.

The test app inherited `LOCALAPPDATA=C:\Users\doubl\AppData\Local\Temp\Parabolica-Issue8-wasapi-exclusive\appdata`. Fixtures are synthetic PCM WAV and M3U8 files in that disposable namespace. The user's real app data, library, account credentials, Windows mix settings and drivers were not changed. Windows Python read the live disposable SQLite database read-only, including its WAL. Runtime interaction used the real app's UI Automation tree and native file dialogs. Pointer scrolling used UIA window bounds.

## Windows checks

The following passed on the current source:

```text
cargo.exe fmt --all --check
cargo.exe clippy --workspace --all-targets --locked -- -D warnings
cargo.exe test --workspace --locked
cargo.exe build -p parabolica-desktop --locked
```

All 272 workspace tests pass after the review follow-up below. The initial hardware checkpoint passed 270. One pre-existing real Credential Manager test remains ignored. The added checks cover Exclusive-compatible short entries delivering exact PCM on one stream, rate-driven reopening, fixed-rate conversion, unsupported next-rate blocking, busy recovery, stable default identity and position after loss, persisted backend/rate/driver identity, invalid preferences, pause-on-selection and retry/history behavior. Windows tests also verify packed PCM24 and 24-in-32 output, including zero container padding. Existing callback allocation and queue/gapless checks pass. These test backends do not prove a physical DAC's behavior.

## Native endpoints and negotiated streams

The current active list contained Steam Streaming Speakers, DAC (FiiO Q series), Headphones (High Definition Audio Device), and LG ULTRAGEAR (NVIDIA High Definition Audio). The Windows default was the FiiO endpoint, ID `{0.0.0.00000000}.{79d5e961-7833-43c0-b061-a8e716780e11}`. Windows reported FiiO driver `4.47.0.44597`. That label does not identify a K3 revision or establish ASIO capability.

The real app opened these streams:

| Endpoint and mode | Source | Opened stream | Buffer | Reported latency |
| --- | --- | --- | --- | --- |
| FiiO Shared | PCM24 stereo 96 kHz | float32 stereo 44.1 kHz, resampling active | 970 frames, 22.00 ms | Unavailable |
| FiiO Exclusive Follow source | PCM16 stereo 44.1 kHz | PCM16 stereo 44.1 kHz | 132 frames, 2.99 ms | 2.99 ms |
| FiiO Exclusive Follow source | PCM24 stereo 48 kHz | 24-in-32 stereo 48 kHz | 144 frames, 3.00 ms | 3.00 ms |
| FiiO Exclusive Follow source | PCM24 stereo 96 kHz | 24-in-32 stereo 96 kHz | 288 frames, 3.00 ms | 3.00 ms |
| FiiO Exclusive Fixed 48 kHz | PCM16 stereo 44.1 or 12.345 kHz | PCM16 stereo 48 kHz, resampling active | 144 frames, 3.00 ms | 3.00 ms |
| LG Shared | PCM16 stereo 44.1 kHz | float32 stereo 48 kHz, resampling active | 1056 frames, 22.00 ms | Unavailable |
| LG Exclusive | PCM16 stereo 44.1 kHz | PCM16 stereo 44.1 kHz | 160 frames, 3.63 ms | 3.63 ms |

UIA reported zero underruns in the timed matrix and short-sequence observations. A later extended-play snapshot before physical removal reported 307 cumulative underruns. The user still reported continuous sound without gaps or clicks. That later counter is preserved in `before-physical-disconnect-uia.json`; its cause has not been isolated, and zero-underrun extended playback is not established. This is the app's counter, not an analog-output measurement. The FiiO query list included 44.1 through 384 kHz at some probed sample representations; actual playback was checked only at the formats listed above. LG's probed Exclusive rate list was 44.1 and 48 kHz.

A separate native `IAudioClient` was rejected with `AUDCLNT_E_DEVICE_IN_USE`, `0x8889000A`, while the app held Exclusive. In the reverse case, a disposable native holder owned the endpoint. The app showed the busy error with no open stream, retained the selected entry at zero, remained blocked after the holder closed, and opened the same endpoint only after Retry selected output. No device configuration was written by the holder.

A 12,345 Hz source failed in Follow source with no stream and no fallback. Choosing Fixed 48 kHz explicitly recovered it. An automatic playlist transition into that unsupported source selected it at zero and blocked. An automatic 44.1/48/96 kHz playlist opened each source's rate with resampling off. The six-part compatible 44.1 kHz repeat included one-frame and seven-frame entries; the app remained on Exclusive with zero reported underruns.

A native clean restart preserved the exact FiiO ID, Exclusive, Fixed 96 kHz, current queue-entry ID and `1.261000 s` position, restored paused. Endpoint switching used the actual Listening device controls. The 640-logical-pixel client width kept rate controls within the settings column and the persistent player visible. Native Dark screenshots were inspected; the existing semantic theme system is retained.

## Actual Shared endpoint capture and stall simulation

A separate native WASAPI loopback recorder captured the real FiiO Shared endpoint while the actual app played the compatible split sequence under repeat-all. Its mix format independently reported extensible IEEE float32 stereo 44.1 kHz, matching the app.

All 396,900 recorded frames, nine seconds, matched the synthetic source PCM exactly at a cyclic offset of 7,342 frames. The period was 176,663 frames. The comparison used the entire capture, including multiple wraps and short-entry boundaries, with no sample trimming, gain fitting or time correction. The recorder's initial packet had the normal startup discontinuity flag; packet continuity after that initial packet is checked separately in the local report. Exclusive output cannot be established by a Shared loopback capture.

Suspending only the isolated app process for 3.5 seconds exercised the long-callback-gap path on native Windows. After resume the app showed no open stream, retained the entry and position, and stayed blocked until explicit retry reopened the FiiO endpoint. This is a process-stall simulation. It does not verify actual Windows sleep, USB removal, or physical reconnect.

## Human listening

The user listened to the actual isolated app playing the six-part compatible synthetic sequence under repeat-all through the FiiO WASAPI Exclusive endpoint, PCM16 stereo 44.1 kHz. The user reported: "Continuous, no gaps or clicks". This supplies the acoustic listening observation for the tested sequence and endpoint. It does not prove analog bit-perfect output or coverage of other formats/devices. The report is retained in `human-listening.json`.

## Physical disconnect and Windows sleep

After the listening response, the user unplugged the FiiO USB connection while the app played the compatible repeat. The app closed its stream, exposed Play and Retry selected output, and reported `Selected endpoint unavailable: DAC (FiiO Q series)`. It retained the Exclusive FiiO ID and selected queue entry at `0.073287 s`. When the user reconnected USB, the endpoint returned through enumeration while the app kept exactly that entry and position with no open stream. Explicit Retry reopened the same ID as PCM16 stereo 44.1 kHz, with a 132-frame buffer and zero initial underruns. The queue then continued its normal repeat transitions. `physical-unplugged-result.json` and `physical-reconnect-result.json` retain these observations and the before/after UIA/database snapshots.

The user then put Windows to Sleep from the Start menu, waited, woke and unlocked it. Windows Power-Troubleshooter event 1 independently records sleep at `2026-11-08T04:42:38.215073200Z` and wake at `04:42:59.974490000Z`, about 21.76 seconds later, with Power Button as the wake source. No app interaction preceded inspection. After wake the app had no open stream and showed `the audio device stopped responding`, Play and Retry selected output. It retained the same Exclusive FiiO ID and selected entry at `1.546734 s`. Explicit Retry reopened the same endpoint at 44.1 kHz. Playback was paused after that confirmation. This is actual user-driven Windows sleep/resume evidence, separate from the earlier process-stall simulation. The snapshots and result are named `physical-sleep-*`.

## Review follow-up and publication checks

The user authorized PR creation, babysitting and conditional squash merge after the initial verification. Independent GPT-6 Luna Max review found three issues, addressed before publication:

- R1: the controller probes a source before opening output, then the worker opens it again. A replacement between those opens could previously render with the old Exclusive rate or precision. The worker now checks the actual decoder format before preparing PCM and fails visibly if it no longer fits. Output failures release the stream. A deterministic test replaces the file during output negotiation with different-rate and higher-precision FLAC. It failed with Playing on the old code and now blocks with zero emitted frames.
- R2: a failed first save of the default output previously skipped engine selection and session restoration. That save error is now reported locally while the same selected endpoint reaches the engine and the paused session still restores. A disposable SQLite trigger rejecting only `audio.output` inserts reproduced the old native app showing Nothing playing. The rebuilt app restores the original queue IDs and position, opens the chosen Shared FiiO endpoint paused, and exposes the persistence warning. Removing the trigger permits normal selection and persistence.
- R3: a successful enumeration clears its own refresh error while preserving preference/save errors. A separate refresh-error field and state regression verify that distinction.

The crash-restoration test now waits for the engine acknowledgement and checks the exact source-frame-aligned position, plus a less-than-one-frame bound against the microsecond checkpoint. The old assertion could sample an intermediate snapshot and expected arbitrary microseconds to survive frame rounding exactly. This matches the already documented queue/session precision policy.

The fixed-source fingerprint is `d3329c8b29ea3ea02594b3ff472108ff5261f5a95f66d5ba6ffa1e5390192534`, recorded in `/tmp/parabolica-issue8-send-20261108/review-fixed-source-manifest.json`. The rebuilt executable SHA-256 is `E086E378A1DC758D1754089C32C52F203E9936A4541983C50E0F3E597457D39A`. Windows formatting, workspace Clippy with warnings denied, all 272 tests and the desktop build passed. Native UIA rechecked the injected startup save failure and ordinary Exclusive Follow source, Exclusive Fixed 48 kHz and Shared playback on this executable. The earlier physical unplug/sleep and loopback results belong to the initial source/executable checkpoint above; those manual operations were not repeated for these guards and error-state fixes.

Fresh native repeat runs on the initial executable stayed at zero underruns through 37.19 and 309.35 seconds, with 60 observations in the longer run. They did not reproduce or explain the earlier extended-session count of 307; no cause or performance improvement is claimed. The reviewer found no code evidence establishing that count's cause. This remains a recorded verification limitation.

Continuation evidence is in `/tmp/parabolica-issue8-send-20261108`, including failing/passing native startup snapshots, the rejecting-trigger generator, source manifest, Windows logs and native rate/backend checks. The test app was closed again. All 17 fixture hashes remained unchanged, the final SQLite backup passed quick_check, the disposable namespace was removed and its absence confirmed, and no test credentials remained. The continuation backup SHA-256 is `edea964976aea83d266ce9009a732ad7e670afb3f9ad55d0e3a343689521f77f`.

## Artifacts and remaining checks

Local evidence is in `/tmp/parabolica-issue8-20261108`, including source/fixture manifests, patch, PowerShell build/check scripts and logs, UIA actions and snapshots, read-only database observations, native holder and loopback helpers, full PCM/packet metadata, comparisons and `decisions.tsv`. Screenshots were saved through the Windows Pictures known-folder API in `F:\Libraries\Pictures\Screenshots`, with the prefix `parabolica-issue8-`. They are native app captures, not design mockups.

Human Exclusive listening, physical unplug/replug and actual Windows sleep/resume are confirmed on the tested FiiO endpoint. Other endpoints, Bluetooth, an integer Shared mix format, multichannel output, Narrator speech and other display scaling have not been verified. ASIO remains #9. Neither CI nor the test capture backend substitutes for those checks. Keep issue #8 open while its required external validation remains unmet.

The test app was paused, then closed normally after the human checks. All 17 fixture-file hashes were unchanged before removal. The final native SQLite backup passed `PRAGMA quick_check`; its SHA-256 is `821c16f72136e9be9eb0b7404ad9486c2a686f11096a90def9779323ca4622ea`. Synthetic fixtures and the tested source are archived in the evidence directory. The complete disposable Windows app-data/fixture namespace was removed and its absence confirmed. `cmdkey /list:Parabolica-test*` found no test credentials. The dedicated Cargo target directory, evidence and Pictures screenshots remain. `cleanup-result.json` records the exact target and cleanup outcome.
