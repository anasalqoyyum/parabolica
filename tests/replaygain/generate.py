"""Regenerate CC0 synthetic scanner fixtures and independent C reference values.

Run: uv run python tests/replaygain/generate.py
Requires FFmpeg 8.0.1, a C compiler, curl and tar. No application code is used for measurements.
"""
import hashlib
import math
import struct
import subprocess
import tempfile
import wave
from pathlib import Path

HERE = Path(__file__).resolve().parent
URL = "https://github.com/jiixyj/libebur128/archive/refs/tags/v1.2.6.tar.gz"

def run(*args):
    return subprocess.run(args, check=True, capture_output=True).stdout

def main():
    version = run("ffmpeg", "-version").decode().splitlines()[0]
    if not version.startswith("ffmpeg version 8.0.1"):
        raise RuntimeError("Fixture encoder is pinned to FFmpeg 8.0.1")
    with tempfile.TemporaryDirectory(prefix="parabolica-rg2-reference-") as tmp:
        work = Path(tmp)
        archive = work / "libebur128.tar.gz"
        run("curl", "-fLs", URL, "-o", str(archive))
        archive_hash = hashlib.sha256(archive.read_bytes()).hexdigest()
        if archive_hash != "baa7fc293a3d4651e244d8022ad03ab797ca3c2ad8442c43199afe8059faa613":
            raise RuntimeError("Pinned libebur128 release checksum changed")
        run("tar", "-xzf", str(archive), "-C", str(work))
        source = work / "libebur128-1.2.6" / "ebur128"
        executable = work / "reference"
        run("cc", "-O2", "-I" + str(source), str(HERE / "reference.c"), str(source / "ebur128.c"), "-lm", "-o", str(executable))
        cases = [
            ("loud-44100", 44100, 3.0, "loud", 1, 1, 2),
            ("quiet-96000", 96000, 6.0, "quiet", 2, 1, 2),
            ("gated-48000", 48000, 5.0, "gated", 2, 2, 2),
            ("intersample-48000", 48000, 1.0, "over", 1, 2, 2),
            ("silence-44100", 44100, 1.0, "silence", 1, 3, 2),
            ("short-48000", 48000, 0.19, "loud", 1, 4, 2),
            ("mono-44100", 44100, 2.0, "loud", 1, 5, 1),
            ("surround-48000", 48000, 2.0, "surround", 1, 6, 6),
            ("highrate-192000", 192000, 2.0, "loud", 1, 7, 2),
        ]
        args = []
        paths = []
        for name, rate, duration, signal, disc, track, channels in cases:
            pcm = bytearray()
            for i in range(round(rate * duration)):
                t = i / rate
                if signal == "silence":
                    left = right = 0
                elif signal == "surround":
                    left = 0.1 * math.sin(math.tau * 997 * t)
                    right = 0.03 * math.sin(math.tau * 1703 * t)
                elif signal == "over":
                    left = right = 0.9 * math.sin(math.tau * 12000 * (i + 0.5) / rate)
                else:
                    amplitude = {"loud": 0.4, "quiet": 0.02, "gated": 0.3 if 1 < t < 3 else 0.001}[signal]
                    left = amplitude * math.sin(math.tau * 997 * t)
                    right = amplitude * math.sin(math.tau * 1703 * t)
                values = [left] if channels == 1 else [left, right]
                if channels == 6:
                    values = [left, right, left * 0.5, 0.8 * math.sin(math.tau * 60 * t), right, left * 0.25]
                pcm.extend(struct.pack("<" + "h" * channels, *(round(v * 32767) for v in values)))
            wav = work / (name + ".wav")
            with wave.open(str(wav), "wb") as output:
                output.setnchannels(channels)
                output.setsampwidth(2)
                output.setframerate(rate)
                output.writeframes(pcm)
            path = HERE / "fixtures" / (name + ".flac")
            run("ffmpeg", "-v", "error", "-y", "-i", str(wav), "-map_metadata", "-1", "-metadata", "ALBUM=RG2 two-disc fixture", "-metadata", "ALBUMARTIST=Parabolica fixtures", "-metadata", f"TITLE={name}", "-metadata", f"DISCNUMBER={disc}", "-metadata", f"TRACKNUMBER={track}", "-c:a", "flac", "-bitexact", str(path))
            raw = work / (name + ".f64")
            raw.write_bytes(run("ffmpeg", "-v", "error", "-i", str(path), "-f", "f64le", "-"))
            args.extend([str(raw), str(rate), str(channels)])
            paths.append((path, rate))
        output = run(str(executable), *args).decode().splitlines()
        rows = ["# Independent libebur128 1.2.6; source " + URL, "# source_sha256=" + archive_hash, "# encoder=" + version, "# mono=EBUR128_DUAL_MONO; peaks retain the single source channel", "# fixture\trate\tlufs\tsample_peak\ttrue_peak\tsha256"]
        for (path, rate), line in zip(paths, output):
            fields = line.split("\t")
            rows.append("\t".join([path.name, str(rate), *fields[2:], hashlib.sha256(path.read_bytes()).hexdigest()]))
        rows.append("# " + output[-1])
        # The mixed-rate album reference uses only the first three fixtures.
        album = run(str(executable), *args[:9]).decode().splitlines()[-1]
        rows.append("# mixed_rate_" + album)
        (HERE / "references.tsv").write_text("\n".join(rows) + "\n")
        print("\n".join(rows))

if __name__ == "__main__":
    main()
