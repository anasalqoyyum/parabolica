## Outcome

Deliver the complete player described in PRODUCT.md, docs/TECH_SPEC.md and DESIGN.md, with every implementation issue verified and an installable artifact. This is the delivery tracker; execution order does not defer features to another milestone.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#35 Validate the complete app and produce the final delivery evidence](https://github.com/anasalqoyyum/parabolica/issues/35)

## Can run in parallel with

No independent pairing is scheduled. Follow the blockers above.

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `docs/implementation/README.md`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Keep the linked task checklist, dependency state and delivery evidence current.
- Keep external account, driver, source-data and hardware requirements visible until they are actually resolved.

## Acceptance criteria

- [ ] Every child task is complete against its own acceptance criteria.
- [ ] Final acceptance covers the complete Windows player, the FiiO K3 ASIO path, YouTube Music playback, Discogs album tagging/artwork, and the macOS/Linux desktop ports.
- [ ] A user can obtain the private build artifacts, install/run the app and complete the documented main workflows.
- [ ] Open provider or hardware blockers keep this tracker open; a successful mock or a follow-up milestone does not satisfy a required behavior.

## Validation and completion evidence

- Review the final acceptance report and the final artifact commit/checksums.
- Verify every linked task and source requirement has completion evidence.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## Complete delivery checklist

- [ ] [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)
- [ ] [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [ ] [#4 Implement durable app storage, migrations and job state](https://github.com/anasalqoyyum/parabolica/issues/4)
- [ ] [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)
- [ ] [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [ ] [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [ ] [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)
- [ ] [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)
- [ ] [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [ ] [#11 Implement ReplayGain playback, interoperability and peak protection](https://github.com/anasalqoyyum/parabolica/issues/11)
- [ ] [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [ ] [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [ ] [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)
- [ ] [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [ ] [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [ ] [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [ ] [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [ ] [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)
- [ ] [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [ ] [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [ ] [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [ ] [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [ ] [#24 Verify and fix cross-feature recovery and data integrity](https://github.com/anasalqoyyum/parabolica/issues/24)
- [ ] [#25 Complete keyboard, Narrator, DPI and localization acceptance](https://github.com/anasalqoyyum/parabolica/issues/25)
- [ ] [#26 Validate large-library performance and real-time audio reliability](https://github.com/anasalqoyyum/parabolica/issues/26)
- [ ] [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [ ] [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [ ] [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [ ] [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [ ] [#31 Deliver backup, export/import and library relocation](https://github.com/anasalqoyyum/parabolica/issues/31)
- [ ] [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)
- [ ] [#33 Deliver the complete macOS desktop port](https://github.com/anasalqoyyum/parabolica/issues/33)
- [ ] [#34 Deliver the complete Linux desktop port](https://github.com/anasalqoyyum/parabolica/issues/34)
- [ ] [#35 Validate the complete app and produce the final delivery evidence](https://github.com/anasalqoyyum/parabolica/issues/35)

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
