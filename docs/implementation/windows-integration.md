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

## Not verified on Windows

Everything below needs a real Windows build and has not been run:

1. Media keys reach the player with the window focused, unfocused and minimized; the overlay's buttons do the same.
2. The overlay shows title, artist, album and artwork of the playing entry, changes with Next/Previous and automatic advance without showing the previous entry's artwork, clears when the queue is cleared and after quit, and shows "Parabolica" as the app name.
3. With "Show title, artist and artwork" off, the overlay and toasts show no track details while the buttons still work.
4. Open with from Explorer (one file, several files, Unicode names, paths over 260 characters) while the app is closed and while it is running; a second launch without files brings the existing window forward, also when minimized.
5. A background playback failure flashes the taskbar and shows one toast; clicking it brings the window forward.
6. Sleep while playing: playback is paused before suspend, nothing plays after resume, the same endpoint stays selected, and Play resumes from the saved position. Repeat with the output device unplugged during sleep.
7. Closing the window and signing out or shutting down Windows while playing both restore the same entry and position on the next launch.
8. Native Open file, Add folder and playlist import/export dialogs with Unicode and long paths, and Cancel leaving the session unchanged.
9. Narrator through UI Automation reads Settings › Playback's heading, the checkbox's name and state, and the save error.
