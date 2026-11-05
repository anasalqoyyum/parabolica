## Outcome

Run the full agreed player on macOS using native desktop/audio/account integration and produce a usable application bundle.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [#31 Deliver backup, export/import and library relocation](https://github.com/anasalqoyyum/parabolica/issues/31)

## Can run in parallel with

- [#34 Deliver the complete Linux desktop port](https://github.com/anasalqoyyum/parabolica/issues/34)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/platform/src/macos/**`
- `crates/audio/src/output/coreaudio/**`
- `packaging/macos/**`
- `.github/workflows/package-macos.yml`
- `docs/SETUP_MACOS.md`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement CoreAudio device/rate/format handling, media controls, dialogs, credential storage, theme observation and native lifecycle.
- Adapt the existing GPUI Kit shell while keeping all portable library, queue, gain, CUE, tagging, Discogs and streaming workflows.
- Provide a reproducible bundle/package and document the tested macOS/architecture baseline; ASIO remains a Windows-specific backend.

## Acceptance criteria

- [ ] The macOS app completes local/remote playback, queue, ReplayGain, metadata/artwork, Discogs, CUE and account flows.
- [ ] The signal path reports actual CoreAudio negotiation and hardware uncertainty.
- [ ] Appearance/keyboard/IME/assistive behavior and sleep/device-loss recovery are verified on the target OS.
- [ ] A clean supported Mac can install/run the produced bundle, with accurate signing/notarization status.
- [ ] The port is a completed deliverable in this issue set rather than an unimplemented platform abstraction.

## Validation and completion evidence

- Run portable tests plus real macOS audio/UI/lifecycle/VoiceOver checks.
- Validate bundle installation and source-commit/checksum evidence.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Requires a supported macOS runner and actual platform/audio validation. Distribution signing/notarization requires owner credentials if requested; label unsigned private artifacts accurately.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
