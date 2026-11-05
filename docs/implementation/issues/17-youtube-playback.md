## Outcome

Play remote items from the app’s queue and switch between local FLAC and YouTube Music without two backends producing sound.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)

## Can run in parallel with

- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/providers/youtube/src/playback/**`
- `apps/desktop/src/controllers/provider_playback/**`
- `tests/integration/mixed_sources/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement the proven playback adapter, seek/pause/next, expiry/retry and capability-aware metadata/quality state.
- Integrate remote queue entries, source handoff, session restoration and unavailable-item recovery.
- Publish truthful provider-owned versus app-owned PCM/output information; apply native DSP/ASIO only where the actual path supports it.

## Acceptance criteria

- [ ] Local→remote→local playback works from the same queue, and exactly one backend is audible.
- [ ] Expired/signed URLs or sessions refresh through bounded recovery without exposing credentials or restarting the wrong item.
- [ ] Queue restore, shuffle/repeat, user seeks and late async responses preserve the intended playing entry.
- [ ] Source bitrate/codec and output ownership are reported only when verified; browser output does not falsely show native ReplayGain/ASIO.
- [ ] Unsupported streaming gapless or quality is represented honestly while supported playback behavior works end to end.

## Validation and completion evidence

- Run mixed-backend race/state tests with deterministic fake transports and a real connected session.
- Record actual remote playback, source switching, expiry recovery and output ownership.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Live validation requires the account and access route proved by the access issue. Retain that dependency rather than replacing streaming with an untested stub.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
