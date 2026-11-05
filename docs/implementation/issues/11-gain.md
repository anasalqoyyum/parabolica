## Outcome

Use scanned or existing file ReplayGain during real playback, preserve album dynamics, and write interoperable gain tags when requested.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)

## Can run in parallel with

- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/audio/src/dsp/replaygain/**`
- `crates/replaygain/src/tag_mapping.rs`
- `apps/desktop/src/ui/replaygain/playback/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement Off/Track/Album/Auto, preamp, effective-context selection and smooth explicit gain changes.
- Read/write conventional FLAC ReplayGain fields through the safe tag service; keep app/file analysis sources explicit.
- Implement album-wide peak-aware attenuation, optional true-peak protection and requested-versus-applied gain reporting.

## Acceptance criteria

- [ ] Album mode applies one gain and one peak-protection bound across the album, preserving inter-track loudness differences.
- [ ] Mixed/shuffled Auto context uses track mode; missing/stale values have the documented visible behavior.
- [ ] Gain/preamp/volume and peak protection produce verified output samples without introducing clipping from the selected gain path.
- [ ] Existing file tags and app analysis remain distinguishable, and explicit tag writes round-trip with foobar2000.
- [ ] Gain changes are reflected in actual audio and output-state snapshots; metadata changes alone do not silently enable a playback mode.

## Validation and completion evidence

- Run DSP/output capture and sample/true-peak edge-case checks.
- Verify native playback modes, scan-to-playback flow and file-tag round-trips.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
