"""Regenerate the decoder fixtures and their reference checksums.

Run from this directory:

    uv run --with soundfile==0.13.1 --with numpy==2.3.4 python generate.py

Requires `ffmpeg` with libmp3lame, libopus and libvorbis on PATH. Every signal is synthetic.

Lossless files are encoded with libFLAC (through libsndfile). `references.json` records, for
each lossless file, the MD5 of the PCM decoded by two independent decoders: libFLAC/libsndfile
and FFmpeg. The bytes hashed are interleaved little-endian signed integers of the source width,
the layout FLAC's STREAMINFO MD5 uses. The script fails if the two decoders disagree.
"""

import hashlib
import json
import subprocess
from pathlib import Path

import numpy as np
import soundfile as sf

HERE = Path(__file__).resolve().parent
LOSSLESS_SECONDS = 0.2
LOSSY_SECONDS = 1.0

LOSSLESS = [
    ("flac/s16_44100.flac", "FLAC", "PCM_16", 16, 44_100),
    ("flac/s24_44100.flac", "FLAC", "PCM_24", 24, 44_100),
    ("flac/s24_48000.flac", "FLAC", "PCM_24", 24, 48_000),
    ("flac/s24_88200.flac", "FLAC", "PCM_24", 24, 88_200),
    ("flac/s24_96000.flac", "FLAC", "PCM_24", 24, 96_000),
    ("flac/s24_176400.flac", "FLAC", "PCM_24", 24, 176_400),
    ("flac/s24_192000.flac", "FLAC", "PCM_24", 24, 192_000),
    ("wav/s16_44100.wav", "WAV", "PCM_16", 16, 44_100),
    ("wav/s24_48000.wav", "WAV", "PCM_24", 24, 48_000),
]


def lossless_signal(bits: int, rate: int) -> np.ndarray:
    """Stereo integers: leading silence, a 997 Hz sine, both extremes, and low-level noise."""
    frames = int(rate * LOSSLESS_SECONDS)
    full = (1 << (bits - 1)) - 1
    n = np.arange(frames)
    left = np.round(0.9 * full * np.sin(2 * np.pi * 997 * n / rate)).astype(np.int64)
    # A fixed linear congruential generator keeps the noise identical across platforms.
    state = 0x2545F491
    noise = np.empty(frames, dtype=np.int64)
    span = 1 << (bits - 8)
    for i in range(frames):
        state = (state * 1103515245 + 12345) & 0x7FFFFFFF
        noise[i] = state % (2 * span) - span
    right = noise
    left[:64] = 0
    right[:64] = 0
    left[100], right[100] = full, -full - 1
    left[101], right[101] = -full - 1, full
    return np.stack([left, right], axis=1)


def pcm_md5(samples: np.ndarray, bits: int) -> str:
    width = bits // 8
    data = bytearray()
    for value in samples.reshape(-1):
        data += int(value).to_bytes(width, "little", signed=True)
    return hashlib.md5(data).hexdigest()


def libsndfile_md5(path: Path, bits: int) -> tuple[str, int]:
    decoded, _ = sf.read(path, dtype="int32", always_2d=True)
    return pcm_md5(decoded.astype(np.int64) >> (32 - bits), bits), decoded.shape[0]


def ffmpeg_md5(path: Path, bits: int) -> str:
    fmt = {16: "s16le", 24: "s24le"}[bits]
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-f", fmt, "-"],
        check=True,
        capture_output=True,
    ).stdout
    return hashlib.md5(raw).hexdigest()


FFMPEG_VERSION = subprocess.run(
    ["ffmpeg", "-version"], check=True, capture_output=True, text=True
).stdout.split(" Copyright")[0]


def ffmpeg(*args: str) -> None:
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def lossy_source(rate: int) -> Path:
    """A 16-bit WAV of two half-scale linear sweeps, matching `lossy_reference` in `decode.rs`.

    Left rises from 200 Hz to 2 kHz and right falls from 2 kHz to 200 Hz over one second. A sweep
    has one alignment with the decoded output, unlike a steady tone.
    """
    frames = int(rate * LOSSY_SECONDS)
    t = np.arange(frames) / rate
    left = 0.5 * np.sin(2 * np.pi * (200 * t + 900 * t * t))
    right = 0.5 * np.sin(2 * np.pi * (2000 * t - 900 * t * t))
    path = HERE / f"lossy_source_{rate}.wav"
    sf.write(path, np.stack([left, right], axis=1), rate, subtype="PCM_16")
    return path


def frame_count(path: Path) -> int:
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-"],
        check=True,
        capture_output=True,
    ).stdout
    return len(raw) // 8


def main() -> None:
    references: dict[str, dict] = {}
    for name, container, subtype, bits, rate in LOSSLESS:
        path = HERE / name
        path.parent.mkdir(exist_ok=True)
        samples = lossless_signal(bits, rate)
        scaled = samples.astype(np.int32) << (32 - bits)
        sf.write(path, scaled, rate, format=container, subtype=subtype)
        expected = pcm_md5(samples, bits)
        libsndfile, frames = libsndfile_md5(path, bits)
        reference = ffmpeg_md5(path, bits)
        assert libsndfile == expected == reference, name
        references[name] = {
            "bits": bits,
            "rate": rate,
            "channels": 2,
            "frames": frames,
            "pcm_md5": expected,
            "decoders": [
                ("libFLAC via " if container == "FLAC" else "") + "libsndfile " + sf.__libsndfile_version__,
                FFMPEG_VERSION,
            ],
        }

    source = lossy_source(44_100)
    lossy_dir = HERE / "lossy"
    lossy_dir.mkdir(exist_ok=True)
    ffmpeg("-i", str(source), "-c:a", "libmp3lame", "-b:a", "192k", str(lossy_dir / "sine.mp3"))
    ffmpeg("-i", str(source), "-c:a", "aac", "-b:a", "192k", str(lossy_dir / "sine.m4a"))
    for name in ["lossy/sine.mp3", "lossy/sine.m4a"]:
        references[name] = {"rate": 44_100, "channels": 2, "ffmpeg_frames": frame_count(HERE / name)}

    unsupported = HERE / "unsupported"
    unsupported.mkdir(exist_ok=True)
    ffmpeg("-i", str(source), "-c:a", "libopus", "-b:a", "96k", str(unsupported / "opus.ogg"))
    ffmpeg("-i", str(source), "-c:a", "libvorbis", "-q:a", "3", str(unsupported / "vorbis.ogg"))
    ffmpeg("-i", str(source), "-c:a", "alac", str(unsupported / "alac.m4a"))
    (unsupported / "not-audio.flac").write_text("This is a text file with a .flac extension.\n")
    source.unlink()

    (HERE / "references.json").write_text(json.dumps(references, indent=2) + "\n")


if __name__ == "__main__":
    main()
