# Agent workflow

Read the assigned GitHub issue, [the workflow](docs/implementation/AGENT_WORKFLOW.md), and the spec sections it references before editing. [PRODUCT.md](PRODUCT.md) records product decisions; [DESIGN.md](DESIGN.md) records the UI and copy rules. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) is the source of truth for the workspace layout, check commands, path ownership and the shared contracts (domain, storage, desktop shell, provider accounts); read the sections your issue touches.

Use the issue's blockers and ownership boundary. A blocker is satisfied once its implementation is merged on `master`, even while the issue stays open for hardware or account checks; its latest issue comment lists what remains unverified. Parallel work is safe only after both issues' prerequisites are complete and their shared contracts are merged. Coordinate changes to shared manifests, module registries and migrations. Preserve concurrent edits.

Feature completion includes a real application entry point, integrated behavior, failure handling and relevant verification. Record actual hardware/account evidence separately from fixtures and simulations. Keep unmet external requirements visible. Follow the user's Git, external-message and deployment authorization for the assignment.

The selected stack is Rust with GPUI Kit. Keep decoder, DSP, output, storage and provider work independent of UI code. Apply the repository's semantic Light/Dark themes and functional-copy preference. Design reference artwork and captures are review material; package only production assets with appropriate rights.

## Windows verification

Windows MSVC is the primary target and CI runs there; run the ARCHITECTURE check commands on Windows as well as Linux before opening a PR.

- From WSL, run the Windows toolchain (`cargo.exe`) through PowerShell on the worktree's `\\wsl.localhost\...` path, with `CARGO_TARGET_DIR` on a Windows drive and one target directory per worktree so parallel threads don't contend for build locks.
- A running `parabolica.exe` holds its binary and makes the next build fail with "Access is denied". Close the app before rebuilding.
- Tests and manual checks that touch OS state (Credential Manager, audio devices, settings) use disposable values in their own namespace and remove what they create. Confirm the cleanup with the OS tool, for example `cmdkey /list:Parabolica*`.
- Drive real-app checks through UI Automation, the tree Narrator reads, and report them separately from headless UI tests.
