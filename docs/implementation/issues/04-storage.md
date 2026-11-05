## Outcome

Persist the library, queue, preferences, source links, analysis and write-job state in SQLite with recoverable migrations.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)

## Can run in parallel with

- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/storage/**`
- `crates/storage/migrations/0004_*.sql`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement the database owner, typed repositories, transactions, versioned migrations, FTS setup and error reporting.
- Support atomic snapshots/checkpoints for queue/preferences and resumable scan/tag/import jobs, with schema extension rules for feature agents.
- Store source provenance and file identity separately from display tags; keep secrets out of this database.

## Acceptance criteria

- [ ] Create/open/reopen/migrate a database without losing valid library or queue data.
- [ ] Interrupted transactions and concurrent worker events preserve invariants; schema changes have deterministic ownership.
- [ ] Corrupt/incompatible databases produce recovery/export options instead of being silently overwritten.
- [ ] Provider credentials are absent from database exports and logs.
- [ ] All downstream features can use the storage contract without direct ad hoc UI-thread SQL.

## Validation and completion evidence

- Run migration, rollback/fault-injection and concurrent command tests with temporary databases.
- Inspect persisted data and integrity after forced termination.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
