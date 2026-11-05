## Outcome

Establish a reproducible Rust/GPUI Kit workspace and shared contracts so feature agents can work without inventing incompatible models.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

No task prerequisites. Check the external requirements below before claiming completion.

## Can run in parallel with

- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `Cargo.toml`
- `Cargo.lock`
- `rust-toolchain.toml`
- `apps/desktop/Cargo.toml`
- `crates/domain/**`
- `crate manifests and initial module roots`
- `.github/workflows/check.yml`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Pin GPUI Kit with its matching GPUI snapshot and toolchain; create desktop, domain, storage, library, audio, replaygain, tags, cue, provider and platform module boundaries. Choose crate splits for ownership rather than unnecessary abstraction.
- Define stable IDs and commands/events for tracks, releases, physical/virtual files, playable sources, queues, decoder/output formats, signal-path stages, gains, tag patches, background jobs and provider capabilities.
- Define the UI dispatch and versioned snapshot/event rules, cancellation/generation IDs, ownership of SQLite writes, and a no-blocking-work audio callback contract.
- Publish the path and schema/migration ownership map and make Windows CI build/check the initial workspace.

## Acceptance criteria

- [ ] A fresh Windows MSVC checkout resolves the pinned dependencies and runs the documented fmt, lint, test and build commands.
- [ ] Core contracts distinguish source resolution, internal PCM and negotiated output; ASIO and browser-owned output fit the model.
- [ ] Stable album/queue/file identities and CUE virtual ranges have types and contract tests.
- [ ] Feature module boundaries and registration points are documented and usable by downstream tasks without editing competing root manifests.
- [ ] CI exercises real compilable Rust contracts; placeholder success jobs do not satisfy this issue.

## Validation and completion evidence

- Run the pinned Windows build and core contract tests.
- Check Cargo.lock reproducibility and CI logs from a clean checkout.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
