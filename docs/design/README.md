# Design handoff

Updated 2026-11-06. The proposed Windows UI follows the supplied album-art-led desktop reference and impeccable's Operate guidance. Light, Dark, and Follow system are required. Local playback supports a proposed ASIO path for the user's FiiO K3 alongside WASAPI Shared and Exclusive.

[Technical specification](../TECH_SPEC.md) · [Design system](../../DESIGN.md)

## Doop

[Canvas](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS) · [Prototype](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS/proto)

The canvas has Light and Dark pages, each with seven matching screens. Each page starts its prototype at Albums.

| Screen | Light | Dark |
| --- | --- | --- |
| Albums | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=MEa1E_TS4x) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=Ql2mDnh2V6) |
| Album detail | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=28B5YbgdAq) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=ySa1MFEyrY) |
| ReplayGain | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=SiHuLDUKLQ) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=sGdl2HHDGa) |
| ASIO signal path | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=JUMRy7HkD_) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=9eUKiSSvM0) |
| Sources & output | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=iaAnLYtxOn) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=a8v12B7zbH) |
| Listening device | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=v6rsM8wkiS) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=h2j2D-Yndg) |
| Appearance | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=AwqtZ6bXM6) | [Frame](https://en.pipefish-sirius.ts.net:8443/c/yw3yVRBewS?frame=l42qoG9mGI) |

The top-bar appearance shortcut and settings navigation open Appearance. Its Light artboard starts with Light selected and its Dark artboard starts with Dark selected. Follow system observes browser color-scheme changes after explicit selection in this mockup; the native app defaults to Follow system and must observe Windows app-theme changes and persist the user's mode. Navigation targets update to the matching theme after a choice. GPUI Kit is the selected UI framework; standard controls come from `gpui-component`, with custom player views composed using Kit and `gpui-base`.

Listening device separates the backend from its driver, channel mapping and rate policy. The ASIO example shows the FiiO K3 with a driver-managed 256-frame buffer. At 96 kHz that holds 2.67 ms of audio; the separate latency field says Not reported by driver. The inspector identifies the Windows mixer as bypassed, keeps ReplayGain visible, and makes no bit-perfect claim.

Cross-screen navigation is wired. Form fields illustrate the intended settings and do not configure hardware, scan files, sign in, or start playback. Album cards route to the representative Solo Piano screen. Escape closes the signal inspector. Its underlying album is a representative static state rather than preserved navigation history.

## Review and limitations

Recaptured all fourteen 1440 × 960 frames after the copy cleanup and reviewed them in a contact sheet, with full-size checks of Appearance Light/Dark, Listening device and Sources. Removed the page taglines, appearance explanation blocks, redundant current-theme readout, repeated scanner helper text and static ready-index announcement. Kept controls, values, relevant write consequences, driver constraints and recovery information. Reproduced the old Light frame switching to Dark under a dark OS preference, then verified the fixed startup remains Light and an explicit Follow system choice still follows Dark. Previous palette checks remain applicable; tested text pairs exceed 4.5:1.

Current screenshots are in the user's Pictures/Screenshots folder, named `parabolica-<screen>[-dark]-concise-20261106.png`. [gallery.html](gallery.html) opens on the corrected Appearance Light screen and provides light/dark preview controls for every screen. The HTML documents are fixed-size mockups. Smaller-window behavior, native Windows theme events, keyboard/Narrator behavior and actual ASIO playback remain implementation checks. The K3 driver name, sample format, buffer and channel values are illustrative until the installed driver is inspected on the user's Windows machine.

Committed copies of those captures are under [screenshots](screenshots/), and the supplied feedback images are under [references](references/). Open [index.html](index.html) for the portable gallery after cloning. `gallery.html` is the original T3 presentation source. Regenerate the portable gallery and local HTML variants with `python3 scripts/prepare_design_review.py` from the repository root.

The supplied reference was inspected directly. Doop's screenshot renderer could not load the private attachment-host URL during the original task, so proposed screens use verified public artwork URLs. No AI-generated raster was needed.

## Local snapshots and assets

Light snapshots use `<screen>.html`; dark snapshots use `<screen>-dark.html`. The corresponding `*-local.html` variants use relative paths to the downloaded artwork. These are exports of the current Doop frames, not an app implementation. [frames.json](frames.json) maps both themes to frame IDs. The files open without a development server and use a Noto Sans JP stylesheet for browser CJK fallback.

[catalog.json](assets/catalog.json) records artwork URLs and local files. [SOURCES.md](assets/SOURCES.md) records provenance. Artwork and album metadata are reference content for this exercise; no distribution rights for a bundled catalog are asserted. All library counts, tracks, paths, account states and measurements are illustrative, as labelled in the window.
