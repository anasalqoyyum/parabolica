# Parabolica

<!-- impeccable:product-schema 1 -->

## Platform

Windows desktop is the primary validation platform. The implementation issue set also includes complete macOS and Linux desktop ports in the same delivery. Mobile and web product targets remain undefined; GPUI support alone does not define their scope.

## Stack

Rust with GPUI Kit is the selected UI stack. Use Kit's `gpui-component` controls and compose custom music-player views with its GPUI and `gpui-base` APIs. Pin Kit with its matching GPUI snapshot after Windows rendering, accessibility and integration checks. This stage produces a specification and design mockups, without an application implementation.

## Users

A personal music listener who wants YouTube Music streaming and a local file collection in one desktop player. The user accepts an experimental, unofficial YouTube Music integration for this personal project.

## Product purpose

Browse and play streaming music and local files, including lossless FLAC. Scan local albums for ReplayGain and preserve their intended differences in track loudness during album playback.

## Capabilities and constraints

- Windows is the initial release target.
- Local setup includes folder selection, library indexing, metadata, and playback.
- FLAC decoding and album ReplayGain scanning are required.
- Album-level Discogs metadata and album-art import are required in Files & tags. The user uses Mp3tag for this today. Release selection, track mapping and review must apply to a whole local album, including multi-disc releases, rather than requiring one lookup per song.
- Discogs API feasibility research is in `docs/research/DISCOGS.md`. Permanent Discogs artwork embedding and the interaction of API data-use rules with durable tags remain open integration questions. The feature requirement stays in scope; it is not a claim that these uses are already cleared.
- Light, Dark, and Follow system appearance are required. Follow system tracks Windows app-theme changes while the player is running.
- Windows local playback must support ASIO as well as WASAPI Shared and Exclusive. The user owns a FiiO K3 ASIO device; its installed driver and actual formats need validation on that Windows machine.
- YouTube Music Premium membership is acceptable. Membership does not establish third-party playback access.
- Streaming authentication, playback, and available quality need a feasibility spike.
- Foobar2000 is the behavioral reference for album ReplayGain scanning.
- Public distribution is outside the confirmed initial scope.

## Brand commitments

The supplied image is the starting visual reference: light surfaces, prominent square album artwork, a library sidebar, and persistent bottom playback controls. Parabolica is the repository name and a provisional product name.

Keep interface copy functional. Omit slogans, explanations of obvious controls, and commentary about theme changes applying throughout the app or preserving playback. Keep information that changes a choice, explains a non-obvious consequence, or helps recover from an error.

## Evidence on hand

The initial supplied image is preserved at `docs/design/references/initial-player.png`, with the later audio-transparency and copy feedback images beside it. The repository had no application code when this work began. Mockup counts, tracks, file paths, playback values, and account states are illustrative.

## Open decisions

Proposed features beyond the required playback and scanner capabilities, exact dependency versions, supported Windows baseline, mobile scope, app license, and future public distribution remain decisions for implementation.
