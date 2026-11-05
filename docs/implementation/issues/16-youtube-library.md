## Outcome

Browse and search local files and the real connected YouTube Music library in one app while preserving source identities.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)

## Can run in parallel with

- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/youtube/src/catalog/**`
- `apps/desktop/src/ui/library/remote/**`
- `crates/storage/migrations/0016_*.sql`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement catalog search, albums/artists/releases, saved library and paginated incremental sync from the proven transport.
- Integrate All music/Local/YouTube filters, federated search cancellation, artwork and independent per-source errors.
- Implement explicit reversible local/remote recording links, retaining distinct editions and provider IDs.

## Acceptance criteria

- [ ] Connected account results populate actual library and search views, and source filtering works.
- [ ] Remote failure/signed-out state never hides local matches or blocks local playback.
- [ ] Different masters/editions and duplicate sources are not silently merged or substituted.
- [ ] Pagination, rate limiting, stale responses, deleted/unavailable/region-limited items and sync recovery have visible tested behavior.
- [ ] Selected remote items dispatch through the established provider playback/queue contract.

## Validation and completion evidence

- Run catalog fixture/contract tests and live read-only account checks.
- Exercise mixed-source search/browse/filter/reconnect in the app.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
