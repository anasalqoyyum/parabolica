## Outcome

Operate every delivered workflow with the keyboard and Windows assistive technology at supported scaling and text lengths.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)
- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [#31 Deliver backup, export/import and library relocation](https://github.com/anasalqoyyum/parabolica/issues/31)

## Can run in parallel with

- [#24 Verify and fix cross-feature recovery and data integrity](https://github.com/anasalqoyyum/parabolica/issues/24)
- [#26 Validate large-library performance and real-time audio reliability](https://github.com/anasalqoyyum/parabolica/issues/26)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `tests/ui/accessibility/**`
- `docs/validation/accessibility.md`
- `bounded UI corrections coordinated with page owners`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Exercise full navigation/focus restoration, lists/tables, dialogs, menus, sliders, theme changes and asynchronous status/error announcements.
- Test Narrator/UIA, CJK/Indonesian metadata and IME, 100/150/200% scaling, reduced motion and high contrast.
- Fix inaccessible custom views and layout/text failures in the integrated application.

## Acceptance criteria

- [ ] A keyboard-only user can import/search/play/queue/scan/edit tags/sync Discogs/connect providers/change output and appearance/export data.
- [ ] Narrator exposes correct names, values, roles and actionable state, including errors and live job results.
- [ ] Long and Japanese/Indonesian content remains usable without clipped controls or lost focus.
- [ ] Light/Dark/Follow system works under a dark OS preference; artwork colors and current playback survive theme changes.
- [ ] Headless semantics and screenshots are supplemented by real Windows screen-reader operation.

## Validation and completion evidence

- Run the complete interaction matrix and relevant GPUI Kit integration tests.
- Record Windows, scaling, build and assistive technology for each actual observation.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
