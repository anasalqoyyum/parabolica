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
| `parabolica-library` | `crates/library` | domain | Indexing, collection queries, CUE indexing, relocation |
| `parabolica-audio` | `crates/audio` | domain | Decode, engine, DSP, output backends, callback contract |
| `parabolica-replaygain` | `crates/replaygain` | domain | Loudness analysis and `REPLAYGAIN_*` mapping |
| `parabolica-tags` | `crates/tags` | domain | Tag/artwork reads and safe album writes |
| `parabolica-cue` | `crates/cue` | domain | CUE parsing |
| `parabolica-platform` | `crates/platform` | domain | Devices, ASIO enumeration, credentials, media controls, lifecycle |
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
| `apps/desktop/src/main.rs`, `apps/desktop/src/ui/{shell,theme,appearance}/**` | #3 |
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
- `Storage` methods block. Controllers and workers call them; the UI thread never does. `Storage` is `Clone`; clones share the writer, the readers and the lock.
- A `<database>.lock` file holds an OS lock for the life of the handle, so a second app instance gets `OpenError::InUse`.
- Nothing reachable from the audio callback touches storage. Playback position is checkpointed by the queue controller on pause, seek, clean exit and a periodic timer, with `save_playback_position`.
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

**Queue** (`queue`). Each insertion creates new `QueueEntryId`s, so a duplicated source is two entries. Moves and removals keep IDs. #7 extends this module with shuffle, repeat, history and persistence.

## Commands, events and generations

- Views never block. A UI action becomes a command (for example `PlaybackCommand`) sent to the controller that owns the state. Controllers run off the UI thread.
- A controller bumps its `GenerationCounter` for each command that supersedes pending work (`PlaybackCommand::supersedes_pending_work`: play, stop, seek, next, previous, output change; new searches likewise). Async work returns `Tagged<T>` and the controller drops results from older generations, so a late seek or provider response cannot change a newly selected track.
- Long work polls a `CancelToken` between bounded units and never commits a partial result as complete.
- Controllers publish state as `Versioned<T>` from a `SnapshotSequencer`. The UI keeps a `SnapshotGate` per stream and ignores anything not newer than what it applied.
- The UI drains each event channel once per frame, runs `dispatch::coalesce` and applies the batch in one update. Position, progress and snapshot events coalesce to the latest; failures and other discrete events are all kept, in order.
- The UI shows position by extrapolating the engine's `PositionStamp`, not by counting frames itself.

## Real-time audio callback

`crates/audio/src/realtime.rs` defines `RealtimeRender`, which output backends call from the device callback. Code reachable from it must not allocate or free, lock, wait on other threads, touch files, storage or the network, or log. It copies prepared frames from a lock-free buffer filled by decoder workers, converts them to the device sample type, writes silence for a shortage and increments an `UnderrunCounter` that other threads read. Stream setup, reconfiguration after a driver reset and teardown run outside the callback.

`realtime::probe` (feature `test-support`) is a counting global allocator. Register it in a test binary and wrap a render call in `allocations_during` to assert zero allocations; the crate's own tests do this for the reference renderer.

## Changing a contract

After #2 merges, a change to `crates/domain/**` (other than #7's `queue/**`), the root manifests or `realtime.rs` is a coordinated change: say which issues consume the affected types, keep existing variants and fields unless every consumer is updated in the same change, and update this document.
