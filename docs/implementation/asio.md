# ASIO playback

Issue [#9](https://github.com/anasalqoyyum/parabolica/issues/9) builds on merged master `234eb5ef48ff021f4565d61fdc99c1d873b45955`. The merged #5, #7, #8 and #14 interfaces are its prerequisites. Issue #48 tracks a separate WASAPI extended-session underrun observation; this implementation does not resolve or investigate it.

Implementation and hardware verification ran locally. The owner subsequently authorized committing/publishing a PR, babysitting its checks and reviews, and closing #9 only if its acceptance criteria are clearly satisfied. Merge is not authorized. Native and fixture evidence below retains its original source/executable checkpoints.

## Entry point and behavior

On Windows, run the desktop app and open Settings > Listening device > ASIO. Select a compatible installed driver, then use Open file, the library, or a playlist to play local audio. Output changes pause at the retained position. Play or Retry selected output opens the selected driver. There is no fallback to another driver or WASAPI.

Registry enumeration reads both Windows architecture views and checks each registered DLL's PE machine type. A driver is identified by its canonical CLSID and backend. Its WASAPI endpoint ID cannot select it. Incompatible registrations stay visible with their architecture and reason. Refresh drivers reads the registry again; explicit refresh also queries the selected driver's capabilities. Automatic registry refresh does not open ASIO drivers, start audio or resume playback.

The page reports queried output names/sample types, individual supported rate queries, buffer bounds, granularity and preferred size. Follow source asks for the source rate, including rates outside the displayed probe list. Fixed requests the selected rate and uses the existing single resampling stage when needed. Unsupported requests fail visibly. The driver determines the output sample representation; PCM conversion and app gain remain represented by the existing native signal path. Integer output rounds and clips to the reported valid precision, without dither. DSD sample types are rejected.

Left/right selection uses zero-based driver indices internally and one-based labels. Selecting the other side's channel swaps the mapping, preserving distinct outputs. Mono is duplicated by the existing engine onto the selected stereo outputs. A wider source than the selected mapping fails. Compatible entries keep one stream. A source-following rate change drains and renegotiates outside the callback.

Driver preferred re-queries the preferred buffer on each open. Explicit frame requests are offered only when the driver advertises variable sizes and must satisfy its bounds/granularity. A successful ASIO `createBuffers` accepts that frame count; otherwise playback fails. Current stream reports that accepted size and its computed duration. Reported output latency comes separately from `getLatencies`, queried after buffer creation. Neither is a measured round-trip latency.

Open driver panel pauses and releases the stream first, then calls the installed driver's `controlPanel` on an STA worker. Unsupported panel calls produce an error. The integration cannot predict panel support without invoking this API. The panel may change the driver's settings only through the user's interaction. A driver call that exceeds the five-second timeout retains its owner and lease until it returns; the app reports the timeout and cannot open a competing driver in the meantime.

Reset, resynchronization, sample-rate/clock, buffer-size and latency notifications atomically mark the stream invalid. The callback writes silence after invalidation. A callback gap over two seconds, including suspend, also blocks before consuming another frame. The controller closes the stream, retains selection/position, and requires explicit retry, which reloads and re-queries the same driver. Returning hardware never resumes playback automatically. A hung driver teardown detaches its owner after five seconds and retains callback memory and the driver lease until it exits.

Windows mixer and session volume are outside the ASIO signal path. App volume and format conversion remain active where applicable. No bit-perfect claim follows from selecting ASIO.

## Backend and build

The production backend is a small native C++17 bridge to the ASIO SDK's `IASIO` interface, with Rust owning selection, conversion and callback lifetime. CPAL 0.18.2 also passed a separate Windows MSVC feasibility build with its `asio` feature, asio-sys 0.4.0, the same SDK, and libclang from `C:\Program Files\LLVM\bin`. The executable reported compiled hosts `[Asio, Wasapi]`; its manifest, lockfile and build log are archived under `/tmp/parabolica-issue9-20261108/cpal-feasibility`. It was inspected as the specification's candidate. Its output callback takes `asio_streams.lock()` in `src/host/asio/stream.rs`; that violates this repository's no-lock callback contract. The native alternative also exposes channel types, buffer limits, driver notifications and panel access directly.

The SDK is **Steinberg ASIO 2.3.4**, downloaded from `https://www.steinberg.net/asiosdk`. The archive SHA-256 is `d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca`. Setup fails if that mutable URL serves different bytes. There is no network download during Cargo's build. `crates/platform/build.rs` additionally verifies the exact SHA-256 of all three included SDK headers before compiling the bridge. The SDK stays outside the repository. C++ build dependencies are `cc =1.11.0` and `sha2 =0.10.9`, locked in `Cargo.lock`; registry access uses `winreg =0.56.0`. Rust and GPUI pins remain those in ARCHITECTURE.md.

Windows needs Visual Studio C++ build tools and a Windows SDK, plus the existing project's CMake requirement. The native bridge needs no LLVM, libclang or generated bindgen bindings. From PowerShell at the repository root:

```powershell
. ./scripts/setup-asio.ps1
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
cargo run -p parabolica-desktop --locked
```

The desktop enables its `asio` feature by default. `--no-default-features` deliberately builds without native ASIO; a saved ASIO selection stays selected and fails as unavailable. The platform/audio crates enable ASIO explicitly when used independently. For read-only native enumeration and capability inspection, without starting audio or setting a rate:

```powershell
cargo run -p parabolica-platform --example asio_devices --features asio --locked
cargo run -p parabolica-platform --example asio_devices --features asio --locked -- '{driver-clsid}'
```

Both Windows CI workflows prepare the same hash-verified SDK. `windows-asio.yml` checks the ASIO callback/fault fixtures, the desktop ASIO build, and the explicit WASAPI-only build. CI has no attached K3 and proves no hardware behavior.

## License and dependency notice

The downloaded ASIO SDK 2.3.4 is copyright Steinberg Media Technologies GmbH and is dual licensed under GPLv3 or the proprietary Steinberg ASIO license. Its `common/LICENSE.txt` says proprietary publication requires a signed Steinberg agreement and forbids SDK redistribution without prior written agreement. The GPLv3 option carries its own distribution obligations. The setup script does not choose a project license or accept a proprietary agreement on the owner's behalf. No SDK headers, SDK source, archive, logos or driver binaries are vendored here. The application license and public distribution remain open product decisions; assess the chosen license before distributing an ASIO-enabled binary. ASIO is a Steinberg trademark. No ASIO logo is packaged.

The project's bridge code calls the installed driver. `cc` is MIT/Apache-2.0, `sha2` is MIT/Apache-2.0, and `winreg` is MIT. Their exact source checksums and transitive dependencies are in `Cargo.lock`. Existing Symphonia/GPUI and other dependency notices still apply.

## Local checks and source checkpoint

Evidence is under `/tmp/parabolica-issue9-20261108`, separate from the preserved #8 archives. Windows Cargo runs through PowerShell on this worktree's UNC path, with `CARGO_TARGET_DIR=C:\Users\doubl\.cargo-targets\parabolica-issue9-asio-playback`. No drivers, Windows audio settings, or real library/settings have been changed.

Native enumeration and initialization succeeded. The ASIO CLSID is `{6b3ba606-8664-4426-8994-0f1e6fe6199f}`. The compatible DLL is `C:\Program Files\FiiO\FiiO_Driver\W10_x64\FiiO_UsbAudioAsio_x64.dll`, file version `4.47.0.0`. Its separate 32-bit registration is correctly reported incompatible with the x64 app. The driver reports driver-version code `1071`, two outputs named `Analogue 1/SPDIF 1` and `Analogue 2/SPDIF 2`, both `ASIOSTInt32LSB`, 32 valid/container bits. Individual rate queries accepted 44100, 48000, 88200, 96000, 176400, 192000, 352800 and 384000 Hz. At initial inspection the current rate was 44100 Hz and buffer limits were 8–2048 frames, preferred 64, power-of-two granularity.

Windows PnP reports bus description `FiiO K3`, USB VID `2972`, PID `0047`, `REV_0221`, and signed driver version `4.47.0.44597`. This bus description establishes more than the WASAPI label `DAC (FiiO Q series)`. The descriptor does not establish the retail model or physical switch position. The owner confirmations and actual application checks follow below.


Windows MSVC formatting, Clippy with `-D warnings`, all 280 workspace tests, the default ASIO desktop build and the explicit `--no-default-features` desktop check passed. One existing Credential Manager integration test remains ignored. The separate Linux domain test suite also passed. The mechanical UI detector reported no findings. These results do not replace physical hardware checks.

The initial native checkpoint uses source fingerprint `226a4dcb6c32288f993734b1e1123b5b7dc8f32a789e75321dd71eb64c39c7cf` and executable SHA-256 `cc6e35728b4724971e17b1e0427d2d83b8e218e10207bd48b2db229abfbae6c0`. `source-manifest-initial.json` gives the hash algorithm and every Rust/C++/TOML/lockfile hash, and `source-initial/` archives those files. It is an uncommitted patch over the base commit above.

The isolated application uses `LOCALAPPDATA=C:\Users\doubl\AppData\Local\Temp\Parabolica-Issue9-asio-playback\appdata` and synthetic PCM fixtures in that same disposable namespace. Native interaction uses UI Automation and native file dialogs. Screenshots resolve the Windows Pictures known folder, `F:\Libraries\Pictures`, and are saved under its `Screenshots` directory with unique `parabolica-issue9-*` names. No real library, settings, driver installation or Windows mix/session-volume setting is changed.

The real player opened the installed FiiO ASIO driver for 24-bit/96 kHz stereo WAV. It negotiated 96 kHz, `ASIOSTInt32LSB`, outputs 1/2, and a driver-preferred 128-frame buffer. The displayed duration was 1.33 ms; `getLatencies` separately reported 2.50 ms output latency. Resampling was Off, Windows mixer Bypassed and Windows session volume Not in this path. The initial stream sample and subsequent samples reported zero ring underruns. This is no statement about issue #48 or long-session reliability. The owner confirmed hearing continuous audio from this running application and executable.

A Windows session-manager query for this exact process found no WASAPI audio session on the default FiiO endpoint. The app's backend, driver and processing labels came from its actual native route. The Dark theme check retained the stored output choice and active ASIO playback. One earlier UI automation pass selected a 512-frame buffer and paused; the paced repeat retained the intended driver-preferred choice. An attempted native format matrix stopped at a file-dialog automation failure and is not counted as a successful matrix.


The owner physically unplugged and reconnected the K3 during ASIO playback. The native driver delivered a reset notification. After reconnection, the app still showed the reset failure and No open stream, with the same selected driver and queue position. `physical-reconnected-before-retry-*` records this state. Explicit Retry reopened the same CLSID at 96 kHz, PCM32, 128 frames and 2.50 ms reported output latency; the stream sample reported zero underruns. `physical-reconnected-after-retry-*` and the corresponding Pictures screenshot record recovery. No WASAPI backend or other endpoint was selected.


The owner identified the attached hardware as **FiiO K3s**, with its physical switch set to **USB 2.0**. Windows' bus descriptor still reports `FiiO K3` and `REV_0221`; those machine strings are preserved rather than treated as a retail-model identification.

The owner put Real to sleep during ASIO playback and woke it without pressing Play/Retry. Windows Power-Troubleshooter records sleep at `2026-11-08T08:55:31.1694063Z` and wake at `2026-11-08T08:55:49.7999080Z`, an 18.6305017-second interval. The app received a driver reset, closed the stream and remained blocked at the retained position with the same ASIO selection. Explicit Retry reopened 96 kHz PCM32, 128 frames and 2.50 ms reported latency. The captured retry sample reported zero underruns. `power-resume.json` and `physical-sleep-awake-{before,after}-retry-*` preserve the OS event and native app state. These are actual Windows sleep and owner observations, separate from the simulated callback-gap fixture.


The successful native cases are recorded by `native-44100-*`, `native-48000-*`, `native-96000-*`, `native-fixed48-source96-*`, `native-buffer256-*`, `native-map21-*`, `native-unsupported-rate-*`, and `native-unsupported-fixed-recovery-*`. The native host accepted the reversed mapping 2/1 and showed it in Current stream. Physical left/right channel assignment was not independently captured. A 12345 Hz Follow source WAV failed with its requested rate and selected ASIO backend intact. An explicit fixed 48000 Hz choice recovered through ASIO and showed resampling Active. The 96 kHz FLAC case showed resampling Off.

| Source / policy | Opened rate | Buffer frames | Computed duration | Driver-reported output latency |
| --- | --- | --- | --- | --- |
| PCM16 WAV 44100, Follow source | 44100 Hz | 64 | 1.45 ms | 2.49 ms |
| PCM24 WAV 48000, Follow source | 48000 Hz | 64 | 1.33 ms | 2.50 ms |
| PCM24 WAV/FLAC 96000, Follow source | 96000 Hz | 128 | 1.33 ms | 2.50 ms |
| PCM24 FLAC 96000, Fixed 48000 | 48000 Hz | 64 | 1.33 ms | 2.50 ms |
| Same source/policy, explicit 256-frame request | 48000 Hz | 256 | 5.33 ms | 8.50 ms |

Every row used PCM32 ASIO output on this actual driver. The recorded per-open samples reported zero ring underruns. A requested buffer size also became this driver's new preferred size; a subsequent preferred-buffer open at 96 kHz used 512 frames and reported 8.50 ms. Driver preferred is therefore queried each time, rather than treated as a constant. The initial preference is restored before cleanup below.

Open driver panel released the stream and opened the actual `FiiO Control Panel`, `FiiOCplApp.exe`, without an app error. Its UI Automation tree reported ASIO not active, preferred 512 samples, current rate 96000 Hz and output latency 816 samples / 8.50 ms, matching the app's driver-reported value at that configuration. `fiio-control-panel-uia.json`, `panel-processes-after.json` and `native-driver-panel-command-*` preserve these observations. No control-panel settings were changed.

The native format/mapping checks needed controls scrolled into the viewport. Earlier automation attempts outside it and an early file-dialog automation failure remain archived; they are not counted as passes. The completed cases used UI Automation plus a native wheel message addressed to the test window using its UIA bounds. The initial native format/mapping cases used one unchanged source/executable checkpoint; the final UI error fix and its separate native cases are recorded below.


## Final UI error fix and checkpoint

A seeded nonexistent ASIO CLSID exposed a native accessibility crash in the initial executable. Playback failure and capability failure both said `no audio output device is available`, and the shared warning helper derived both node IDs from that text. GPUI's debug accessibility builder rejected the duplicate. The final change gives each error origin a stable ID and adds a headless regression that finds both errors independently. `missing-driver-red-panic.log` preserves the native failure.

The final source fingerprint is `b42b4be7e1b73fbf61936c2f781dd0bfa69f048c63bdf835f9c0ab043d5139cc`; final executable SHA-256 is `2facfeac6a28a937778b659db512275e9c23e5924b8a845fae15df22c404918c`. `source-manifest-final.json` and `source-final/` archive this checkpoint. Only the Listening device error IDs/test support and its regression test changed after the initial native checkpoint. The audio, platform bridge, driver lifecycle and callback code are identical across those checkpoints. The owner's listening, physical USB and actual sleep observations above belong to the initial executable and are not relabelled as tests of the final one.

Final Windows formatting, Clippy with `-D warnings`, all **281 tests**, the ASIO desktop build and the WASAPI-only desktop check passed. One existing Credential Manager integration test remains ignored. Logs are `final-{clippy,tests,build,feature-off}.log`; `final-checks.ps1` exits at the first failed command, including formatting.

On the final executable, the app started with a deliberately nonexistent driver CLSID in its disposable preferences. Both errors remained in the real UIA tree without a crash; no stream opened. Retry preserved that missing selection and did not choose WASAPI or an installed driver. Explicitly selecting the actual x64 FiiO ASIO driver then played through ASIO at 96 kHz PCM32, with a 512-frame driver-preferred buffer, 5.33 ms buffer duration, 8.50 ms reported latency and zero underruns in the captured sample. `final-native-missing-driver-*` and `final-native-missing-driver-recovered-*` record this actual app behavior. The earlier normal restart retained the selected ASIO CLSID, rate policy, mapping, preferred-buffer policy and queue position, with transport paused. Startup may prepare a silent paused stream for decoder prefetch; its position did not advance.

## Limits

Busy-driver recovery, synthetic concurrent/invalid callbacks, the callback gap without a driver reset, and non-FiiO sample representations were exercised through fixtures. An external hardware busy condition was not forced. Actual K3s unplug/replug and Windows sleep delivered driver reset notifications, so those observations establish that reset/retry path. Individual capability queries above 96 kHz do not prove native playback at those rates. There was no independent ASIO waveform capture, physical channel-isolation capture, measured round-trip latency, Narrator speech test or additional display-scaling test. The owner's continuous-audio observation is the audible evidence. No driver/hardware DSP transparency or bit-perfect result is claimed. The recorded implementation/hardware checks preceded PR publication; remote CI results are tracked separately on the PR.


## Archive and cleanup

The final native restoration case opened 44100 Hz PCM32 with 64 frames, 1.45 ms computed duration and 2.49 ms reported output latency, then selected Driver preferred and closed the app. A fresh native driver inspection after exit confirmed the initial current rate of 44100 Hz and preferred 64-frame buffer, with limits 8–2048 and power-of-two granularity. This is recorded in `restore-driver.log`, `final-native-original-rate-buffer-*`, `final-native-driver-preference-restored-*` and `driver-probe-restored.log`. The test-created control-panel process was closed and its absence confirmed.

All 14 synthetic fixture files retained their recorded hashes and are archived in `fixtures-final/`. `verification-portable-final.sqlite3` is a standalone backup in DELETE journal mode; Windows and an independent Linux read both returned `PRAGMA quick_check = ok`. Its SHA-256 is `bdc21452dec5259d2bc618acffb9cb6c6310dd63412dd3126ed1f2a6c59f9787`. Earlier failed archive attempts remain in the evidence directory and are not the final backup.

`cleanup-result.json` and `cleanup-os-confirmation.txt` confirm that the complete disposable Windows namespace was removed, no test app or owned control-panel process remains, and `cmdkey /list:Parabolica-Issue9*` lists no credentials. No credentials were created by these ASIO checks. The dedicated Cargo target directories, pinned SDK cache, evidence archive and Pictures screenshots remain. Both earlier #8 archives were preserved.

The decision log was audited against the available command artifacts, source manifests, runtime captures and owner confirmations. No transcript export was available for a separate transcript audit. The implementation and hardware-verification phase made no Git state changes or external writes. Publication and babysitting were subsequently authorized, with issue closure conditional on the acceptance evidence and live review/CI result. Issue #9 remains open during that evaluation; merge is not authorized.
