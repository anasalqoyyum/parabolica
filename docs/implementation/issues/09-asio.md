## Outcome

Select the installed FiiO K3 ASIO driver and play local audio through it with working rate, channel and buffer controls.

## Read before editing

- [Product requirements](https://github.com/anasalqoyyum/parabolica/blob/master/PRODUCT.md)
- [Technical specification](https://github.com/anasalqoyyum/parabolica/blob/master/docs/TECH_SPEC.md), especially the sections covering this outcome.
- [Design rules](https://github.com/anasalqoyyum/parabolica/blob/master/DESIGN.md) and [design handoff](https://github.com/anasalqoyyum/parabolica/blob/master/docs/design/README.md) for user-visible work.
- [Agent workflow](https://github.com/anasalqoyyum/parabolica/blob/master/docs/implementation/AGENT_WORKFLOW.md). Check merged blocker evidence and coordinate shared registration/migration changes before starting.

## Blocked by

- [#8 Implement WASAPI Exclusive and listening-device controls](https://github.com/anasalqoyyum/parabolica/issues/8)

## Can run in parallel with

- [#10 Implement album and track ReplayGain analysis with the scanner UI](https://github.com/anasalqoyyum/parabolica/issues/10)
- [#12 Implement safe album metadata and artwork editing](https://github.com/anasalqoyyum/parabolica/issues/12)
- [#15 Resolve and prove authenticated YouTube Music playback access](https://github.com/anasalqoyyum/parabolica/issues/15)
- [#20 Implement the Discogs release and artwork source adapter](https://github.com/anasalqoyyum/parabolica/issues/20)

A pairing is valid only when both tasks' blockers are complete and shared contracts are merged. These are compatible ownership boundaries, not permission to bypass dependencies. Serialize shared manifests, module registration and migrations; coordinate any fix crossing another active owner's files.

## Ownership

- `crates/audio/src/output/asio/**`
- `crates/platform/src/windows/asio/**`
- `apps/desktop/src/ui/listening_device/asio.rs`
- `.github/workflows/windows-asio.yml`

Paths are proposed boundaries. Use the foundation issue's final layout and coordinate boundary changes before parallel edits.

## Deliverables

- Integrate the pinned ASIO backend/SDK build and enumerate compatible driver architecture, outputs, sample types and supported rates.
- Wire ASIO into the existing player and Listening device UI, including driver-panel support where exposed.
- Report real buffer frames, negotiated format and driver-reported latency separately from computed buffer duration.

## Acceptance criteria

- [ ] The native app plays through the actual FiiO K3 ASIO driver; a callback demo or WASAPI fallback is insufficient.
- [ ] Rate policy, output channel mapping, driver-managed buffer and driver panel behave as reported by the installed driver.
- [ ] Driver resets, busy/missing drivers, unplug/replug and sleep/resume recover without callback blocking or silent backend changes.
- [ ] Windows mixer/session volume are not shown as active ASIO processing; app gain and volume remain truthful.
- [ ] Windows ASIO build requirements and license/dependency notices are reproducible in CI and local setup.

## Validation and completion evidence

- Run backend/driver fault fixtures and a Windows build with the ASIO feature.
- Record K3 model/revision, USB mode, driver version, real negotiated formats and audible/captured playback evidence.

Record commands/results, source commit, fixtures and actual runtime/account/device observations. Feature work must be wired into the app. Keep this issue open while any acceptance criterion or required external validation is unmet.

## External requirements

Requires a Windows machine with the FiiO K3 attached and its appropriate installed ASIO driver. Driver installation or machine access needs the user’s authorization. Keep the issue open if actual hardware validation is unavailable.

Priority: P0. All tasks belong to this delivery; priority sets execution order.

Posted by gpt-6.1-sol on behalf of Anas Alqoyyum.
