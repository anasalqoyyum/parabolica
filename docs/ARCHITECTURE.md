# Architecture and ownership

This is the workspace layout, the contracts in `parabolica-domain`, and the rules feature issues follow when they add code. [The technical specification](TECH_SPEC.md) defines behavior; this file defines where it lives and how modules talk.

## Toolchain and pins

| Item | Pin | Where |
| --- | --- | --- |
| Rust | 1.99.0, MSVC target on Windows (`x86_64-pc-windows-msvc`) | `rust-toolchain.toml` |
| GPUI Kit | `gpui-kit =0.9.0` (Apache-2.0) | `Cargo.toml` `[workspace.dependencies]` |
| GPUI snapshot | `gpui-pre =0.3.11`, required exactly by Kit 0.9.0 | `Cargo.lock` |
| Everything else | Exact versions | `Cargo.lock` |

Kit 0.9.0 requires Rust 1.92 or later. The app depends on `gpui-kit` only and reaches GPUI through `gpui_kit::*`. Do not add `gpui-pre` or another GPUI crate directly; upgrade Kit and its snapshot together.

Windows builds need Visual Studio 2022 Build Tools (or Community) with the Desktop development with C++ workload and a Windows SDK, plus CMake on `PATH`. Kit's [installation guide](https://gpui-kit.com/docs/installation/) lists the Linux and macOS packages.

## Commands

Run from the repository root. `--locked` fails if `Cargo.lock` would change.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build -p parabolica-desktop --locked
cargo run -p parabolica-desktop --locked
```

The app binary is `parabolica` (`parabolica.exe` on Windows). `.github/workflows/check.yml` runs these commands on a Windows Server 2025 MSVC runner, checks that `Cargo.lock` is current, and fails if `parabolica-domain` gains a GPUI, Windows, database, audio or network dependency.

## Crates

| Crate | Path | Depends on | Purpose |
| --- | --- | --- | --- |
| `parabolica-desktop` | `apps/desktop` | all crates, `gpui-kit` | GPUI Kit app: views, UI commands, controllers |
| `parabolica-domain` | `crates/domain` | `uuid`, `thiserror` | IDs and cross-module contracts |
| `parabolica-storage` | `crates/storage` | domain | SQLite, migrations, the write owner, backup |
| `parabolica-library` | `crates/library` | domain, storage | Indexing, collection queries, CUE indexing, relocation |
| `parabolica-audio` | `crates/audio` | domain | Decode, engine, DSP, output backends, callback contract |
| `parabolica-replaygain` | `crates/replaygain` | domain | Loudness analysis and `REPLAYGAIN_*` mapping |
| `parabolica-tags` | `crates/tags` | domain | Tag/artwork reads and safe album writes |
| `parabolica-cue` | `crates/cue` | domain | CUE parsing |
| `parabolica-platform` | `crates/platform` | domain, provider contracts | Devices, ASIO enumeration, credentials, media controls, lifecycle |
| `parabolica-provider-contracts` | `crates/providers/contracts` | domain | Provider adapter traits and credential contracts |
| `parabolica-youtube` | `crates/providers/youtube` | domain, provider contracts | Experimental YouTube Music adapter |
| `parabolica-discogs` | `crates/providers/discogs` | domain, provider contracts | Read-only Discogs adapter |
| `parabolica-scrobbling` | `crates/providers/scrobbling` | domain, provider contracts | Opt-in scrobbling |

Only `parabolica-desktop` may depend on `gpui-kit`. Feature crates do not depend on each other unless the dependency is added in a coordinated change; they meet in the desktop controllers through domain types.

## Registration points

Every crate, module and view listed in the implementation plan's ownership boundaries already exists and is registered: the root `Cargo.toml` lists every member, `apps/desktop/Cargo.toml` depends on every crate, and each parent `mod.rs`/`lib.rs` declares its children. A feature issue fills in its own module files and does not edit a root manifest or a parent module owned by another issue.

What a feature issue still edits:

- **Its crate's `Cargo.toml`** for third-party dependencies it needs. Declare them in that crate with exact versions. Add a dependency to `[workspace.dependencies]` only when two crates must share one version, as a coordinated change.
- **`Cargo.lock`**, which changes with any new dependency. On a merge conflict, take the target branch's lockfile and rerun `cargo metadata` (or any build) so Cargo re-adds your crates; never hand-edit it.
- **New files under its own module directory.** Submodules of an owned module are the owner's business.

Platform-specific modules are `cfg`-gated in their parent (`windows`, `macos`, `linux`), so a module for another OS does not compile on Windows.

## Path ownership

| Path | Owner |
| --- | --- |
| `Cargo.toml`, `rust-toolchain.toml`, `apps/desktop/Cargo.toml`, `.github/workflows/check.yml` | #2, then coordinated changes |
| `crates/domain/**` except `queue/**` | #2, then coordinated changes |
| `crates/domain/src/queue/**` | #7 |
| `apps/desktop/src/main.rs`, `apps/desktop/src/ui/{shell,theme,appearance}/**`, `crates/platform/src/contrast.rs` | #3 |
| `crates/storage/**` | #4 (each issue below owns its `migrations/NNNN_*.sql` files and its `src/<area>` repository module; see [Extending storage](#extending-storage)) |
| `crates/audio/src/{decode,engine}/**`, `crates/audio/src/output/wasapi_shared.rs`, `apps/desktop/src/ui/player/**`, `apps/desktop/src/commands/playback.rs` | #5 |
| `crates/audio/src/realtime.rs` | #2, then coordinated changes with #5 |
| `crates/library/**`, `apps/desktop/src/ui/library/**`, `apps/desktop/src/ui/sources/folders.rs` | #6 |
| `apps/desktop/src/controllers/queue/**`, `apps/desktop/src/ui/{queue,playlists}/**`, `crates/audio/src/transitions/**` | #7 |
| `crates/audio/src/output/wasapi_exclusive.rs`, `apps/desktop/src/ui/listening_device/**`, `crates/platform/src/windows/audio_devices.rs` | #8 |
| `crates/audio/src/output/asio/**`, `crates/platform/src/windows/asio/**`, `apps/desktop/src/ui/listening_device/asio.rs`, `.github/workflows/windows-asio.yml` | #9 |
| `crates/replaygain/src/analyze/**`, `apps/desktop/src/ui/replaygain/scanner/**` | #10 |
| `crates/audio/src/dsp/replaygain/**`, `crates/replaygain/src/tag_mapping.rs`, `apps/desktop/src/ui/replaygain/playback/**` | #11 |
| `crates/tags/**`, `apps/desktop/src/ui/tags/**` | #12 |
| `crates/cue/**`, `crates/library/src/cue_adapter.rs`, `crates/audio/src/cue_ranges.rs`, `apps/desktop/src/ui/cue/**` | #13 |
| `crates/providers/contracts/**`, `crates/platform/src/credentials/**`, `apps/desktop/src/ui/accounts/**` | #14 |
| `crates/providers/youtube/src/{auth,transport}/**` | #15 |
| `crates/providers/youtube/src/catalog/**`, `apps/desktop/src/ui/library/remote/**` | #16 |
| `crates/providers/youtube/src/playback/**`, `apps/desktop/src/controllers/provider_playback/**` | #17 |
| `crates/providers/youtube/src/actions/**`, `apps/desktop/src/ui/discovery/**`, `apps/desktop/src/ui/playlists/remote/**` | #18 |
| `crates/providers/discogs/**` | #20 |
| `crates/tags/src/discogs_import/**`, `apps/desktop/src/ui/tags/discogs/**` | #21 |
| `crates/audio/src/inspection/**`, `apps/desktop/src/ui/signal_path/**` | #22 |
| `crates/platform/src/windows/{media,lifecycle}/**`, `apps/desktop/src/controllers/os_events/**` | #23 |
| `crates/library/src/collections/**`, `apps/desktop/src/ui/collections/**` | #27 |
| `apps/desktop/src/{ui,controllers}/sleep_timer/**` | #28 |
| `crates/library/src/lyrics/**`, `crates/providers/youtube/src/lyrics.rs`, `apps/desktop/src/ui/lyrics/**` | #29 |
| `crates/providers/scrobbling/**`, `apps/desktop/src/ui/accounts/scrobbling.rs` | #30 |
| `crates/storage/src/backup/**`, `crates/library/src/relocation/**`, `apps/desktop/src/ui/data_transfer/**` | #31 |
| `crates/platform/src/macos/**`, `crates/audio/src/output/coreaudio/**` | #33 |
| `crates/platform/src/linux/**`, `crates/audio/src/output/linux/**` | #34 |

Where two rows overlap, the more specific path wins (for example #13 owns `crates/library/src/cue_adapter.rs` inside #6's crate). Paths outside the workspace (`docs/research/**`, `tests/**`, `packaging/**`) keep the ownership listed in [plan.json](implementation/plan.json).

## Database ownership

- `parabolica-storage` owns the database file, its schema and every SQLite connection. The app reaches it only through `Storage`, whose methods are typed repository operations; no other crate depends on `rusqlite` or runs SQL.
- One writer thread holds the only read-write connection. Each write method sends one closure to it over a bounded channel (64 pending writes) and waits for the result. The closure runs as one `BEGIN IMMEDIATE` transaction: an error or a panic rolls back everything it did, and the writer keeps serving.
- Reads use a small pool of read-only connections in WAL mode. Each read method runs in one read transaction, so it sees a consistent snapshot and a long write does not block library browsing.
- `Storage` methods block. Controllers and workers call them; the UI thread never does. The one exception is startup in `main.rs`, which opens the database and reads the saved appearance before the event loop and any window exist. `Storage` is `Clone`; clones share the writer, the readers and the lock.
- A `<database>.lock` file holds an OS lock for the life of the handle, so a second app instance gets `OpenError::InUse`.
- Nothing reachable from the audio callback touches storage. The queue worker checkpoints the full session transaction on commands, entry boundaries, pause, seek, clean exit and a 500 ms timer. `save_playback_position` remains available for callers that need a guarded position-only update.
- The database holds no credentials. Preference keys that name a credential are rejected, and a test fails if any migration adds a table or column named for one. Storage errors name tables, columns and identifiers, never stored values.

### Opening and recovery

`Storage::open(path)` creates the file when absent, runs `PRAGMA quick_check` on an existing one, checks the `application_id` and the migration ledger, then applies pending migrations. It never overwrites or deletes an existing file. Failures are `OpenError` variants, each with `recovery_options()` for the UI:

| Error | Cause | Options |
| --- | --- | --- |
| `InUse` | Another instance holds the lock | Retry |
| `Corrupt` | Not an SQLite file, or `quick_check` failed | Export a copy, move aside and start fresh |
| `Incompatible` | Foreign database, a migration this build does not know (a newer app), or an applied migration whose checksum changed | Export a copy, move aside and start fresh |
| `MigrationFailed` | A pending migration failed; the database is unchanged and the pre-migration backup is named | Export a copy, move aside and start fresh |

`recovery::export_copy` copies the database and its WAL/journal without overwriting anything at the destination. `recovery::move_aside` renames the files to `<name>.unusable-<unix ms>` so the next open starts empty. A write that later hits corruption returns `StorageError::Corrupt` (`is_corrupt()`); the app drops its handles and offers the same options. Backup/export of a healthy database belongs to #31 (`crates/storage/src/backup/**`).

### Migrations

Migrations live in `crates/storage/migrations/` as `NNNN_MM_slug.sql`. `NNNN` is the owning issue number, zero-padded (`0004`, `0006`, ...); `MM` orders one issue's migrations; the slug is lower-case letters, digits and `_`. Each issue owns its `NNNN_*` files only. `build.rs` embeds every file in the directory, sorted by name, and fails the build for a misnamed one, so adding a migration needs no list edit.

Issues merge in dependency order, which is not always numeric order. The runner records each applied migration by file name with a checksum (line endings ignored) and applies every unapplied one in name order. All pending migrations run in one transaction with foreign keys off, followed by `PRAGMA foreign_key_check`; any failure leaves the database as it was. An existing database is first copied with `VACUUM INTO` to `<name>.backup-<unix ms>-before-<first pending migration>`.

Rules for migration files:

- Use `STRICT` tables, 16-byte `BLOB` IDs (`CHECK (length(id) = 16)`) and integer Unix milliseconds for times.
- Reference only tables created by your own issue, by #4, or by issues that block yours.
- Contain no `BEGIN`, `COMMIT` or `PRAGMA foreign_keys`; the runner owns the transaction.
- A merged migration is never edited; the checksum makes the next open fail with `Incompatible`. Fix it with a new migration.
- Rebuild a table (create new, copy, drop, rename) instead of editing a column's constraints.

### Core schema (`0004_01_core.sql`)

| Tables | Holds |
| --- | --- |
| `library_roots` | Folder roots with path, volume identity and availability. An offline root keeps its files. |
| `media_files` | Physical file identity: root, `/`-separated relative path, OS file ID, signature, presence. `seq` is the stable search key. |
| `media_file_tags` | Display tags as read from the file, multi-value fields in order. Kept apart from identity. |
| `media_file_search` | FTS5 trigram index over title, artist, album, album artist, genre/style/composer/performer/label/catalog number and file name, plus the same text lower-cased without Latin accents for one- and two-character words. Rewritten with the tags and on every move. |
| `virtual_tracks` | CUE frame ranges inside an image file. Track numbers are unique per image, checked by `replace_virtual_tracks` so retained tracks can be renumbered. |
| `provider_items`, `tracks`, `track_sources` | Provider item identity, logical recordings, and confirmed/suggested/rejected links from sources to recordings. A source links to at most one recording. |
| `preferences` | Typed preferences (`preferences::Preference`), saved in batches. |
| `queue_entries`, `queue_state` | The queue checkpoint: order, stable entry IDs, sources, current entry, position, repeat, shuffle seed and order. |
| `jobs`, `job_items` | Job state checked with `JobState::transition` inside the transaction, progress, failure reason, and per-item outcomes with the file signature they were recorded against. |

### Extending storage

A feature issue that owns an `NNNN` migration adds:

1. Its migration files.
2. Its repository as a new module, `crates/storage/src/<area>.rs` (or a directory), owned by that issue, containing `impl Storage { ... }` methods built on the crate-internal `write`/`read` closures. Register it with one `pub mod <area>;` line in `crates/storage/src/lib.rs`; that line is the only shared edit, and concurrent issues rebase it.
3. Tests that open a temporary database through `Storage::open`.

Changing a #4 repository method or the core schema is a coordinated change with #4's consumers.

## Domain contracts

`parabolica-domain` imports no UI, device, database or network crate. The rules below are encoded in its types and tested in `crates/domain/tests/`.

**Identities** (`ids`). Every persisted entity has an app-generated UUIDv7 wrapped in its own type: `LibraryRootId`, `MediaFileId` (physical file), `VirtualTrackId` (CUE track), `TrackId`, `ReleaseId`, `ArtistId`, `ArtworkId`, `PlaylistId`, `PlaylistEntryId`, `QueueEntryId`, `JobId`. Storage keeps the 16 raw bytes; logs and exports use the prefixed text form (`rel_0190...`), which only parses back as the same type. IDs never derive from paths or titles. Provider items are `ProviderItemRef` (provider plus the provider's own ID).

**Files** (`files`). A physical file has a `FileLocation` relative to its root, an optional OS file ID and a `FileSignature` (size, mtime, optional content digest). Stale analysis and tag-write rejection compare signatures with `matches_recorded`.

**Sources, PCM and output** (`source`, `format`, `output`). These are three separate layers:

1. A queue or playlist entry refers to a `PlayableSource`: a local file, a CUE track or a provider item.
2. Resolution produces a `ResolvedSource`: a local file plus `FrameSpan` (a CUE track resolves to its image and exact frame range), provider media the native engine decodes (`ProviderPcm`), or provider-managed playback such as a browser session (`ProviderManaged`).
3. A `PlaybackRoute` is either `Native` (resolved source, internal `PcmFormat`, `NegotiatedOutput` from the backend) or `ProviderManaged`. A provider-managed source cannot be given a native route.

Internal PCM is interleaved `f64` (`PcmSample`), which holds every integer sample up to 32 bits exactly. `SourceFormat` describes the file, `OutputFormat` is what the backend opened, and `DeliveryFormat` is what a provider reported. `OutputBackend` covers WASAPI Shared, WASAPI Exclusive and ASIO; an `OutputDeviceId` is per backend, so the same DAC under WASAPI and ASIO has two identities. `BufferOwner::Driver` records a driver-owned ASIO buffer. `buffer_duration()` is not latency; `reported_latency` is present only when the backend reported it.

**Signal path** (`signal_path`). `SignalPath::describe` derives the stages from the route. WASAPI Shared keeps the Windows mixer and session volume active but unverified. Exclusive and ASIO mark the mixer bypassed and session volume not in path; ASIO output values come from the driver. Provider-managed routes mark decode and output as managed by the provider and app processing as not in path. `app_changes_samples()` reports gain, volume, DSP, resampling, or a conversion that cannot represent every source sample (any float source to integer output, or an integer source wider than the output). An unknown source format never reports conversion as bypassed; `false` is not a bit-perfect claim because the device stage stays unverified.

**Gain** (`gain`). `ReplayGainMode` defaults to Off. Auto resolves to Album for ordered album playback and Track otherwise. A missing album measurement in album mode is `NotScanned` and plays at 0 dB; it never substitutes track gain. Peaks are linear, gains are finite dB.

**Tags** (`tags`). A `TagPatch` targets one physical file and carries the signature it was reviewed against. `AlbumWrite` rejects two patches for one file (a CUE image is written once) and patches for excluded files. `AlbumWriteStatus` reports Complete, Partial or Failed from per-file outcomes.

**Jobs** (`jobs`). `JobState` transitions are checked: a cancelled, failed or incomplete job cannot become Completed, and only a Completed job can go Stale.

**Providers** (`provider`). `ProviderCapabilities` treats an unreported capability as `NotSupported`. Without `AppOwnedPcm`, the provider owns playback and native ReplayGain, device selection, exclusive mode and ASIO do not apply.

**Queue** (`queue`). Each insertion creates new `QueueEntryId`s, so a duplicated source is two entries. `QueueSession` owns current selection, deterministic SplitMix64 shuffle, repeat and previous navigation. Queue edits use effective shuffled indices; turning shuffle off restores insertion order. Play next inserts immediately after the current entry in both orders. Explicit next/previous ignores repeat-one. Previous follows confirmed starts, with a preceding-order fallback. Removing the selected entry selects its effective successor, or stops at the end, including repeat-all. Moves keep the current identity. Navigation retains the latest 1000 distinct consecutive starts; a separate listening ledger retains repeat boundaries too.

## Provider accounts

`parabolica-provider-contracts` defines how providers sign in, keep credentials and report what they can do. Downstream providers (#15 YouTube Music, #20 Discogs, #30 scrobbling) plug into it without changing domain or queue types.

- **Services** (`service`). `ServiceId` is a catalog `ProviderId` or `Scrobbling`. Its `key()` names the OS credential. `AccountIdentity` holds the service's stable account ID, which a reconnect compares, and a display name.
- **Secrets** (`secret`). Tokens, cookies, session keys and authorization codes travel as `Secret`: `Debug` prints `Secret([redacted])`, there is no `Display`, and memory is zeroed on drop. `redact_url` reduces a URL to scheme and host, because signed media URLs carry signatures in the path as well as the query. `ProviderError` messages name the service and problem, never a value.
- **Credentials** (`credentials`). `CredentialStore` keeps one credential per service in the OS store; the database never holds one. `parabolica_platform::credentials::OsCredentialStore` uses Windows Credential Manager: a generic credential named `Parabolica/<service key>`, persisted for the current user on this computer (`CRED_PERSIST_LOCAL_MACHINE`), with the account ID as user name, the display name as comment and the secret (at most 2,560 bytes) as blob. macOS and Linux report `CredentialError::Unavailable` until #33/#34 add Keychain and Secret Service. `MemoryCredentialStore` (feature `test-support`) is for tests.
- **Capabilities** (`capabilities`). A provider declares each capability as `Access::Public` or `Access::Account` in `ProviderSupport`. `capabilities(connection, reachability)` reports Offline or the restriction for every declared capability when the service is unreachable, SignedOut or SessionExpired for account capabilities, and `NotSupported` for anything undeclared in every state.
- **Requests** (`requests`). `Account::requests()` returns a `RequestScope`. `begin()` cancels the previous request; `accept(Tagged<T>)` drops results from superseded requests and from an ended session. Adapters poll `Request::check()` between network calls. Signing out or reconnecting ends the session, which cancels its requests.
- **Sign-in** (`auth`). A provider implements `AuthFlow::sign_in` (through the user's default browser, never a native password field) and `revoke`. `BrowserAuthorization` creates the `state` and PKCE S256 values and validates the redirect; `LoopbackReceiver` receives it on `127.0.0.1` at an ephemeral port, answers requests with another `state` or path and keeps waiting, and honours cancellation and a timeout. `parabolica_platform::credentials::SystemBrowser` opens `https`/`http` URLs. No provider ships a shared token; `tests/no_embedded_credentials.rs` scans provider and app sources for known credential formats and opaque key-like literals.
- **Lifecycle** (`account`). `Account` restores the saved sign-in, runs the flow, rejects a superseded attempt and a reconnect that signed in to a different account, saves the credential and ends the old session. `sign_out` removes the credential first; if that fails the account stays connected and says so. It then ends the session, sends `AccountEvent::SignedOut` (controllers stop provider playback and pending work) and asks the service to revoke. `report(request, error)` marks the account expired when a request of the current session is rejected. Every change publishes a versioned `AccountStatus`; `changed(version)` resolves when a newer one exists.

The desktop registers accounts in `ui::accounts::init` and shows them in Settings › Sources & output. One task per account awaits `changed` and applies snapshots through a `SnapshotGate`, so changes made by controllers appear without a UI action. Account work runs on background threads. YouTube Music is registered without a flow until #15 provides one: its card can show and remove a saved sign-in, and Connect is disabled with "Sign-in is not available in this build."

## Commands, events and generations

- Views never block. A UI action becomes a command (for example `PlaybackCommand`) sent to the controller that owns the state. Controllers run off the UI thread.
- A controller bumps its `GenerationCounter` for each command that supersedes pending work (`PlaybackCommand::supersedes_pending_work`: play, stop, seek, next, previous, output change; new searches likewise). Async work returns `Tagged<T>` and the controller drops results from older generations, so a late seek or provider response cannot change a newly selected track.
- Long work polls a `CancelToken` between bounded units and never commits a partial result as complete.
- Controllers publish state as `Versioned<T>` from a `SnapshotSequencer`. The UI keeps a `SnapshotGate` per stream and ignores anything not newer than what it applied.
- The UI drains each event channel once per frame, runs `dispatch::coalesce` and applies the batch in one update. Position, progress and snapshot events coalesce to the latest; failures and other discrete events are all kept, in order.
- The UI shows position by extrapolating the engine's `PositionStamp`, not by counting frames itself.

## Desktop shell

`main.rs` opens the database at `%LOCALAPPDATA%\Parabolica\parabolica.db` (`~/Library/Application Support/Parabolica` on macOS, `$XDG_DATA_HOME/parabolica` on Linux) before the event loop starts, reads the saved appearance, sets the theme and only then opens the window, so the first frame is drawn in the right theme. If the database cannot be opened, the shell runs without it and shows the error above the page.

**Pages.** `ui::shell::Page` lists every destination and `SettingsSection` every settings section. `Shell::render_content` and `Shell::render_settings` match on them; a page without an implementation shows "Not available yet". A feature issue replaces its own arm with its view. A new destination also adds a `Page` variant and, for the sidebar, a `NAV_GROUPS` entry in `ui/shell/page.rs`; coordinate those edits with #3's owner. `Shell::navigate` records back/forward history. The player bar is `ui::player::Player`, an entity the shell creates and draws below the page.

**Appearance.** `ui::appearance` owns the saved `AppearanceMode` (preference `appearance.mode`, default Follow system), the last OS appearance and any active high-contrast colors; `ui::theme::apply` maps them onto Kit's theme. Follow system reacts to GPUI's window-appearance notification. Windows does not announce high contrast or animation-effect changes to GPUI, so both are re-read whenever the window becomes active. An active high-contrast theme replaces every color role with Windows system colors. Selections are saved off the UI thread, one write at a time, so the last choice is the one stored.

**Accessibility.** Sidebar and settings navigation use `ui::shell::nav::NavItem`, a focusable list item that reports the current page as selected. Gaps found in Kit 0.9.0 and how the app handles them:

- `SidebarMenuItem` cannot take keyboard focus. `NavItem` replaces it.
- A focus handle passed to `track_focus` ignores the element's `tab_stop`; create the handle with `cx.focus_handle().tab_stop(true)`.
- `Button::selected` changes only the presentation. Use list items with `aria_selected` where selection must be announced.
- `RadioGroup` has no arrow-key movement; each choice is a Tab stop and Space or Enter selects it.
- AccessKit names a `Role::Label` from its value: use `aria_value`, not `aria_label`, for text that should be read.

UI tests in `apps/desktop/src/ui/shell/tests.rs` run the shell in Kit's headless window (`gpui-kit` `test-support`). That feature makes Cargo resolve Kit's optional `gpui-fast` packages, whose exact pins hold `taffy` at 0.15.0, `unicode-properties` at 0.1.3 and `windows-capture` at 1.4.2, the versions in Kit's own lockfile.

## Real-time audio callback

`crates/audio/src/realtime.rs` defines `RealtimeRender`, which output backends call from the device callback. Code reachable from it must not allocate or free, lock, wait on other threads, touch files, storage or the network, or log. It copies prepared frames from a lock-free buffer filled by decoder workers, converts them to the device sample type, writes silence for a shortage and increments an `UnderrunCounter` that other threads read. Stream setup, reconfiguration after a driver reset and teardown run outside the callback.

`realtime::probe` (feature `test-support`) is a counting global allocator. Register it in a test binary and wrap a render call in `allocations_during` to assert zero allocations; the crate's own tests do this for the reference renderer.

## Local playback engine

`parabolica-audio` decodes and plays local files. `ui::player`, the library and the queue/playlist pages send nonblocking commands through `PlaybackCommands` and `controllers::queue::QueueService`. The queue worker owns engine events, source resolution and storage; the UI reads memory-only snapshots.

**Decoders** (`decode`). `Decoder` opens a file read-only with Symphonia 0.6 and returns interleaved `f64` PCM. Integer samples are scaled by `2^(bits-1)`, so the exact source value survives. Enabled container/codec pairs are `decode::SUPPORTED`: FLAC in FLAC, PCM in WAV, MP3 in MPEG audio and AAC-LC in MP4. Any other pair fails with `DecodeError::Unsupported`, which names both, for example "Opus audio in Ogg files is not supported". Opus, Vorbis, ALAC and FLAC in Ogg stay off until a second decoder validates them. FLAC and WAV must decode exactly: a packet that fails to decode, or whose first frame does not follow the previous one, is reported as corrupt, and a stream that ends before its stated length as truncated. MP3 and AAC packets that fail to decode are skipped, as their decoders recover at the next packet; a segment in which nothing decodes is reported as corrupt. Their stated length can be an estimate (an MP3 without a Xing header), so it is not checked. `SourceInfo::encoder_delay` carries container delay/padding. MP3 LAME/Xing trimming is already applied by Symphonia. The decoder removes any reported untrimmed delay/padding once, including a bounded padding tail and audible-frame seeks. Symphonia's MP4 reader ignores edit lists, so unreported AAC priming remains unknown and is not guessed. `channel_layout` retains the actual Symphonia channel layout for transition compatibility.

Fixtures and their reference checksums are generated by `crates/audio/tests/fixtures/generate.py`. Lossless fixtures (16/44.1 and 24-bit at 44.1–192 kHz, plus WAV) must decode to the PCM that libFLAC/libsndfile and FFmpeg produce.

**Threads.** `PlaybackEngine::start` spawns the controller thread. The output stream opens on the first play and stays open across tracks. Then a decoder worker thread fills an `rtrb` single-producer ring buffer (0.5 s) with frames already converted to the device rate and channel layout. The backend's device thread calls `StreamRenderer` (the `RealtimeRender` implementation), which only reads atomics and the ring buffer, applies app volume with a 10 ms full-scale ramp and writes the device sample type. A new sequence or seek starts at the target gain without a fade. Compatible track boundaries do not alter the gain.

**Generations and flushes.** Sequence selection/replacement, seek and stop advance the controller's `GenerationCounter`. Compatible entry boundaries remain in one generation. `PlaybackSequence` holds the caller's effective order and repeat mode. Its worker opens upcoming files ahead of the output clock, continues the same converter across compatible source rates/layouts and reports boundaries by absolute output-frame offset. Repeat and shuffle use this same path. A 65,536-sample cache bounds decoded repeats; larger files are reopened. Sequence edits rebuffer and may briefly interrupt sound. `EntryBoundary` events are never coalesced and the queue worker processes them before UI snapshot coalescing. Before writing a new generation's audio, the worker publishes it as the requested epoch. The renderer discards everything buffered and acknowledges it, and the worker waits for that acknowledgement. The controller drops worker reports from older generations. `PlaybackEngine::seek` names the queue entry it was aimed at. The queue worker also tags seeks with the observed generation, so an old seek for a repeated entry cannot alter its newer playback.

**Rate and format.** WASAPI Shared (`output::wasapi_shared`) opens the default console endpoint at its mix format (`GetMixFormat`), event-driven, on an MMCSS "Pro Audio" thread. Because the stream matches the mix format, Windows does not convert it. When the source rate differs, the worker resamples once with rubato's FFT resampler in `f64` (`engine::resample`). Each continuous compatible segment has exactly `round(frames × to / from)` output frames. Rate/layout changes flush the old converter tail, configure the next converter and emit `SourceFormatChanged`; the player displays the change. The device remains at its negotiated shared format; incompatible boundaries have no continuous-output guarantee. Mono plays on both front channels; extra device channels are silent; a source with more channels than the device fails with a channel-layout error. `f32` output is exact for sources up to 24 bits. Integer mix formats are rounded without dither.

**State.** The controller publishes `EngineSnapshot`: the domain `PlaybackSnapshot` (transport, position stamp on the output clock, duration, `PlaybackRoute`, `SignalPath`, gain Bypassed, volume), plus the file path, decoder `SourceInfo`, the `NegotiatedOutput`, the underrun count and the last failure. Position events refresh the stamp every 500 ms while playing and when it changes otherwise, for example after a pause fade. A device that disappears blocks playback with `OutputLost`; playing again reopens the default endpoint. ReplayGain, EQ, crossfade and other effects are off.

## Local collection

`parabolica-library` uses typed `Storage` methods for indexing and queries. This coordinated dependency keeps SQL and connections in `parabolica-storage`; the library still has no UI or audio dependency. Issue #6 adds `storage::collection`, registers it in storage, and exposes two crate-internal helpers from #4's library repository for FTS folding and move reindexing. Migration `0006_01_collection.sql` adds albums, artists, song projections and memberships. #7 consumes stable file IDs for queues, #10 consumes album membership, #12 consumes tags and groups, and #16 can compose local and remote queries independently. No existing domain or provider contract changes.

`index::scan` streams a directory traversal and commits metadata per file. Size, mtime and OS identity skip unchanged files. A SQLite trigger marks the song projection dirty on physical-record changes; a successful projection update clears it, so interruption between core metadata and projection writes is retryable. Safe `file-id` calls provide Unix device/inode or Windows volume/file IDs. Renames with one missing OS-identity match across configured readable roots keep the file ID, regardless of root scan order; a whole folder move can retain its album ID. The root stores a volume identity. A different volume at the same path requires explicit relocation. Offline roots retain their collection. Traversal skips symlinks and Windows reparse points. Cancellation, inaccessible subdirectories and incomplete traversals never mark unseen files missing. A successful pass marks missing records without deleting tags, queue sources or logical-track links.

The service has one indexing thread and a 32-command queue. Watch callbacks coalesce changes and overflow/errors into full reconciliation, with a 500 ms debounce capped at two seconds and a 30-second periodic pass. Restart reconciles every root and cancels interrupted index jobs. A command can cancel the current pass; user cancellation pauses automatic scans until Rescan or another folder command. Failed commands also resume an interrupted pass, while their error remains visible until the next command. Index jobs record Running, Completed, Incomplete, Failed or Cancelled in storage. Progress reaches GPUI in batches, with at most 32 retained error details across the pass. Collection and folder settings display those paths and a Rescan action. Indexing yields between files while playback is active.

FLAC indexing reads stream information and at most 1 MiB of Vorbis comments, skips pictures by block length, and never decodes PCM. Other enabled containers preserve raw text, ratings, ReplayGain and custom tags alongside canonical browse fields using Symphonia metadata. Size checks cover ID3, WAV, MP4 and Ogg headers; WAV/MP4 preflight stops at 4096 headers and 8 MiB of non-audio metadata. Cancellation is checked between metadata blocks and stream reads. Artwork loads after browsing requests it. Embedded front covers and local `cover`/`folder` JPEG or PNG files feed thumbnails capped at 256 px. Encoded artwork is limited to 8 MiB, source images to 16 million pixels and 8192 px per dimension, decoder allocation to 128 MiB, and the thumbnail cache to 64 entries. The UI retains artwork for at most one 96-item page. No reference artwork is bundled.

Automatic groups use root, album folder, album artist, album title and separate edition identifiers. Disc directories with disc-number tags share their parent album folder. Compilation flags provide Various artists when album artist is absent; track artists retain separate browse identities. Edition tags and folder fallback prevent title-only merges. Users can select songs across pages, group them into a chosen album, create a separate group or reset grouping. Overrides survive rescans. Future album analysis must check current membership before using a result.

Relocating a root verifies a matching sampled signature before changing its path and volume. Missing-file recovery uses size plus a bounded SHA-256 sample of the first and last 64 KiB as a hint, followed by explicit user selection. It preserves the original file and album IDs. An already indexed duplicate is absorbed only if it has no queue, logical-track, CUE, job or explicit-group references; otherwise recovery returns a conflict. This hint is not a whole-file integrity hash. Audio data is never written.

`ui::library::LibraryView` supplies Albums, Artists, Songs, folder browsing and album detail. Sources & output and Library settings use `ui::sources::folders` for folder health, relocation and removal confirmation. The shared shell forwards search and page changes; `main.rs` starts the collection service with the same storage handle used by appearance. Storage queries run on background workers, use FTS5 and return bounded pages. Search generations reject superseded results. GPUI `uniform_list` renders visible grid rows or song rows. Sort, folder and availability filters run against stored projections. Play album resolves indexed file IDs in disc/track order and dispatches them to the existing player. Its small sequential album list stops on failure; full persisted queues, next-track prefetch and gapless transitions remain #7's responsibility.

Verification and the Windows runtime checklist are in [local collection evidence](implementation/local-collection.md).

## Changing a contract

After #2 merges, a change to `crates/domain/**` (other than #7's `queue/**`), the root manifests or `realtime.rs` is a coordinated change: say which issues consume the affected types, keep existing variants and fields unless every consumer is updated in the same change, and update this document.

## Queue, playlists and session recovery

`QueueService` has a bounded 64-command inbox and one worker. Full inbox failures appear in the shared snapshot. The worker resolves local IDs through storage, including persistent external-file IDs for Open file and imported paths. Library actions pass stable IDs, so unavailable songs stay in the queue. CUE/provider identities stay intact and show their unsupported playback state until their own adapters implement it. Source failures retain Retry and Skip; Retry allows three attempts per entry, while explicit Skip visits at most the queue length. Output failures do not mark the source unavailable. Automatic playback stops visibly at an unavailable entry; it does not hide that entry or loop over failures.

`storage::session::save_session` commits queue entries, insertion and shuffle order/seed, current ID, position, repeat, previous navigation, unavailable states and the latest 1000 listening starts together. It shares the existing queue transaction helpers. Entry IDs never change during restoration, and restored playback is paused. The player registers an app-quit hook that runs `QueueService::shutdown` off the UI thread and joins the final checkpoint. Forced termination can lose progress since the last committed checkpoint, at most the normal 500 ms interval plus pending work.

Migration `0007_01_queue_playlists.sql` adds external file paths, playlists/entries, app likes, queue navigation/unavailability and listening history. `storage::playlists` owns their SQL and source display/resolution. Playlist entries have independent stable IDs and retain duplicates. Local app likes may refer to any source; they never write a provider like. Removing a library root leaves queue/playlist/history identities visible as unavailable.

M3U8 import accepts UTF-8, BOM/CRLF and relative Unicode file paths. Parsing, external identities and playlist insertion are transactional. Limits are 8 MiB, 10000 entries and 32768 bytes per line. URLs, NUL and line-break path injection are rejected. Export preserves duplicate and missing file paths, prefers relative paths and lists every non-exportable CUE/provider identity. The app refuses a partial export and does not overwrite an existing destination. Import/export disk I/O runs on the queue worker.

Queue and Playlists are sidebar destinations. Queue controls operate on effective order; History shows actual starts. Playlist controls create/rename/delete with confirmation, append the queue, move/remove entries, play now/next/later, like in app, and import/export through native path dialogs. Library album actions and selected songs also offer Play next/later; selected songs can be liked in the app. The persistent player uses the same next/previous/shuffle/repeat commands. Linux headless output tests are separate from Windows application and WASAPI acceptance. See [queue/session evidence](implementation/queue-and-session.md).
