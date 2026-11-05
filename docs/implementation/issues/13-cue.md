## Outcome

Import and play full-album FLAC/CUE rips as correctly ordered virtual tracks, with accurate gaps and metadata behavior.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)

## Can run in parallel with

- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/cue/**`
- `crates/library/src/cue_adapter.rs`
- `crates/audio/src/cue_ranges.rs`
- `apps/desktop/src/ui/cue/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Parse external single/multi-file CUE sheets, indexes/pregaps, disc/track metadata and supported encodings.
- Integrate virtual file ranges with library identities, queue/seek/gapless, ReplayGain analysis and Files & tags.
- Distinguish one physical tag/artwork container from virtual-track metadata and any separately reviewed sidecar/CUE edits.

## Acceptance criteria

- [ ] A full-album image appears as tracks and plays/seeks at sample-accurate boundaries without double-counting audio.
- [ ] INDEX 00/pregap and multi-file ordering have documented behavior and reference fixtures.
- [ ] Track/album gain and peak analysis use the correct virtual ranges and preserve the album context.
- [ ] Album art/tags write once to a single image; the UI does not claim separate physical tags per virtual track.
- [ ] Moved/missing images, malformed CUE data and edits preserve stable identities or show explicit recovery.

## Validation and completion evidence

- Run sample-boundary, pregap, multi-disc/multi-file and corrupt-CUE tests.
- Complete import→queue→seek→scan→tag/art review in the native app.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
