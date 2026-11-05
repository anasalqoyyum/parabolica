## Outcome

Read synchronized or unsynchronized lyrics for the current track without relying on an unauthorized lyrics scrape.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)

## Can run in parallel with

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/library/src/lyrics/**`
- `apps/desktop/src/ui/lyrics/**`
- `crates/providers/youtube/src/lyrics.rs`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Support supported embedded text and local LRC sidecars, with actual playback-time synchronization and manual offset where needed.
- Use provider lyrics only when the proven API/access route permits them; represent unavailable results honestly.
- Implement discovery/selection, loading/error/empty states and both theme layouts without blocking playback.

## Acceptance criteria

- [ ] Local lyrics display correctly and synchronized lines follow seek/pause/track changes.
- [ ] Late responses cannot put one track’s lyrics over another.
- [ ] Unicode, malformed timestamps, missing sidecars and long text have tested behavior.
- [ ] Provider lyrics either work through a documented permitted path or show unavailable while user-supplied local lyrics still work.
- [ ] No unauthorized third-party scrape or invented lyric content is used to claim completion.

## Validation and completion evidence

- Run LRC/parser/time-sync tests and native playback/seek scenarios.
- Verify any enabled remote source against its actual access/usage contract.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
