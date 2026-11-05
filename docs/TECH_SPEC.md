# Parabolica technical specification

Status: proposal for a personal Windows player. Updated 2026-11-06. This document specifies behavior; no playback implementation or Windows hardware validation has run yet.

## Product decisions

Build a dependable local player first, using Rust and GPUI Kit as the selected UI stack. Validate the Windows integration before implementation expands. Add a unified YouTube Music library through an isolated experimental adapter. The user accepts an unofficial integration for personal use. That acceptance does not establish playback access or permission from Google.

Local FLAC and streaming items share navigation, search, playlists, and queue controls. Keep their source identities and playback capabilities distinct. Never silently replace a local release with a streamed version; masters, track order, availability, and loudness can differ.

The visual proposal follows the supplied light desktop reference. Album art leads the library. Audio details sit behind an expandable player control and dedicated settings, with important changes visible during playback.

## Priorities and expected features

The user's priorities determine execution order. The [implementation issue set](implementation/README.md) covers the complete delivery, including requirements originally marked proposed or convenience. P0/P1 does not defer accepted work to a later milestone. Windows is the primary validation platform; the same work breakdown includes concrete macOS/Linux desktop ports.

| Priority | Requirement | Expected behavior |
| --- | --- | --- |
| P0 | Accurate lossless FLAC decoding | Preserve decoded PCM precision. Accept stereo 16/44.1, 24/44.1, 24/48, 24/88.2, 24/96, 24/176.4, and 24/192 fixtures. Detect corrupt or unsupported files without crashing. |
| P0 | Album and track ReplayGain | Read existing tags; scan tracks and albums; choose Off, Track, Album, or Auto during playback. Preserve relative track loudness in album mode. |
| P0 | Gapless playback | Continuous local album playback without inserted silence, missing samples, or clicks for compatible output formats. Respect encoder delay/padding for other supported codecs. |
| P0 | Reliable audio output | Explicit device selection through WASAPI Shared, WASAPI Exclusive, or ASIO; documented sample-rate handling; no enabled-by-default EQ, crossfade, or effects. Validate each backend, including the user's FiiO K3 and its installed ASIO driver. |
| P0 | Library management | Add folders, incremental indexing, fast search, Albums/Artists/Songs views, sorting, filtering, and reliable metadata browsing. |
| P0 | Reliable queue | Play now/next/later, reorder, remove, shuffle, repeat off/all/one, history, and restore the queue and position after restart. Restore paused. |
| P1 | Unified FLAC and YouTube Music library | Federated search and mixed app-owned playlists/queues, with source badges and explicit account/availability states. |
| P1 | Artwork and metadata editing | Edit local tags and artwork with a preview and a clear write action. Include album-level metadata and album-art import from Discogs, with release selection, track mapping and review before writing. Streaming catalog metadata remains provider-owned. |
| P1 | CUE sheets | Parse external CUE sheets, create virtual tracks over a FLAC image, preserve boundaries, and play continuously. |
| P1 | ReplayGain peak protection | Use peak-aware gain attenuation with an optional true-peak scan. Explain when protection reduces requested gain. |
| P1 | Device and format information | Expand the audio signal path from the player. Distinguish decoded source, app processing, negotiated stream, and reported endpoint configuration. |
| P0, proposed | Recoverable failures | Handle removed drives, changed files, device disconnects, corrupted metadata, disk-full errors, and interrupted scans. |
| P0, proposed | Keyboard and Windows integration | Keyboard navigation, accessible controls, media keys, Windows media-session controls, native file dialogs, and high-DPI support. Validate Narrator before committing to GPUI. |
| P0, proposed | Data ownership | Keep library, queue, analysis, and preferences local. Provide a backup/export route and schema migrations. Keep credentials out of those exports. |
| P1, proposed | Collection tools | Folder view, missing-file repair, compilation/multi-disc handling, playlist import/export, and an album identity override. |
| P1, proposed | Basic format breadth | MP3, WAV, and supported AAC/M4A files after FLAC correctness. Verify each container/codec combination. Opus requires a verified decoder beyond the initial FLAC path. |
| Required | Appearance | Light, Dark, and Follow system. Default to Follow system, persist the choice, and react to Windows app-theme changes without interrupting playback. |
| P1 | Convenience | Smart playlists, ratings, opt-in scrobbling, sleep timer, and lyrics from local or permitted provider sources. |

Streaming search, album/artist pages, likes, playlists, recommendations, radio, unavailable-track handling, and account reconnect are expected at P1 where the provider supports them. Distinguish local app likes from remote likes. Remote writes require an explicit user action and may fail independently. Do not promise provider downloads, stream ReplayGain scanning, gapless streaming, crossfade between backends, or an EQ over provider playback.

## Feasibility and stack recommendation

### GPUI Kit

[GPUI Kit](https://gpui-kit.com/docs/) is the user's selected application UI framework, built on GPUI. Use its `gpui-component` controls for buttons, inputs, selects, menus, dialogs, sliders and virtualized track tables/lists. Compose the album grid, player bar and signal inspector as custom views using those components and `gpui-base` behavior where needed. Map Parabolica's semantic Light/Dark tokens into Kit's theme system and connect Follow system to OS appearance changes.

Depend on `gpui-kit` and use its matching GPUI re-exports. Pin a tested Kit release, lockfile and Rust toolchain after the Windows spike; do not independently override one of Kit's GPUI snapshot packages. The [installation guide](https://gpui-kit.com/docs/installation/) explains the version alignment. Kit documents its software under Apache-2.0. Verify the selected release and transitive licenses when packaging. The application does not need `gpui-shell` or JavaScript extensions for the proposed music-player UI.

Keep audio decoding, ReplayGain, WASAPI/ASIO, indexing and provider integration independent of the UI framework. Kit supplies controls and interaction infrastructure; our application supplies audio behavior and the custom views shown in Doop.

The spike must prove a Windows build, text/IME input, virtualization of a large library, artwork caching, native dialogs, media integration, 100/150/200% DPI, keyboard navigation, and usable Narrator exposure. A fast painted interface alone is insufficient. Do not adopt the framework if essential accessibility cannot be delivered within the project's scope.

Future platforms mean macOS and Linux desktop first. Keep playback, indexing, and provider code independent of GPUI. Mobile support needs its own UI and lifecycle evaluation; no claim of GPUI support for every platform is made here.

### Proposed dependencies

| Responsibility | Candidate | Decision to verify |
| --- | --- | --- |
| UI, selected | GPUI Kit with `gpui-component` and `gpui-base` | Pin Kit and its matching GPUI snapshot; validate custom theme styling, accessibility, Windows packaging and rendering. |
| Local decode | Symphonia | FLAC support and exact PCM checks; gate other codecs individually. |
| Output | CPAL with WASAPI and the Windows `asio` feature | Shared/exclusive WASAPI plus ASIO driver enumeration, sample formats, channel routing, buffers and recovery on the pinned release. |
| Loudness analysis | Rust `ebur128` | ReplayGain 2.0 target conversion, channel mapping, album aggregation and peak behavior. |
| Local tags | `lofty` | FLAC Vorbis comments, pictures, preservation of unknown fields, safe album write jobs, and Discogs field mapping. |
| Album metadata source | Discogs REST adapter | Read-only search/release/version lookup, artwork access, authentication options, rate-limit handling and source attribution. See [research](research/DISCOGS.md). |
| Database | SQLite through `rusqlite`, FTS5 | Transactions, migrations, search and worker ownership. |
| Background I/O | Tokio; bounded blocking workers | Keep decoding, scanning, artwork and filesystem work off the UI/audio callback. |
| File events | `notify` | Debounce, reconcile missed events, support removable volumes. |
| Windows APIs | `windows` crate | Device properties, WASAPI details, Credential Manager and media controls. |
| Experimental catalog | Provider adapter | Compare a Rust client with a narrow `ytmusicapi` helper during the spike. Avoid adding a Python runtime without a demonstrated need. |

[Symphonia](https://github.com/pdeljanov/Symphonia) documents its codec support and MPL-2.0 license. [CPAL releases](https://github.com/RustAudio/cpal/releases) include WASAPI exclusive-mode work, so validate the current API instead of assuming it is unavailable. The [Rust ebur128 project](https://github.com/sdroege/ebur128) implements loudness measurement. No exact dependency set has been compiled together. Review transitive licenses before packaging.

## YouTube Music integration

There is no supported general YouTube Music playback SDK established by this research. [ytmusicapi](https://ytmusicapi.readthedocs.io/en/stable/) emulates browser requests for catalog/account operations and explicitly says Google does not endorse it. Catalog access is separate from obtaining and playing an authorized media stream.

Google's [YouTube API policies](https://developers.google.com/youtube/terms/developer-policies) restrict audio separation, hidden background playback and downloading through API clients. Premium membership does not remove those API restrictions. Treat integration choice and possible public distribution as unresolved until a current terms review and technical spike establish a viable path.

[YouTube Music's quality documentation](https://support.google.com/youtubemusic/answer/9076559?hl=en) describes AAC/Opus quality settings up to 256 kbps. Do not label streams lossless or assume the adapter can obtain that maximum. Show verified stream information only; show Unknown where unavailable.

Evaluate two paths:

1. A separate browser-backed YouTube Music session. Test actual sign-in support, media controls, background behavior, account persistence and GPUI/WebView2 integration. Embedded sign-in may be blocked. Do not collect Google passwords in native UI. A browser-backed session may own its output separately from the native FLAC engine.
2. An unofficial catalog adapter plus a separately verified playback resolver for personal experimentation. Test entitlement, signed URL expiry, seeking, playback restrictions, authentication expiry and rate limits. Do not infer that a successful metadata query proves playback.

Use the system browser for a supported authorization flow where possible. Keep tokens or session credentials in Windows credential storage. Never log them, export them, or request them through ordinary chat. Signing out removes stored credentials and stops provider playback. Do not distribute another application's embedded credentials.

The provider reports capabilities such as search, playlist read/write, seek, quality inspection, app-owned PCM, background playback and gapless. Hide or disable unavailable actions with a specific reason. Local browsing and playback work with the provider signed out or broken. Provider-owned playback has no claimed native ReplayGain or output format control.

A failed spike leaves P0 local playback intact and marks P1 streaming blocked. Do not replace the requested unified player with an external-browser shortcut without a product decision.

## Architecture and threading

Use a small Rust workspace with app, domain, library, audio, replaygain, provider, and platform modules. Start with modules where a crate split adds no useful boundary. The domain owns stable IDs, queue commands, playback state and source-aware models; it imports neither GPUI nor WASAPI.

The UI sends commands to a playback controller and observes snapshots/events. The controller selects a local PCM backend or provider backend. Exactly one backend may produce sound. Switching sources stops the old backend before starting the next; do not promise a seamless transition across independent engines.

For local audio, decoder workers feed a bounded PCM ring buffer. The device callback consumes prepared frames only. It performs no allocation, locks, database queries, network calls, file access or logging. Report underruns through counters read outside the callback. Timestamp the state used by the UI so the seek position does not drift with UI frame rate.

Use separate bounded workers for scanning and indexing; lower their priority during playback. A database owner serializes writes. Batch events to GPUI instead of dispatching one update per file or audio block. Art decoding uses size limits, thumbnails, cancellation and a bounded cache.

## Local audio pipeline

The ordered path is decode to PCM, apply the chosen ReplayGain/preamp, apply any explicit optional DSP, resample if required, apply app volume, convert/dither if reducing integer precision, and send to the negotiated device stream. Keep processing at sufficient floating-point precision. A FLAC decoder can be lossless while the output path changes samples.

Default to shared WASAPI and the Windows default endpoint. Query the actual mix/stream format. Resample once in a documented stage when needed. Exclusive mode requests a supported source-compatible format, reports failure, and offers shared mode as an explicit choice. It must not silently switch modes, devices, sample rates or DSP.

### ASIO and the FiiO K3

ASIO is a required Windows local-playback backend. The user owns a FiiO K3. [FiiO's K3 specifications](https://www.fiio.com/K3) list ASIO, and its [USB mode guidance](https://www.fiio.com/newsinfo/130092.html) describes the Windows driver requirement for USB 2.0 mode. Validate the user's exact K3 revision, USB mode, installed driver, and supported formats rather than assuming a generic USB endpoint exposes ASIO. The app must not install or change drivers automatically.

[CPAL](https://github.com/RustAudio/cpal) documents a Windows ASIO backend behind its `asio` feature, with SDK and build-tool requirements. Include it in the Windows feasibility build. Pin and document the required SDK/toolchain, verify its license requirements, and test a native alternative if CPAL cannot expose essential driver capabilities. A WASAPI-only build does not satisfy the updated requirement.

The Listening device page separates output backend from device/driver. It offers WASAPI Shared, WASAPI Exclusive, and ASIO, then enumerates compatible endpoints or ASIO drivers for that backend. Persist backend, stable endpoint/driver identity, output channel mapping, buffer preference, and sample-rate policy. Driver names come from enumeration; the FiiO labels in mockups are illustrative. A 64-bit application must account for ASIO driver architecture when diagnosing an empty list.

For ASIO, expose the selected driver's output channels, reported sample type and valid bits when available, negotiated rate, buffer size in frames, and reported output latency when available. Buffer duration computed from frames/rate is not round-trip latency. Offer Open driver panel only when the integration supports it. If the driver owns buffer configuration, report that instead of offering an ineffective app control. Follow source requests the source rate; an unsupported request pauses with an explanation and an explicit fixed-rate or backend choice.

The ASIO path bypasses the Windows shared mixer. Windows shared mix format and session volume must not appear as active stages. The app's own volume, ReplayGain, resampling, format conversion and driver/hardware processing remain relevant. Hardware volume may be outside the app's visibility. ASIO alone does not establish bit-perfect playback.

Handle missing drivers, a busy driver, unsupported rates, driver reset/rate-change messages, buffer changes, unplug/replug and sleep/resume. Re-query capabilities after a reset. Stop and reconfigure outside the real-time callback. Keep compatible album transitions on one stream. Never silently fall back to WASAPI or a different device; show the failed selection and let the user choose. Verify theme changes do not reset or reopen the ASIO stream.

Keep the output stream open across compatible local track transitions. Prefetch the next file, apply decoder delay/padding correctly, and avoid seek/track-change fades at gapless boundaries. On a rate or channel-layout change, report reconfiguration; continuous playback across incompatible formats is not guaranteed. Use short ramps for user-initiated volume jumps without altering musical boundaries.

Lossless is a source/decoder property. A bit-perfect mode would additionally require unity gain/volume, no ReplayGain, no resampling, no effects, appropriate sample conversion and a verified direct output path through WASAPI Exclusive or ASIO. Do not display a global bit-perfect badge. Hardware DSP and driver processing may remain outside the app's visibility.

On endpoint loss, pause and show the missing device. Default to an explicit choice before rerouting from headphones to speakers. Remember a selected endpoint's stable ID and handle a device returning with different capabilities.

## ReplayGain behavior

Use ReplayGain 2.0 measurement with the compatible -18 LUFS reference, rather than storing broadcast -23 LUFS gains under conventional ReplayGain tags. Foobar2000 scanner behavior is the user experience reference, not a promise of byte-identical results across versions. See the [revised specification](https://wiki.hydrogenaudio.org/index.php?title=ReplayGain_2.0_specification) and [foobar2000 scanner preferences](https://wiki.hydrogenaudio.org/index.php?title=Foobar2000%3APreferences%3AReplayGain_Scanner).

### Grouping and jobs

Support Scan tracks, Scan as one album, and Scan albums by tags. Default automatic grouping uses album artist, album title, edition/release identity where available, and a folder fallback. Treat album artist as distinct from track artist. Multi-disc sets form one album by default; allow grouping by disc or manually overriding the group. Never merge same-titled releases solely by title.

Show the proposed groups before analysis when selection is ambiguous. Track count tags can flag an incomplete album but cannot prove completeness. Label missing/suspected tracks and require the user to accept the selected group. Adding/removing tracks invalidates its album analysis.

Decode the complete audio content. Calculate track loudness and peaks separately. Calculate album loudness from the combined gated measurement distribution for the selected group, using a verified multi-state aggregation implementation. Never average track dB gains. Album peak is the largest measured peak in that group. Account for channel layouts and differing sample rates during analysis without rewriting source audio.

Show Queued, Scanning, Ready, Incomplete, Failed, Stale and Cancelled. Display track/album progress, current file and failures. Support cancellation between processing blocks. Resume by reusing unchanged complete track analyses. Only publish album results when the accepted group has finished successfully. A skipped or failed track leaves the group incomplete until the user explicitly changes that group.

### Storage and tag interoperability

Default to analysis in the app database, leaving files unchanged. Offer an explicit Write tags action after preview. Record album membership, file identity/content signature, analyzer version, algorithm, target and peak mode. Prefer the user's selected analysis source; show when file tags and app results differ rather than guessing from timestamps.

For FLAC, read/write conventional Vorbis comment fields `REPLAYGAIN_TRACK_GAIN`, `REPLAYGAIN_TRACK_PEAK`, `REPLAYGAIN_ALBUM_GAIN`, and `REPLAYGAIN_ALBUM_PEAK`. Gain uses signed dB text; peak is a unitless linear amplitude, not dB. Preserve unrelated metadata, pictures and audio frames. Do not apply gain destructively to PCM.

Write through a verified temporary sibling file, flush it, then replace using the platform's safe replacement API. Keep the original intact on failure, reject changes detected since analysis, and serialize writes to a given file. Record per-file outcomes; do not pretend a multi-file album update is atomic. After a partial write, offer retry and retain the complete database result. A read-only file can still use database analysis. Verify unchanged decoded PCM after a successful tag write.

### Playback modes

- Off bypasses ReplayGain. App volume remains separate.
- Track uses track gain for comparable loudness between songs.
- Album applies the same album gain across the selected release.
- Auto uses album mode for ordered album playback, and track mode for shuffled/mixed queues. Resolve it from the playback context and display the effective mode. A missing gain uses 0 dB initially and shows Not scanned; do not silently substitute track gain during album playback.

Preamp defaults to 0 dB. Scan results do not take effect halfway through a track; apply on the next start unless the user explicitly changes playback gain. Ramp an explicit live change to avoid clicks.

Peak protection remains P1 per the user's priorities. Until it exists, show positive-gain clipping risk and keep ReplayGain playback off by default. When enabled, calculate the upper gain bound from measured peak and the configured headroom. In album mode use one album-wide bound so protection preserves track relationships. Do not normalize each track independently under album mode.

For peak `p > 0`, a proposed bound in dB is `ceiling_dBFS - 20*log10(p)`. Applied gain is the lesser of that bound and requested gain plus preamp, conservatively independent of listening volume. Silence, non-finite measurements, absent peaks and stale analysis need explicit handling. Display requested and applied values when protection intervenes. True-peak scanning is separate from sample-peak scanning; label the method. Later processing and hardware can still create peaks, so never promise this proves end-to-end absence of clipping.

### CUE interaction at P1

Virtual tracks inherit an album identity and exact image offsets. Scan both the whole image for album measurement and the declared track ranges for track measurement without double-counting samples. Store per-virtual-track gains in the database or a documented sidecar. One FLAC image cannot hold distinct conventional file-level track gain tags for every CUE track; explain that limitation before writing tags. Preserve INDEX 00/pregap policy and test boundaries.

## Audio output transparency

An expandable Signal path button is always available in the player, with a compact summary such as `Shared · 48 kHz · Processing`. The expanded panel shows each stage and the provenance of its values:

| Stage | Example | Truth required |
| --- | --- | --- |
| Source | Local FLAC, 24-bit integer, stereo, 96 kHz | Read from the selected file and active decoder, distinct from stream delivery format. |
| Decode | 32-bit float PCM, stereo, 96 kHz | Actual internal representation. A float container is not extra source resolution. |
| Processing | Album ReplayGain -7.4 dB, preamp 0 dB, EQ off | Read active DSP state, including protection and app volume. |
| Resampling | 96 kHz → 48 kHz | Show actual conversion and its stage. If bypassed, say so. |
| App output | WASAPI shared, 32-bit float, stereo, 48 kHz | Use the stream format returned by the active backend. |
| Windows/device | Selected USB DAC; reported mix format and system volume if available | Distinguish endpoint-reported settings from the DAC's physical conversion. |

The examples are illustrative. Do not infer device output from a FLAC badge. Show app volume separately from Windows volume and identify whether app samples change. In shared mode, Windows/driver effects and other apps can alter the path; label downstream processing Unverified unless the app can inspect it. In exclusive mode, report negotiated format and exclusive status, while retaining uncertainty about device-internal DSP.

For ASIO, replace the Windows mix stage with the selected ASIO driver, actual channel mapping, sample type/rate and buffer size. Mark the Windows mixer Bypassed and Windows session volume Not in this path. Report driver latency only when available, and keep hardware volume/DSP Unverified when unreadable. The updated mockup shows an illustrative FiiO K3 path at 96 kHz with resampling off and album ReplayGain active; no bit-perfect claim follows from selecting ASIO.

For browser/provider playback, show source delivery details only if known; label output `Managed by YouTube Music/browser` and sample format `Unavailable` when the native backend cannot inspect it. Native ReplayGain, device selection, ASIO and exclusive mode must not appear effective for that backend.

Update the panel on device/track/rate/gain changes. Keep it keyboard reachable, dismissible with Escape, and copyable as text. Link to Audio settings and provide an intelligible explanation of why a path is processed. Audio transparency is a P1 feature per the user's table; trustworthy internal state/reporting is part of the P0 engine design.

## Library, identity and persistence

### Album metadata import

Discogs import is required in an album's Files & tags view. [Mp3tag's online tag import](https://docs.mp3tag.de/tag-sources/import/) is the workflow reference. One import job covers the selected album, including all its discs. Sync means a user-triggered reimport from the selected release with a fresh review. The [research note](research/DISCOGS.md) records first-party response examples, field behavior and open API-use questions.

#### Album workflow

1. Choose Update from Discogs from the album's Files & tags view or an album selection. Capture stable album and file IDs, the selected discs, and current tags/artwork. Flag a selection containing different albums before proposing shared fields.
2. Search by album artist and title, refine by barcode/catalog number, or paste a release URL/ID. Show candidate title, artists, year, country, format, label/catalog number and release ID. Identifiers narrow a search but may still return multiple editions.
3. Choose a specific release. A Discogs master groups editions and can have a different tracklist. Use a master to browse versions, then fetch the selected release for actual metadata. Preserve a link back to the source. [Discogs master guidelines](https://support.discogs.com/hc/en-us/articles/360005055493-Database-Guidelines-16-Master-Release)
4. Review the entire local album beside the release tracklist, grouped by disc. Suggest matches using existing disc/track numbers, titles and durations; let the user reorder or remap them. Display missing files, extra source tracks, non-audio rows and ambiguous matches. Never shift later mappings automatically because one track is absent. Before writing, every included file needs a resolved mapping; explicitly excluded files remain untouched.
5. Review Current and Discogs values for album fields and mapped track fields. Choose fields to replace, fill only when empty, or keep. A missing source value keeps the local value by default. Artwork has its own selection and destination controls.
6. Apply the accepted changes as one album job. Report per-file outcomes and retry failed targets without duplicating covers or overwriting later edits. A partial write remains Partial, with the successfully changed files visible.

#### Metadata and track mapping

| Source | Local field or behavior |
| --- | --- |
| Release title and release artist credits | `ALBUM`, `ALBUMARTIST`; use the chosen edition's title and credited artist name/join values. |
| Track title and track artist credits | `TITLE`, `ARTIST` per mapped file. Keep compilation album artist separate from individual track artists. |
| Track positions and release order | `TRACKNUMBER`, `DISCNUMBER` and totals when known; preserve raw positions such as `A1`, `B2` and `2-1` separately. |
| Release date/year | `DATE` with known precision. An unknown month/day remains unknown; do not invent a complete date or treat edition date as original-release date. |
| Labels and catalog numbers | `LABEL`, `CATALOGNUMBER`, preserving multiple label/catalog pairs in the app model. Let the user choose a file-format representation. |
| Genre/style arrays | `GENRE` plus a separate `STYLE` field when selected. Preserve their distinction and multi-value structure. |
| Country, credits and edition details | Optional fields. Release-wide credits and track-specific credits retain their scope. A physical release format such as Vinyl is edition metadata, not the file's codec. |
| Source identity | Chosen release ID, optional master ID, source URL, fetch time and mapping provenance, subject to the data-use decision below. |

Use the selected release response, not the combined artist/title display string from a search result. Preserve artist IDs, credited name variations and joins in the source model; do not destructively normalize artist names or credits. Duration is a matching aid and never replaces decoder-measured file duration.

Discogs tracklist rows can include `track`, `heading` and `index` types, with absent positions or durations. Preserve source order and grouping, including nested structures if supplied. Headings must not become file rows. An index or movement grouping requires review when the local rip combines or splits its audio differently. For ambiguous one-to-many mappings, keep affected files excluded until the user chooses a supported mapping. Do not silently split or merge audio. [Discogs tracklisting guidelines](https://support.discogs.com/hc/en-us/articles/360005055373-Database-Guidelines-12-Tracklisting)

Preserve existing ReplayGain, ratings, play counts, unrelated/custom tags and pictures outside the selected change. Importing tags must not merge same-named editions or replace stable library/queue identities. Keep audio analysis valid when decoded audio and album membership are unchanged. If an accepted regrouping changes album membership, invalidate that group's album analysis rather than quietly reusing it.

#### Artwork at album scope

The album job can choose the release's primary image or another returned image, inspect available dimensions, keep existing artwork, or supply a local image. Use the returned full image URL rather than stretching a search thumbnail. Missing images and low-resolution scans are valid outcomes; a high-resolution front cover is not guaranteed. [Example release image data](https://api.discogs.com/releases/1890689)

The requested destinations are Embed in selected files, Save album cover beside files, or Both. Apply one front-cover selection to the accepted album, with optional back/booklet images assigned appropriate picture types. Preserve existing art until the user chooses its replacement. An existing `cover.jpg`/`folder.jpg` requires a specific replace choice; retries must not add duplicate embedded images. Decode and validate supported image formats and dimensions before any tag writes, and report artwork failures independently of tag failures.

Discogs-sourced persistent artwork remains an integration constraint to resolve before implementation. Its [API terms](https://support.discogs.com/hc/en-us/articles/360009334593-API-Terms-of-Use) distinguish CC0 metadata from Restricted Data images and impose use, storage and attribution conditions. An accessible image URL does not establish permission for permanent embedding. The interaction between API freshness/storage conditions and durable local metadata also needs clarification. These are open questions, not findings that all personal tagging is prohibited. Keep the requested Discogs artwork workflow in scope while resolving them; local artwork import remains available independently. Do not turn this research uncertainty into repetitive legal prompts in the player.

#### API adapter and authentication

Use `GET /database/search`, `GET /releases/{id}`, and optionally `GET /masters/{id}/versions` when browsing editions. Fetch once for the release and reuse the proposal across its mapped files. Paginate candidate/version results on demand. Keep this adapter independent of YouTube Music and file-writing code.

The research observed successful unauthenticated search/release/master requests, but that is an observation rather than a guarantee for every endpoint or future session. Support Discogs authentication when required. A personal token is an option for this personal app; OAuth 1.0a provides a browser authorization path similar to [Mp3tag's Discogs connection](https://docs.mp3tag.de/tag-sources/discogs-auth/). Store credentials in OS credential storage and send them only to the intended API origin. Request only read behavior from the adapter; do not add collection, marketplace or Discogs database writes.

Send an identifying User-Agent. Observe `X-Discogs-Ratelimit`, `X-Discogs-Ratelimit-Used` and `X-Discogs-Ratelimit-Remaining`; do not assume a fixed allowance or evade it through extra credentials. Respect valid Retry-After information when supplied and back off on 429 or transient failures. Handle authentication failure, inaccessible release IDs, offline state and missing artwork without altering files. Keep any already staged proposal visible while recovering. Cache and retain source data only under the verified data-use rules, and show the required Discogs source attribution beside the import data.

#### Safe album writes and CUE

Build a staged patch for each physical file and validate the whole accepted set before writing. Reuse the verified temporary-file/replacement strategy described for ReplayGain tags, preserve original metadata/artwork for recovery, and reject targets changed by another program since preview. Defer a playing file's replacement if its active Windows handle prevents a safe write. Track completion per file; album-level intent does not make several filesystem replacements atomic. Verify that decoded audio and unrelated metadata remain unchanged.

A single FLAC image with a CUE sheet has one physical tag/artwork container. Write album-level data and selected covers once; keep virtual-track mappings in the app model, with separately reviewed CUE/sidecar editing if supported. Do not claim distinct embedded file tags for each virtual track. A CUE referencing separate physical files can map and write those files normally.

Reimport the stored release ID, fetch a fresh proposal, and show a new diff. Never silently replace user edits through periodic background sync. A write job, its review state and any source-data retention must follow the resolved API terms.

### Collection persistence

Store tracks, local files, releases, artists, artwork, provider items, playlist entries, queue entries and scan jobs with stable internal IDs. A provider item is identified by provider and provider ID. Multiple local editions can share a release title without sharing an identity.

Keep logical track identity separate from playable sources. Link local and remote recordings only after user confirmation or a reversible suggestion with clear evidence. Retain duplicate playlist entries and stable queue-entry IDs. A mixed playlist is app-owned; exporting it to YouTube Music cannot upload local FLAC files. Report non-exportable entries.

Persist folder roots, canonical paths, volume identity, size/mtime, metadata and signatures needed to detect stale analysis. Use OS file identity where available and a signature when resolving ambiguous moves. Do not mark an unplugged root's files deleted. Reconcile watched folders after restart and after watcher overflow. Avoid following recursive symlink/junction cycles. Handle Unicode and long Windows paths.

Index readable metadata first and decode artwork lazily. Start playback without waiting for the library scan. Use FTS5 for search and virtualized grids/tables. Search local and remote sources independently, expose loading/error per source, and cancel outdated queries. Online failures must not hide local matches.

Save queue order, shuffle seed/order, repeat state, current entry and position in a transaction. Checkpoint position periodically outside the audio thread and on pause/seek/clean exit. Restore paused and explain unavailable entries. On a recoverable failed item, keep it visible and offer retry/skip; cap repeated retries. Prevent old asynchronous seeks or provider results from changing a newly selected track through generation IDs.

## Key flows and states

### Appearance

Offer Follow system, Light, and Dark in Settings > Appearance. Follow system is the proposed default. Persist the chosen mode separately from its resolved light/dark appearance. On Windows, read the app-theme preference and observe its change notifications. Resolve it before the first frame to avoid a white flash. Explicit Light/Dark ignores OS theme changes; Follow system updates all open windows and popovers while running. Theme changes must preserve playback, queue, selection, focus, scroll position and analysis jobs. Pin reference artboards to their named theme; a Light artboard must not resolve its initial state from the reviewer's OS preference.

Use semantic tokens for both themes, including selection, focus, status, separators, form controls, overlays and native-compatible title-bar colors. Keep artwork unchanged. Test contrast and long/CJK metadata in each theme. High contrast and reduced motion remain separate OS preferences rather than alternate names for Dark. The Doop canvas includes all main screens in both themes plus an Appearance page showing the three choices. The web mockup can demonstrate browser theme changes; native Windows behavior remains an implementation acceptance check.

### Collection and playback

Keep UI copy limited to labels, values, relevant consequences and recovery actions. Omit page taglines, descriptions of self-explanatory controls, repeated status prose, and assurances that changing appearance affects the app or preserves playback. Keep details such as metadata write destinations, driver-managed buffers, unavailable sources, and uncertainty in the audio signal path.

First launch offers Add music folder and Connect YouTube Music independently. Local setup does not require an account. Show indexing progress, a permission recovery action, and a useful empty state before any files exist.

The library offers All music, Local files and YouTube Music filters over one collection. Album detail emphasizes Play album, queue actions and track order, with file quality and scan status as secondary facts. A scanner preview lists groups, analysis settings, destination and outcomes. Source settings show folder health, account health and reconnect/disconnect actions.

Handle a disconnected account, expired session, regional/unavailable item, no network, removed file, empty search, failed scan, read-only tags, incompatible device format and changed output endpoint. Show the relevant recovery action. Avoid dismissing failures automatically or reporting a failed remote write as saved.

## Validation and acceptance

These are planned acceptance checks, not completed results.

1. Compare FLAC decoded integer PCM against a trusted decoder across the stated formats, silence, extrema and corrupt/truncated files. No resampler or DSP participates in the decoder comparison.
2. Capture the engine's PCM at the backend boundary for a continuous album fixture split into tracks. Assert exact concatenation, no inserted/removed frames, and sample-accurate CUE boundaries when P1 ships. Validate actual Windows playback separately on real hardware.
3. Compare loudness/gain with pinned foobar2000 and independent ReplayGain 2.0 fixtures using the same grouping, target and peak mode. Set the initial loudness tolerance to 0.1 dB, document any justified algorithm differences, and never round before analysis completes.
4. Verify album gain preserves inter-track differences, changes to membership invalidate results, protection uses one bound per album, cancellation never commits a partial album as complete, and tag writes preserve PCM and unrelated metadata.
5. Test WASAPI shared/exclusive negotiation on onboard audio, USB DAC and Bluetooth endpoints. Test ASIO with the user's FiiO K3 and installed Windows driver, including enumeration, architecture, channel mapping, sample types, rate changes, driver panel access, buffer/reset messages and reported latency. Include missing/busy drivers, incompatible formats, hot unplug, sleep/resume and Windows volume behavior. Compare signal-path values with negotiated backend values and verify no silent backend fallback.
6. Exercise queue restoration after clean exit and forced termination, repeat/shuffle semantics, mixed-source switching, stale async responses and unavailable items.
7. Use a representative 100,000-track local fixture. Proposed budgets are under 100 ms for a warm local search and under 2 seconds to a usable restored window on a documented reference PC. Report machine, dataset, cold/warm state and measurements; these are targets, not benchmark claims.
8. Test first-run/empty/error views, long Japanese/Indonesian metadata, keyboard-only use, Narrator and 100/150/200% Windows scaling. UI can remain smooth while scan workers run and audio callbacks avoid blocking.
9. The provider spike must demonstrate real authenticated playback, Premium quality behavior, seeking, expiry/reconnect, source switching and a truthful signal path. A catalog-only demo does not pass.
10. Discogs import covers a whole album, a two-disc album, alternate editions with different bonus tracks, a compilation, vinyl positions, headings/index rows, and absent durations. Verify that missing/extra tracks do not shift later mappings, unselected fields and ReplayGain survive, and a reimport preserves later manual edits until explicitly selected.
11. Exercise missing/secondary/low-resolution artwork, selected embedded and sidecar destinations, existing covers, read-only targets, concurrent Mp3tag edits, cancellation and partial failures. Verify PCM preservation, per-file recovery, duplicate-free retries and single-file/multi-file CUE behavior. API errors, 429s and auth expiry must leave files unchanged. Resolve the source-data/artwork use questions before shipping Discogs-backed durable writes.

## Delivery dependencies and unresolved choices

1. Establish the pinned GPUI Kit workspace, storage and integration contracts. Resolve provider access/data-use questions alongside implementation wherever dependencies permit.
2. Integrate local and streaming playback, queues, library management, ReplayGain/peak protection, metadata/artwork editing, CUE and Discogs through complete user-visible flows. Include all convenience and desktop integration issues in the same delivery.
3. Complete Windows/K3 ASIO validation, accessibility, failure recovery, performance, data transfer, reproducible packaging and the macOS/Linux desktop ports.
4. Close delivery only after the full acceptance matrix verifies the combined features and final installable artifacts. Keep external account, hardware and source-data blockers explicit until resolved. Public distribution remains a separate owner decision.

Propose Windows 11 x64 as the initial tested baseline, with ARM64 and Windows 10 support undecided. Exclusive-mode launch timing, multichannel audio, exact scanner compatibility target, app license and public distribution remain open.
