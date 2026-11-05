## Outcome

Use Update from Discogs in Files & tags to match, review and update an entire local album and its artwork, including multi-disc/CUE cases.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#13 Support CUE album images and virtual tracks throughout the player](https://github.com/anasalqoyyum/parabolica/issues/13)

## Can run in parallel with

- [#18 Complete streaming discovery, playlists, likes and radio](https://github.com/anasalqoyyum/parabolica/issues/18)
- [#22 Deliver the expandable audio signal-path inspector](https://github.com/anasalqoyyum/parabolica/issues/22)
- [#23 Integrate Windows media controls, dialogs and application lifecycle](https://github.com/anasalqoyyum/parabolica/issues/23)
- [#27 Deliver ratings, smart playlists and collection organization](https://github.com/anasalqoyyum/parabolica/issues/27)
- [#28 Deliver a sleep timer for local and streaming playback](https://github.com/anasalqoyyum/parabolica/issues/28)
- [#29 Deliver lyrics from supported local and permitted provider sources](https://github.com/anasalqoyyum/parabolica/issues/29)
- [#30 Deliver opt-in scrobbling with secure auth and offline recovery](https://github.com/anasalqoyyum/parabolica/issues/30)
- [#32 Produce reproducible Windows installers and release artifacts](https://github.com/anasalqoyyum/parabolica/issues/32)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `apps/desktop/src/ui/tags/discogs/**`
- `crates/tags/src/discogs_import/**`
- `crates/storage/migrations/0021_*.sql`
- `docs/design/discogs-flow/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement release search/selection with edition details and a local-file/source-track mapping view.
- Implement current/proposed album and individual-track fields with keep/fill-empty/replace controls and separate artwork/destination review.
- Apply through the safe album write service and persist permitted release/mapping provenance for reviewed reimports.
- Complete the missing Files & tags/Discogs UI states in both themes, following the functional-copy preference.

## Acceptance criteria

- [ ] One user-triggered import handles all selected album discs; a lookup/write per song is not required.
- [ ] Missing, extra, reordered, compilation, bonus, heading/index and combined/split cases cannot silently shift subsequent mappings.
- [ ] Selected album fields reach all included targets and track fields reach only their resolved files.
- [ ] Chosen cover/secondary art reaches embedded/sidecar/both destinations under the resolved source rules, preserving untouched art and avoiding duplicate retries.
- [ ] Reimport shows a fresh diff, preserves later manual edits until selected, and retains ReplayGain/PCM/library identity.
- [ ] Partial failures and active-file/concurrent-Mp3tag conflicts can be recovered from the UI with per-file outcomes.

## Validation and completion evidence

- Run the full release selection→mapping→diff→art review→write→reopen flow on multi-disc and CUE fixtures.
- Verify physical tags/artwork, PCM equality, exclusions, partial retry and reimport behavior.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
