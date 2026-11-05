## Outcome

Choose a Windows endpoint and use WASAPI Shared or Exclusive from Listening device settings.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)

## Can run in parallel with

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/audio/src/output/wasapi_exclusive.rs`
- `apps/desktop/src/ui/listening_device/**`
- `crates/platform/src/windows/audio_devices.rs`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement backend/device enumeration, stable selection, supported formats, source-following/fixed-rate policy and negotiation.
- Connect Listening device settings to the actual output controller and publish actual stream state.
- Implement pause-on-disconnect and explicit recovery rather than silently switching to speakers or a different backend.

## Acceptance criteria

- [ ] Shared/Exclusive and endpoint choices affect real playback, survive restart and reflect negotiated formats.
- [ ] A busy exclusive endpoint or unsupported format reports the failure and offers an explicit alternative.
- [ ] Source-rate changes and compatible gapless transitions follow the documented policy.
- [ ] Disconnect/reconnect/sleep handling re-queries capabilities and preserves safe playback state.
- [ ] Selection and state interfaces are ready for ASIO without treating an ASIO driver as a WASAPI endpoint.

## Validation and completion evidence

- Run format negotiation/device-loss tests and actual shared/exclusive playback on Windows.
- Record native UI and endpoint evidence, including an incompatible/busy case.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
