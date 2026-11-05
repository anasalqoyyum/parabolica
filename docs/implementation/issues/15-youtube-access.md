## Outcome

Prove a viable personal-account integration with real authenticated audio playback before downstream UI work depends on it.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)

## Can run in parallel with

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)
- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/youtube/src/auth/**`
- `crates/providers/youtube/src/transport/**`
- `docs/research/YOUTUBE_MUSIC.md`
- `tests/provider-live/youtube/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Evaluate current account/login, entitlement, stream or browser-backed playback paths, GPUI integration and applicable service constraints.
- Choose and implement a reusable auth/transport path with a real playback proof, account expiry/reconnect, seeking and observed quality/capabilities.
- Document the supported capability matrix and keep unmet requirements explicit if access cannot be established.

## Acceptance criteria

- [ ] A real entitled account plays a chosen YouTube Music item through the selected integration; catalog-only JSON and external-browser shortcuts do not pass.
- [ ] Login uses a supported/user-approved handoff and never collects a Google password in native UI.
- [ ] Actual seek, expiry, reconnect, background behavior and stream/browser output inspection are recorded.
- [ ] The selected path has reusable provider interfaces for catalog and queue agents, with no shared provider-specific application credential.
- [ ] An unsupported required capability remains a blocker and is escalated for a product decision; it is not closed as a future milestone or silently substituted.

## Validation and completion evidence

- Run live account/entitlement/seek/expiry checks and saved redacted protocol fixtures.
- Record what the app can actually control or inspect, including audio output ownership.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Requires the owner’s YouTube Music account, appropriate entitlement and interactive authorization. Service access/policies may require a product decision; an AI agent cannot manufacture access or certify an unsupported playback route.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
