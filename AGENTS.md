# Agent workflow

Read the assigned GitHub issue, [the workflow](docs/implementation/AGENT_WORKFLOW.md), and the spec sections it references before editing. [PRODUCT.md](PRODUCT.md) records product decisions; [DESIGN.md](DESIGN.md) records the UI and copy rules.

Use the issue's blockers and ownership boundary. Parallel work is safe only after both issues' prerequisites are complete and their shared contracts are merged. Coordinate changes to shared manifests, module registries and migrations. Preserve concurrent edits.

Feature completion includes a real application entry point, integrated behavior, failure handling and relevant verification. Record actual hardware/account evidence separately from fixtures and simulations. Keep unmet external requirements visible. Follow the user's Git, external-message and deployment authorization for the assignment.

The selected stack is Rust with GPUI Kit. Keep decoder, DSP, output, storage and provider work independent of UI code. Apply the repository's semantic Light/Dark themes and functional-copy preference. Design reference artwork and captures are review material; package only production assets with appropriate rights.

External comments and messages posted on behalf of the owner must end with `Posted by <actual model-name> on behalf of Anas Alqoyyum.` Use runtime metadata for the model name.
