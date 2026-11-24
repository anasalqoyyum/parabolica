# Parabolica

A personal desktop music player combining local files and experimental YouTube Music streaming. The selected UI stack is Rust with GPUI Kit.

This repository contains the product requirements, technical specification, design artifacts, implementation work breakdown and the Rust workspace. The app includes the desktop shell, appearance settings, Windows WASAPI Shared playback and a persistent local collection. Add a music folder, browse Albums, Artists, Songs or Folders, search indexed metadata, and play an album while indexing continues. [Local collection evidence](docs/implementation/local-collection.md) separates automated checks from outstanding Windows runtime acceptance.

- [Product requirements](PRODUCT.md)
- [Technical specification](docs/TECH_SPEC.md)
- [Architecture, contracts and path ownership](docs/ARCHITECTURE.md)
- [Design system](DESIGN.md)
- [Design handoff and Doop frame links](docs/design/README.md)
- [Discogs API research](docs/research/DISCOGS.md)
- [Implementation issues, dependencies, and parallel work](docs/implementation/README.md)

## Required behavior

- Lossless FLAC decoding, gapless playback, reliable queues and session restoration.
- WASAPI Shared/Exclusive and ASIO output, including validation with the owner's FiiO K3.
- Track/album ReplayGain scanning, playback, tag interoperability and peak protection.
- Local library indexing, search, metadata/artwork editing and CUE sheets.
- Album-level Discogs release matching, tag review and artwork import.
- Unified local and YouTube Music browsing/playback, subject to a working authenticated integration.
- Light, Dark and Follow system appearance, keyboard access and Windows accessibility.

The implementation issue set covers the complete agreed delivery. Priorities specify execution order; a feature is not deferred to an unspecified later milestone. External access, provider constraints and hardware checks are explicit dependencies.

Windows is the first runtime target. The work breakdown also includes concrete macOS and Linux desktop ports. Mobile and web product targets remain undefined and are not treated as promised releases.

## Review the designs

Open [docs/design/index.html](docs/design/index.html) from a local checkout for the portable light/dark gallery. The `*-local.html` snapshots use the artwork committed under `docs/design/assets/`. They need no development server. Live Doop links are retained in the handoff.

Review images are committed copies of captures saved in the owner's Pictures/Screenshots folder. Artwork and the supplied references are design material, with provenance recorded in [SOURCES.md](docs/design/assets/SOURCES.md). They are not an approved production asset bundle.

## Agent work

Start with the assigned issue and [the agent workflow](docs/implementation/AGENT_WORKFLOW.md). Check its blockers and ownership before editing. Feature completion includes integration, error handling, meaningful checks and user-visible behavior, rather than a detached mock or an unconnected backend.

`master` is the default branch.

## Build and check

Rust 1.99.0 and GPUI Kit 0.11.0 are pinned. On Windows, install Visual Studio 2022 Build Tools with the C++ desktop workload and a Windows SDK, plus CMake. Then, from the repository root:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p parabolica-desktop --locked
```

[Architecture](docs/ARCHITECTURE.md) lists the toolchain requirements, crate layout and the CI workflow that runs these on Windows.
