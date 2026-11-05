## Outcome

Supply real, normalized edition metadata and artwork proposals for an album through the supported Discogs API path.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)

## Can run in parallel with

- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/discogs/**`
- `tests/fixtures/discogs/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement search, release URL/ID lookup, master-version browsing, pagination, user agent and conditional auth.
- Normalize artists/ANVs/joins, release fields, track/index/heading structures, positions, partial dates, genre/style arrays and image metadata without losing source detail.
- Implement rate-header handling, bounded retry, permitted retention, source attribution and image decode/size validation.

## Acceptance criteria

- [ ] Live queries and pinned redacted fixtures return edition-level proposals with actual track/image fields.
- [ ] Search display titles and master tracklists do not overwrite release data.
- [ ] Missing durations/images, multi-disc/vinyl positions, optional nested grouping and multiple labels/credits preserve meaning.
- [ ] 401/403/404/429/network failures are typed recoverable outcomes and do not trigger per-track API storms.
- [ ] Artwork uses returned URLs and declared dimensions without assuming high resolution or authorization transfer to an unrelated CDN.

## Validation and completion evidence

- Run normalization/auth/rate-limit/pagination/error fixtures.
- Verify live search→release→image metadata retrieval under the resolved access model.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
