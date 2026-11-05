## Outcome

Make the streaming side usable for daily listening through real discovery and playlist/account actions.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)

## Can run in parallel with

- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `apps/desktop/src/ui/discovery/**`
- `crates/providers/youtube/src/actions/**`
- `apps/desktop/src/ui/playlists/remote/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement Home/Explore, artist/album pages, recommendations/radio where supported, saved items and provider likes.
- Implement app-owned mixed playlists and explicit supported remote playlist/like writes with source attribution and per-action errors.
- Explain non-exportable local entries when exporting to a remote playlist; never silently upload or replace their recordings.

## Acceptance criteria

- [ ] Discovery and radio results can be queued and played through the connected provider.
- [ ] Local app likes and provider likes are distinct, and successful/failed remote writes are reported truthfully.
- [ ] Remote playlist create/edit/reorder and app-owned mixed playlist behavior are complete for proven provider capabilities.
- [ ] Offline, regional, permission and unavailable states have actionable recovery; capability restrictions never look like successful saves.
- [ ] No Home/Explore/playlist control is left as a decorative placeholder.

## Validation and completion evidence

- Run provider action contract tests and native read/write UI flows with an explicitly authorized test playlist.
- Verify remote mutations by reading them back and preserve local-only entries.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Remote write tests require an owner-approved disposable playlist/account action. Never mutate unrelated user playlists during tests.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
