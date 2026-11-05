## Outcome

Edit a whole album’s fields and artwork from Files & tags with a reviewed, recoverable per-file write job.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)

## Can run in parallel with

- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/tags/**`
- `apps/desktop/src/ui/tags/**`
- `crates/storage/migrations/0012_*.sql`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement current/proposed tag patches, shared album fields and individual track overrides with replace/fill-empty/keep choices.
- Implement local front/back/booklet art selection, embedded/sidecar/both destinations and explicit existing-cover replacement.
- Implement temporary sibling replacement, file-change detection, backups/recovery and partial write/retry state.

## Acceptance criteria

- [ ] User-selected album and per-track changes reach the intended files and are reflected after library refresh.
- [ ] Unselected/unknown tags, pictures, ReplayGain and decoded PCM remain unchanged.
- [ ] Artwork validation, existing sidecar conflicts and duplicate-free retries work without changing files before review.
- [ ] Read-only/disk-full/interrupted/concurrently edited/actively playing files produce recoverable per-file outcomes.
- [ ] The reusable tag-patch/write contract supports ReplayGain and Discogs consumers without separate unsafe writers.

## Validation and completion evidence

- Compare decoded PCM and untouched metadata before/after writes; inject failures around replacement.
- Exercise album field/art review, apply, partial retry and recovery through the UI.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
