## Outcome

Back up and move the collection’s app-owned state, restore it safely, and relocate local media roots without losing identities.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)

## Can run in parallel with

- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/storage/src/backup/**`
- `apps/desktop/src/ui/data_transfer/**`
- `crates/library/src/relocation/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement versioned export/import for preferences, playlists/queue, ratings/history, folder roots and allowed analysis/provenance.
- Exclude credentials and disallowed provider caches; distinguish app-state backup from copying a user’s music files.
- Implement validation, collision/merge review, root remapping, restore recovery and corrupted/incompatible archive handling.

## Acceptance criteria

- [ ] Export→fresh install→import restores the supported state with explicit path remapping and unavailable items.
- [ ] Restored identities preserve queue links, album analyses and metadata provenance when the referenced audio is unchanged.
- [ ] Secrets, signed URLs and unsupported retained provider content do not enter the export.
- [ ] Partial/corrupt/incompatible imports preserve the existing valid database and provide recovery.
- [ ] A moved media root can be repaired through the UI without rebuilding user ratings/history.

## Validation and completion evidence

- Run round-trip, migration, collision, tamper/corruption and interruption tests.
- Exercise backup/restore and root relocation on a clean packaged installation.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
