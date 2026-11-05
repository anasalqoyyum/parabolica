## Outcome

Open a local FLAC file in the actual app and play, pause, seek and adjust volume through WASAPI Shared with truthful playback state.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)
- [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [#4 Implement durable app storage, migrations and job state](https://github.com/anasalqoyyum/parabolica/issues/4)

## Can run in parallel with

- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)
- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/audio/src/decode/**`
- `crates/audio/src/engine/**`
- `crates/audio/src/output/wasapi_shared.rs`
- `apps/desktop/src/ui/player/**`
- `apps/desktop/src/commands/playback.rs`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement Symphonia FLAC decoding, bounded PCM buffering, real-time-safe output consumption and controller snapshots.
- Connect native file-open and player actions to real audio output, including duration, seek position and errors.
- Expose negotiated output state and processing facts for the later inspector; default EQ/crossfade/effects off.
- After exact FLAC decoding passes, enable and verify local MP3, PCM WAV and AAC/M4A container/codec combinations. Document enabled decoders; validate Opus with an additional decoder before claiming support.

## Acceptance criteria

- [ ] Decode the stated 16/24-bit and 44.1–192 kHz fixtures with PCM equality against a trusted decoder.
- [ ] Native play/pause/seek/volume works and stale seeks cannot affect a newly selected file.
- [ ] Shared output negotiates the endpoint format and performs a documented single resampling/conversion stage when required.
- [ ] Audio callbacks perform no allocation, blocking I/O, locks or logging; underrun information is collected safely.
- [ ] Corrupt/unsupported files and unavailable devices fail visibly without freezing the shell or altering source files.
- [ ] MP3, PCM WAV and supported AAC/M4A fixtures play and seek in the native player; unsupported combinations report a specific error. Decoder delay/padding is exposed to the queue transition contract.

## Validation and completion evidence

- Run exact decoder comparisons, controller race checks and backend-boundary capture tests.
- Record actual Windows endpoint playback and negotiated format evidence.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
