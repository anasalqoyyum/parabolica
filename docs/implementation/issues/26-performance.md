## Outcome

Keep the complete player responsive with a 100,000-track library and background work while audio remains reliable.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#31 Deliver backup, export/import and library relocation](https://github.com/anasalqoyyum/parabolica/issues/31)

## Can run in parallel with

- [#24 Verify and fix cross-feature recovery and data integrity](https://github.com/anasalqoyyum/parabolica/issues/24)
- [#25 Complete keyboard, Narrator, DPI and localization acceptance](https://github.com/anasalqoyyum/parabolica/issues/25)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `tests/performance/**`
- `scripts/benchmark_*`
- `docs/validation/performance.md`
- `bounded fixes coordinated with owning modules`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Create reproducible synthetic/local fixtures and document hardware, dataset, cold/warm state and measurement procedure.
- Measure startup, search, virtualized browsing, artwork cache, indexing/scan/tag background jobs, memory bounds and output underruns.
- Fix measured regressions without weakening decoder/gapless correctness or inventing unsupported performance claims.

## Acceptance criteria

- [ ] Measure the spec targets for a usable restored window and warm search on a documented reference machine.
- [ ] Indexing, scanning, artwork and tag jobs remain bounded and yield to actual playback.
- [ ] A sustained WASAPI/ASIO playback workload has documented callback/underrun evidence, including rate changes and source switches.
- [ ] The benchmark records before/after data for fixes and distinguishes statistical noise from improvement.
- [ ] Out-of-budget behavior has a concrete fix or explicit owner decision; it is not hidden by a demo-only dataset.

## Validation and completion evidence

- Run targeted benchmarks with the benchmark-checklist skill and the stated corpus.
- Pair timing data with native interaction and audio-boundary correctness checks.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
