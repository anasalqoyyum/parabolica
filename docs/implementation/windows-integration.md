# Windows integration evidence

Issue [#23](https://github.com/anasalqoyyum/parabolica/issues/23) builds on master `0d28ed5`. Its blockers #7 (queue/session, PR #46) and #14 (provider contracts and credentials) are merged.

## App entry points and behavior

Run `cargo run -p parabolica-desktop --locked` on Windows.

- **Media keys and the media overlay.** The main window gets a System Media Transport Controls session (`platform::windows::media::MediaSession`). Play, Pause, Stop, Next and Previous from the keyboard's media keys or the overlay call the same `QueueService` methods as the player's buttons (Stop pauses). Windows routes these keys to the session while the window is unfocused.
- **What the overlay shows.** Title, artist, album and a 256 px artwork thumbnail of the queue's current entry. Decoder tags are used only while the engine reports that same entry; otherwise the queue's title shows with empty artist and album. Provider entries show their title and no artwork. Every update clears the previous display first, so an earlier entry's text or artwork never remains. Artwork loads on its own thread, so text, status, buttons and the privacy setting apply without waiting for a slow file. Artwork that arrives after the entry changed or details were hidden is dropped.
- **Privacy.** Settings › Playback › System media controls has "Show title, artist and artwork" (preference `system_media.track_details`, on by default). Off keeps the playback buttons and status and shows no track details in the overlay or notifications. An unreadable stored value counts as off.
- **Open with and a second launch.** The first process claims the pipe `\\.\pipe\Parabolica-<user SID>`. A later launch sends its command-line files to it and exits before opening the database. The running player adds them with Open file's command (play now) and brings its window forward; a launch with no files only brings the window forward. The pipe's DACL admits only the current user, remote clients are rejected, the client checks that the server process runs as the same user, and it allows only identification, so the server cannot act as the client. Requests are capped at 1 MiB and 4,096 absolute Unicode paths of up to 32 KiB each, without line breaks, NUL or `\\.\` device paths; a request with any invalid path opens nothing. If the pipe cannot be used, the app starts on its own and the database lock reports the second instance as before.
- **Notifications.** A playback failure while the window is in the background flashes the taskbar button and posts one "Playback stopped" toast that replaces the previous one. With track details hidden the toast says "Open Parabolica for details." Clicking it brings the window forward. The app registers `HKCU\Software\Classes\AppUserModelId\Parabolica.Desktop` with the display name Parabolica so the overlay and toasts name the app without a Start menu shortcut.
- **Sleep and resume.** On suspend the app pauses playback and waits up to one second for the paused checkpoint. On resume it refreshes the device list. It does not resume playback and does not change the selected endpoint, so nothing plays until the user presses Play on the saved device.
- **Close, quit and session end.** Closing the window quits. The quit handler joins the queue worker's final checkpoint before returning, because GPUI waits only 200 ms for quit futures and exits the process right after `WM_ENDSESSION`. It then clears the media overlay and releases the pipe. Releasing the pipe cancels any read or write blocked on a stalled client.
- **Dialogs.** Open file, Add folder, folder relocation, Locate file and playlist import/export use GPUI's native Windows dialogs (`IFileOpenDialog`/`IFileSaveDialog`), which return full paths without a `MAX_PATH` limit. Cancelling returns no path and changes nothing.

No log output was added. A toast body holds the same file name and error text the player shows.

## Portable verification

Development environment: Linux x86_64, Rust 1.99.0, GPUI Kit 0.9.0. There is no Linux media-controls backend (#34); the Linux build compiles the Windows code paths out.

- Desktop tests in `apps/desktop/src/controllers/os_events/tests.rs` cover the open-request format (Unicode and 320-character paths, order, empty requests, damaged/relative/NUL/line-break/device paths, path count limit), command-line resolution, presentation rules (no selection, stale engine tags while the next entry loads, hidden details, provider entries), display changes while an artwork read is blocked, late artwork after the entry changed or details were hidden, media actions against a real queue worker with a clock output, sleep pause with the saved position and no resume on wake, the preference, and failure notifications.
- Headless GPUI tests open the real shell with a temporary database: a forwarded request with a long Unicode path plays through the running player and quit saves the session and stops the worker; the Open file dialog keeps a long Unicode path, and cancelling it leaves the queue unchanged.
- `crates/platform/src/windows/lifecycle/tests.rs` runs only on Windows (CI): forwarding through a disposable per-test pipe, the 1 MiB limit, rejected requests, stopping with a stalled client connected, releasing the name and the SID-scoped pipe name.
- `cargo check`/`clippy` of `parabolica-platform` for `x86_64-pc-windows-msvc` passed on Linux. The desktop's Windows glue cannot be checked there (bundled SQLite needs the MSVC toolchain), so a scratch crate type-checked the same `MediaSession::for_window(&gpui::Window, ..)`, `MediaState` and pipe calls against GPUI for that target. Windows CI builds, lints and tests the desktop.

All 308 workspace tests passed on Linux, and formatting, workspace Clippy with warnings denied and the desktop build passed. Commands run on Linux from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
cargo clippy -p parabolica-platform --all-targets --target x86_64-pc-windows-msvc --locked -- -D warnings
python3 scripts/render_issue_plan.py
```

The headless quit test passes even without the synchronous quit handler, because the test executor runs the player's own background shutdown to completion. It confirms quit ordering, not the 200 ms cutoff on a real Windows build.

## Real Windows evidence

Machine: a desktop PC running Windows 11 Pro 26H2 (build 10.0.26300.9711, x64), Rust 1.99.0, output to a USB DAC in shared mode at 44.1 kHz. Source: branch `feat/issue-23-windows-integration` at `d547496` (#23 merged with master `d06b9c8`).

### Check commands

Run from the repository root in Git Bash with `PARABOLICA_ASIO_SDK_DIR` from `scripts/setup-asio.ps1` and a `CARGO_TARGET_DIR` used only for this check:

```sh
cargo fmt --all --check                                         # passed
cargo clippy --workspace --all-targets --locked -- -D warnings  # passed
cargo test --workspace --locked                                 # see below
cargo build -p parabolica-desktop --locked                      # passed
```

`cargo test --workspace --locked --no-fail-fast` passed 343 tests, including the six `windows::lifecycle` pipe tests and the `os_events` tests. The first run without `--no-fail-fast` stopped on `credentials::tests::a_credential_persists_across_processes_until_removed`: after the parent deleted the credential, the child process still read it. Rerunning the platform test binary showed the same failure in 3 of 30 runs filtered to `credentials::` and 5 of 15 full runs. #23 does not touch `credentials`, and the filtered runs exclude the lifecycle tests, so this flake belongs to #14's test and is not fixed here. Credential Manager held no `Parabolica*` entries afterwards (`cmdkey /list`).

### The real app

The app ran from `target\debug\parabolica.exe` with `LOCALAPPDATA` pointing at a disposable folder. Test files were synthetic FLAC tones with distinct tags and solid-colour cover art: red, green, blue, and one file without art. Their names used Unicode such as `05 Ünïcødé 日本語 ✓.flac`, and one sat at a 335-character path under `Löng pâth 長い ✓\`. A throwaway .NET program outside the repository did the driving. It used UI Automation for the app, `SendInput` for media keys and `GlobalSystemMediaTransportControlsSessionManager` to read the session, sampling the thumbnail's centre pixel to tell the artwork apart. It also read `ToastNotificationManager.History`, watched a shell hook window for `HSHELL_FLASH`, and sent Win32 messages to the file dialogs' Open, Save and Cancel buttons. These results are separate from the headless tests above.

1. **Media keys and overlay buttons.** Passed. `VK_MEDIA_PLAY_PAUSE`, `VK_MEDIA_NEXT_TRACK` and `VK_MEDIA_PREV_TRACK` paused, resumed and changed the entry with the window focused, behind Windows Terminal and minimized. The player's UIA labels and the session status changed to match. Two other apps' paused sessions stayed paused. `TryTogglePlayPauseAsync`, `TryPlayAsync`, `TryPauseAsync`, `TrySkipNextAsync` and `TrySkipPreviousAsync` reached the player.
2. **Overlay content.** Passed, except the flyout's app name, which was not observed. The session reported `SourceAppUserModelId` `Parabolica.Desktop`, the playing entry's title, artist and album, a 256×256 thumbnail and Playing/Paused status. Next, Previous and automatic advance (blue → red → green) replaced the text and the thumbnail together. Moving to the file without art showed no thumbnail. At the end of the queue the session showed Stopped with no text. After Clear queue and after the app quit, the session list no longer contained Parabolica. `HKCU\Software\Classes\AppUserModelId\Parabolica.Desktop` held `DisplayName` `Parabolica`, and Notification Center grouped the toast under "Parabolica". The media flyout in Quick Settings was not reachable through UIA on this build, so its app label was not read.
3. **Privacy.** Passed. With "Show title, artist and artwork" off, the session had empty title, artist and album and no thumbnail while toggle, next, previous and the next-track key still worked. The background-failure toast read "Playback stopped | Open Parabolica for details." The setting stayed off after a restart and the session stayed empty. Turning it back on restored the details and artwork of the paused entry.
4. **Open with and a second launch.** Passed for command-line launches. Explorer's Open with menu was not used. A cold launch with one file played it, and a cold launch with three files played them in order: Charlie, the Unicode file, then the 335-character path. With the app running, launches with one file and with three files (once while its window was minimized) added them to the running player, restored and focused its window and exited with code 0. Only one `parabolica.exe` remained, and no database-in-use error appeared. A launch without files brought the window forward from minimized and from behind another window.
5. **Failure notifications.** Passed. Two files were queued, the second was deleted and Windows Terminal was put in front. When playback reached the deleted file, the shell hook received `HSHELL_FLASH` for the Parabolica window and toast history held one "Playback stopped" toast. A second failure replaced it rather than adding one. With details on, the body was "Can't play 08 Doomed.flac: can't read the file: …". The toast banner itself was not caught. Invoking the toast in Notification Center restored the minimized window and focused it.
6. **Sleep and resume.** Not run. The machine was not suspended, and GPUI receives these events through `RegisterSuspendResumeNotification`, which a test cannot trigger without a real suspend.
7. **Close and restore.** Passed for closing the window. Closing through UIA's WindowPattern while playing at 0:08 ended the process within about 100 ms. The next launch showed the same entry paused at 0:08. Sign-out and shutdown were not run.
8. **Native dialogs.** Passed. Open file played a Unicode file and the 335-character file. Add music folder indexed a 309-character Unicode folder ("Album Long · Ärtist Long"). Import M3U8 read a playlist at a 328-character Unicode path with all three entries, including the long path. Export M3U8 wrote a 332-character Unicode path that read back with the same three entries. Cancel in each dialog left the queue, library, playlists and session unchanged, and no exported file appeared.
9. **UIA exposure.** Passed for the heading, checkbox name and state. Settings › Playback exposes Text "Playback", Text "System media controls" and CheckBox "Show title, artist and artwork" with ToggleState On or Off matching the setting. Narrator itself was not run, heading levels were not read, and the save error was not triggered.

Afterwards no `parabolica.exe` was running, Parabolica's toast history was cleared, and both `HKCU\Software\Classes\AppUserModelId\Parabolica.Desktop`, which did not exist before the run, and the `Notifications\Settings\Parabolica.Desktop` key that Windows created for the toasts were removed.

## Not verified on Windows

1. Physical media keys. The checks above used `SendInput`.
2. The app name shown in the media flyout itself.
3. Explorer's Open with menu, as opposed to the same command line.
4. The toast banner appearing on screen.
5. Sleep while playing: playback is paused before suspend, nothing plays after resume, the same endpoint stays selected, and Play resumes from the saved position. Repeat with the output device unplugged during sleep.
6. Signing out or shutting down Windows while playing restores the same entry and position on the next launch.
7. Narrator speech, heading levels and the Settings save error.
