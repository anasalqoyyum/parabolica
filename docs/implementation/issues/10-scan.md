## Outcome

Select albums/tracks, review grouping, scan their audio and save complete ReplayGain analysis to the app library.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)

## Can run in parallel with

- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/replaygain/src/analyze/**`
- `apps/desktop/src/ui/replaygain/scanner/**`
- `crates/storage/migrations/0010_*.sql`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement ReplayGain 2.0 measurement to the -18 LUFS reference, track/album aggregation, sample/true-peak options and provenance.
- Implement the full scanner page, group review, job progress, cancellation/history and incomplete/failed/stale states.
- Handle multi-disc groups and explicit single-album versus tag grouping without averaging dB gains.

## Acceptance criteria

- [ ] A real album scan produces reviewed track and album gains/peaks from fully decoded audio.
- [ ] Pinned reference fixtures match the documented tolerance against foobar2000/independent RG2 measurement.
- [ ] Missing tracks and failures cannot publish an incomplete album as complete; membership changes invalidate album results.
- [ ] Cancel/resume reuses unchanged complete measurements and respects playback priority.
- [ ] Analysis remains non-destructive and saved results/provenance survive restart.

## Validation and completion evidence

- Run loudness, gating/aggregation, silence/non-finite and cancellation fixtures.
- Perform select/group/scan/review/save/reopen through the native app.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
