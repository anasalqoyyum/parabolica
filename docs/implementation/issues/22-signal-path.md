## Outcome

Explain the actual current playback path from source through app processing to negotiated output, with explicit uncertainty.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)
- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)

## Can run in parallel with

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `apps/desktop/src/ui/signal_path/**`
- `crates/audio/src/inspection/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement the player-bar summary, expanded inspector, copy-path action and Listening device navigation.
- Consume active snapshots for source, decoded PCM, gain/preamp/protection, resampling, EQ/crossfade/volume, conversion, backend, channels/rate/format and device/driver buffer.
- Implement appropriate shared/exclusive/ASIO and provider-owned output stages in both themes.

## Acceptance criteria

- [ ] Changing track, backend, rate, gain or endpoint updates the inspector from actual controller/driver state.
- [ ] A 24/96 source through 48 kHz shared output shows that conversion; a source-rate ASIO path shows bypassed resampling and Windows mixer/session-volume scope.
- [ ] Buffer duration is not mislabeled latency, and hardware DSP or uninspectable provider output is marked unverified/unavailable.
- [ ] ReplayGain/volume processing prevents an unsupported bit-perfect claim.
- [ ] The inspector is keyboard reachable, closes with Escape and restores focus; copy-path text matches the visible stages.

## Validation and completion evidence

- Compare snapshots and UI against actual negotiated backend values for WASAPI and K3 ASIO.
- Verify provider-managed states and dark/light screenshots with no inferred device format.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
