## Outcome

Organize the unified collection through ratings, play history and saved rule-based playlists that play real results.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)

## Can run in parallel with

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/library/src/collections/**`
- `crates/storage/migrations/0027_*.sql`
- `apps/desktop/src/ui/collections/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement user ratings/play counts/history and deterministic smart-playlist rule evaluation over supported local and provider fields.
- Implement create/edit/delete/preview rules and queue/play the resulting collection with explicit source availability.
- Preserve ratings/history through rescans, retagging, source links and album identity changes.

## Acceptance criteria

- [ ] Users can create a smart playlist, inspect its real members, edit rules and play its results.
- [ ] Ratings/history are stored once per documented identity and do not duplicate on reconnect or retry.
- [ ] Unavailable remote/offline local entries are represented consistently with queue behavior.
- [ ] Rules, sorting, empty results and malformed saved filters have defined recoverable outcomes.
- [ ] This is delivered functionality in the same issue set, not a postponed convenience milestone.

## Validation and completion evidence

- Run rule/identity/persistence tests and native create→preview→play→restart flows.
- Check a mixed local/provider collection and large-rule result virtualization.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
