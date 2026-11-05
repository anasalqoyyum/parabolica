## Outcome

Run the full agreed player on a documented Linux desktop baseline with working audio, credentials and packaging.

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

- [#33 Deliver the complete macOS desktop port](https://github.com/anasalqoyyum/parabolica/issues/33)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/platform/src/linux/**`
- `crates/audio/src/output/linux/**`
- `packaging/linux/**`
- `.github/workflows/package-linux.yml`
- `docs/SETUP_LINUX.md`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement the selected Linux audio/session backend, device/rate handling, MPRIS/media controls, dialogs, credential integration and theme observation.
- Verify GPUI Kit dependencies and Wayland/X11 behavior, keeping the portable feature set and truthful output inspector.
- Produce an installable Linux artifact and setup guidance for a concrete supported distribution rather than claiming every distribution.

## Acceptance criteria

- [ ] The target Linux desktop completes local/remote playback, queue, ReplayGain, CUE, metadata/artwork, Discogs and account flows.
- [ ] The output inspector distinguishes app negotiation from server/device processing and reports unavailable information honestly.
- [ ] Unavailable keyring/session services have an explicit secure session-only/reconnect behavior rather than plaintext credential fallback.
- [ ] Windowing, keyboard/IME/assistive behavior, media controls and sleep/device-loss recovery are checked on the supported environment.
- [ ] A clean supported Linux installation can install/run the artifact, with source SHA, dependencies and checksums documented.

## Validation and completion evidence

- Run portable tests plus native Linux audio/UI/session checks under the declared display backend(s).
- Verify package contents and installation on the documented distribution.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Requires a graphical supported Linux environment with an audio device/session service; a headless compile alone does not validate playback or desktop integration.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
