## Outcome

Prove that every required feature works together in installable builds and close the delivery only with reproducible end-to-end evidence.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#24 Verify and fix cross-feature recovery and data integrity](https://github.com/anasalqoyyum/parabolica/issues/24)
- [#25 Complete keyboard, Narrator, DPI and localization acceptance](https://github.com/anasalqoyyum/parabolica/issues/25)
- [#26 Validate large-library performance and real-time audio reliability](https://github.com/anasalqoyyum/parabolica/issues/26)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)
- [#33 Deliver the complete macOS desktop port](https://github.com/anasalqoyyum/parabolica/issues/33)
- [#34 Deliver the complete Linux desktop port](https://github.com/anasalqoyyum/parabolica/issues/34)

## Can run in parallel with

No independent pairing is scheduled. Follow the blockers above.

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `tests/end_to_end/**`
- `docs/validation/ACCEPTANCE.md`
- `docs/implementation/README.md`
- `final packaged build verification`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Build a complete requirement-to-test/artifact matrix covering PRODUCT.md, TECH_SPEC.md, DESIGN.md and every child issue.
- Run integrated workflows from clean installs and existing-data upgrades on Windows, macOS and Linux.
- Produce final private review artifacts from the accepted source commit and fix integration defects discovered by the matrix.

## Acceptance criteria

- [ ] Windows evidence includes actual FiiO K3 ASIO, WASAPI Shared/Exclusive, decoder correctness, gapless, queue restore and gain/peak behavior.
- [ ] Real account evidence includes YouTube Music search/library/playback/source switching and permitted playlist actions, plus Discogs whole-album tags and artwork.
- [ ] CUE, metadata preservation, backup/restore, smart collections, lyrics, sleep timer, scrobbling, accessibility and failure recovery are exercised.
- [ ] macOS/Linux packages complete the supported-platform matrix, with genuine native evidence and accurate capability differences.
- [ ] Artifacts install/run, identify the final SHA and checksums, and contain required notices without private data or reference art.
- [ ] No accepted feature is closed through a mock, deferred integration task, waived hardware assertion or unspecified next milestone.

## Validation and completion evidence

- Run the complete native E2E/acceptance matrix and all declared automated checks.
- Record unresolved external requirements as blockers and keep this issue/delivery tracker open until resolved.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Needs authorized real account actions, the FiiO K3 Windows setup and supported macOS/Linux environments. Clearly separate automated, observed hardware and owner-supplied evidence.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
