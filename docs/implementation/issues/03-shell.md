## Outcome

Launch a native Windows player shell matching the saved design, with usable navigation and correct Light, Dark and Follow system behavior.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#2 Build the Rust workspace and freeze integration contracts](https://github.com/anasalqoyyum/parabolica/issues/2)
- [#4 Implement durable app storage, migrations and job state](https://github.com/anasalqoyyum/parabolica/issues/4)

## Can run in parallel with

- [#19 Resolve Discogs access, attribution and durable tag/artwork use](https://github.com/anasalqoyyum/parabolica/issues/19)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `apps/desktop/src/ui/shell/**`
- `apps/desktop/src/ui/theme/**`
- `apps/desktop/src/ui/appearance/**`
- `apps/desktop/src/main.rs`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Implement the window, sidebar, persistent player, navigation and reusable Kit-based controls, with integration slots for feature pages.
- Implement semantic theme tokens, persisted appearance selection through the storage contract, system app-theme observation and reduced-motion/high-contrast handling.
- Use the corrected Appearance reference: explicit Light stays light under a dark OS preference. Keep only useful labels and state.

## Acceptance criteria

- [ ] A real GPUI Kit window opens and all implemented navigation/settings controls are keyboard reachable.
- [ ] Explicit Light/Dark stays fixed; Follow system reacts to Windows changes before/after startup without a flash or resetting app state.
- [ ] Japanese text and IME input work, focus is visible, and 100/150/200% DPI layouts remain usable.
- [ ] Narrator can discover and operate representative Kit controls; document and fix framework gaps needed by our app.
- [ ] Standard controls use gpui-component; custom views preserve accessibility and the supplied design.

## Validation and completion evidence

- Run native Windows UI checks for theme transitions, DPI, IME and keyboard focus.
- Record actual Narrator observations separately from headless semantic assertions.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
