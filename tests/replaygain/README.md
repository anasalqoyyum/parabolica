# ReplayGain reference fixtures

Run `uv run python tests/replaygain/generate.py` to regenerate the FLAC fixtures and `references.tsv`. The generator requires FFmpeg 8.0.1, curl, tar and a C compiler. It verifies the SHA-256 of libebur128 1.2.6 before compiling the independent C runner. Application tests compare the Rust analyzer and Symphonia decoder against these C/FFmpeg measurements.

Signals, generated fixtures and the reference runner are CC0-1.0. They contain synthetic tones, envelopes and silence. Loudness/gain tolerance is 0.1 dB. Linear peak tolerances are 1e-10 for sample peak and 1e-5 for true peak. See [the implementation evidence](../../docs/implementation/replaygain-analysis.md) for measurement, grouping and validity rules.
