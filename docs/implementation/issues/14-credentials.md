## Outcome

Provide a secure, capability-aware account/service foundation for YouTube Music, Discogs and scrobbling.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)
- [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [#4 Implement durable app storage, migrations and job state](https://github.com/anasalqoyyum/parabolica/issues/4)

## Can run in parallel with

- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)
- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/contracts/**`
- `crates/platform/src/credentials/**`
- `apps/desktop/src/ui/accounts/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement provider IDs/capabilities, auth expiry, scoped errors, cancellable generation-tagged requests and account lifecycle events.
- Implement OS credential storage and browser-based auth handoff helpers, with reconnect/disconnect UI integration points.
- Keep credentials, cookies and signed media URLs out of logs, database exports and fixtures.

## Acceptance criteria

- [ ] Provider contract tests cover ready/signed-out/expired/offline/restricted states without misreporting unavailable capabilities.
- [ ] Windows credentials persist securely across restarts, and sign-out revokes/removes local secrets and stops that provider session.
- [ ] Auth callbacks are validated and stale results cannot reconnect the wrong account.
- [ ] No provider relies on a shared personal token baked into the binary.
- [ ] Downstream providers can plug in real auth flows without changing the domain/queue contracts.

## Validation and completion evidence

- Run credential round-trip, redaction, callback and cancellation tests using disposable secrets.
- Verify the native reconnect/disconnect states and OS-backed storage on Windows.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
