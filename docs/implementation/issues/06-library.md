## Outcome

Add music folders and browse/search/play the indexed collection from the app without waiting for a full scan.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)

## Can run in parallel with

- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/library/**`
- `crates/storage/migrations/0006_*.sql`
- `apps/desktop/src/ui/library/**`
- `apps/desktop/src/ui/sources/folders.rs`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement incremental metadata-first indexing, debounced watchers, reconciliation, cancellation and bounded artwork thumbnails.
- Implement Albums, Artists, Songs and folder views, album detail, sorting/filtering, compilation/multi-disc identity and explicit album grouping overrides.
- Implement add/remove/relocate folder and missing-file recovery with stable file/album identities.

## Acceptance criteria

- [ ] First-run Add folder indexes valid files, shows progress, and lets the user play an album from real results.
- [ ] FTS search, album/artist/song browsing, metadata and artwork use the stored library and virtualized views.
- [ ] Unplugging a drive does not delete its collection; reconnect/move recovery restores availability.
- [ ] Unicode/long Windows paths, junction cycles, watcher overflow, corrupt metadata and huge artwork are handled.
- [ ] Distinct editions and compilations remain distinct; rescans preserve queue, ratings and source identity.

## Validation and completion evidence

- Run filesystem/index/search integration fixtures including removable roots and reconciliation.
- Exercise add/search/browse/play/relocate through the native UI.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
