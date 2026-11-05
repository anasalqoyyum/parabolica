## Outcome

Build an installable Windows player from a clean checkout and verify install, upgrade, launch and uninstall.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)
- [#9 Implement FiiO K3 ASIO playback end to end](https://github.com/anasalqoyyum/parabolica/issues/9)

## Can run in parallel with

- [#6 Implement folder indexing and the local collection UI](https://github.com/anasalqoyyum/parabolica/issues/6)
- [#7 Implement queues, playlists, history and session restoration](https://github.com/anasalqoyyum/parabolica/issues/7)
- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#16 Integrate YouTube Music catalog, accounts and unified library search](https://github.com/anasalqoyyum/parabolica/issues/16)
- [#17 Integrate streaming playback with mixed queues and truthful output state](https://github.com/anasalqoyyum/parabolica/issues/17)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [#21 Deliver whole-album Discogs tag and artwork synchronization](https://github.com/anasalqoyyum/parabolica/issues/21)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#24 Verify and fix cross-feature recovery and data integrity](https://github.com/anasalqoyyum/parabolica/issues/24)
- [#25 Complete keyboard, Narrator, DPI and localization acceptance](https://github.com/anasalqoyyum/parabolica/issues/25)
- [#26 Validate large-library performance and real-time audio reliability](https://github.com/anasalqoyyum/parabolica/issues/26)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [#31 Deliver backup, export/import and library relocation](https://github.com/anasalqoyyum/parabolica/issues/31)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `packaging/windows/**`
- `.github/workflows/package-windows.yml`
- `docs/SETUP_WINDOWS.md`
- `docs/RELEASE.md`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Produce portable and installer artifacts with the pinned MSVC/GPUI Kit/ASIO requirements and required runtime dependencies.
- Add package CI, checksums, dependency/license notices, app metadata and file-association handling.
- Keep design-reference art, test credentials and personal library data out of production packages.

## Acceptance criteria

- [ ] A clean Windows VM/user can install and run the packaged player and play a local FLAC file.
- [ ] Upgrade preserves valid user data and uninstall behavior is explicit and tested.
- [ ] ASIO-enabled builds include the app’s backend support without silently installing a vendor driver.
- [ ] Artifacts identify source commit/toolchain and carry checksums plus required notices.
- [ ] Signing is reported accurately; if signing credentials are unavailable, produce a clearly unsigned private artifact rather than falsely claiming a signed release.

## Validation and completion evidence

- Run clean package/install/upgrade/uninstall checks and inspect artifact contents.
- Verify CI artifacts and compare the packaged source SHA with the repository commit.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Authenticode signing, if chosen, requires the owner’s certificate/service. Publishing a release or distributing outside the private project requires the owner’s explicit authorization; producing reviewable private CI artifacts is the task.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
