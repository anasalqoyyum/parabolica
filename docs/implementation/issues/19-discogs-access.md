## Outcome

Establish the supported data-use and access model for the required personal Discogs album tagging and persistent artwork workflow.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

No task prerequisites. Check the external requirements below before claiming completion.

## Can run in parallel with

- [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)
- [#3 Build the GPUI Kit desktop shell and appearance settings](https://github.com/anasalqoyyum/parabolica/issues/3)
- [#4 Implement durable app storage, migrations and job state](https://github.com/anasalqoyyum/parabolica/issues/4)
- [#5 Play local FLAC through the real Windows audio engine](https://github.com/anasalqoyyum/parabolica/issues/5)
- [#14 Implement provider contracts and secure credential storage](https://github.com/anasalqoyyum/parabolica/issues/14)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `docs/research/DISCOGS.md`
- `docs/research/DISCOGS_INTEGRATION_DECISION.md`
- `tests/provider-live/discogs_access/**`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Resolve how the current API terms, CC0 metadata, Restricted Data images, freshness/storage rules and attribution apply to durable local tags and artwork.
- Verify the current search/release/master/image responses, auth requirements, quotas and intended personal credential flow.
- Record a concrete supported implementation decision, including what requires provider or owner input.

## Acceptance criteria

- [ ] The decision covers durable local metadata, embedded images and sidecar images separately, with current primary-source evidence.
- [ ] Required attribution is designed for the import context and does not become repeated generic warnings throughout the player.
- [ ] Public lookup observations and authenticated/production access requirements are distinguished.
- [ ] Unresolved provider permission or owner choices remain explicit blockers; merely restating the old research uncertainty does not complete this issue.
- [ ] The required feature stays in scope until an explicit owner decision changes it; a different art source is not a silent replacement for Discogs.

## Validation and completion evidence

- Review the decision against live first-party API behavior and current terms.
- Record written clarifications or owner decisions when those are needed; no secrets enter the repository.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Provider clarification or an owner product decision may be needed. Sending a message to Discogs requires separate explicit authorization; the issue does not authorize outbound correspondence. This is a real input blocker, not a later milestone.

Priority: P1. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
