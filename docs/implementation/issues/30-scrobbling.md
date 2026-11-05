## Outcome

Optionally send eligible local and streaming listening events to a supported scrobbling service without duplicates or credential leaks.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)

## Can run in parallel with

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/scrobbling/**`
- `apps/desktop/src/ui/accounts/scrobbling.rs`
- `crates/storage/migrations/0030_*.sql`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Select and document a supported service such as Last.fm, verify its current eligibility/auth requirements, and implement the real account flow.
- Consume stable playback events, handle now-playing/eligible completion, seeks, replay, timer stops and source switches.
- Persist a bounded retry/outbox for offline operation and provide disconnect/data controls.

## Acceptance criteria

- [ ] An explicitly connected test account receives eligible local and streaming plays exactly once.
- [ ] Short plays, seeks, repeated events, offline retries and app restart follow the service’s current rules.
- [ ] Disconnect prevents new sends and removes local credentials; stored jobs contain only necessary non-secret data.
- [ ] UI distinguishes pending, failed and delivered state without claiming success for an unsent event.

## Validation and completion evidence

- Run eligibility/deduplication/outbox tests with a fake service.
- Verify real authorized test submissions by reading account history.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Requires an owner-authorized scrobbling test account/API credentials and permission for test submissions. The feature is opt-in for users, but its implementation is part of this delivery.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
