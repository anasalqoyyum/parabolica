## Outcome

Manage a reliable playback queue and app-owned playlists, and restore the listening session after restart.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)

## Can run in parallel with

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/domain/src/queue/**`
- `crates/storage/migrations/0007_*.sql`
- `apps/desktop/src/controllers/queue/**`
- `apps/desktop/src/ui/queue/**`
- `apps/desktop/src/ui/playlists/**`
- `crates/audio/src/transitions/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement play now/next/later, duplicate entries with stable IDs, reorder/remove, history, shuffle and repeat off/all/one.
- Implement local app playlists/likes and M3U8 import/export with explicit unavailable entries.
- Persist queue order, shuffle order/seed, position and repeat state; restore paused.
- Implement next-track prefetch and continuous local output across compatible formats, honoring decoder delay/padding without boundary fades. Reconfigure explicitly for incompatible rates/layouts.

## Acceptance criteria

- [ ] User-visible queue and playlist actions control real playback and maintain deterministic next/previous behavior.
- [ ] Reorder/removal of the playing entry, repeat-one/all and shuffle transitions have defined tested behavior.
- [ ] Clean exit and forced termination restore the most recent valid checkpoint paused, without duplicating entries.
- [ ] Unavailable tracks stay visible with retry/skip, and retry loops are bounded.
- [ ] The source-aware queue contract supports later provider entries without replacing local identity.
- [ ] A continuous album split across files produces exactly concatenated PCM at the backend boundary, with no inserted or dropped frames, and plays gaplessly on the actual Windows backend. Repeat and compatible shuffled transitions use the same engine path.

## Validation and completion evidence

- Run command/state/property-style tests and crash/restart scenarios.
- Exercise queue/playlist imports and actual playback transitions in the app.
- Capture split-album boundary PCM and compare against the unsplit reference; test short files, buffer pressure, delay/padding and format-change boundaries.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
