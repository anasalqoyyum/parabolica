## Outcome

Control real playback through Windows media integration and preserve safe state across ordinary desktop lifecycle events.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)

## Can run in parallel with

- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/platform/src/windows/media/**`
- `crates/platform/src/windows/lifecycle/**`
- `apps/desktop/src/controllers/os_events/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement system media-session metadata/actions, media keys, native open/folder dialogs and appropriate notifications.
- Integrate app activation/single-instance file-open routing, sleep/resume and close/quit behavior with the queue/controller.
- Keep artwork/metadata presented to the OS consistent with the playing entry and privacy preferences.

## Acceptance criteria

- [ ] Windows play/pause/next/previous actions reach the same queue commands as the UI, including when unfocused.
- [ ] OS metadata and artwork follow local/remote playback without stale track leakage.
- [ ] Second-instance/open-with requests reach the running player safely.
- [ ] Sleep/resume and application shutdown checkpoint state without duplicate sound, secret logs or silent device switching.
- [ ] Native dialogs handle Unicode/long paths and cancellation without changing the current session.

## Validation and completion evidence

- Exercise Windows media overlay/keys, activation, dialogs and lifecycle on a real Windows build.
- Run event ordering and shutdown/checkpoint integration tests.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
