"""Reproduce the user-supplied reveal sounds without storing MP3s or silence."""
import argparse
import array
import hashlib
import json
import math
import subprocess
import wave
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def import_sound(ffmpeg, source, target, start, end, target_db):
    raw = subprocess.run(
        [str(ffmpeg), "-v", "error", "-i", str(source), "-af",
         f"atrim=start={start}:end={end},asetpts=PTS-STARTPTS",
         "-ac", "1", "-ar", "48000", "-f", "f32le", "pipe:1"],
        check=True, capture_output=True,
    ).stdout
    samples = array.array("f", raw)
    rms = math.sqrt(sum(x*x for x in samples) / len(samples))
    gain = 10 ** (target_db / 20) / rms
    duration = len(samples) / 48000
    filters = (
        f"volume={gain:.9f},"
        "alimiter=limit=0.891251:attack=1:release=35:level=false:latency=true,"
        "afade=t=in:d=0.003,"
        f"afade=t=out:st={duration - 0.02:.9f}:d=0.02"
    )
    subprocess.run(
        [str(ffmpeg), "-v", "error", "-y", "-f", "f32le", "-ar", "48000",
         "-ac", "1", "-i", "pipe:0", "-af", filters, "-map_metadata", "-1",
         "-c:a", "pcm_s16le", "-fflags", "+bitexact", str(target)],
        input=raw, check=True, capture_output=True,
    )
    with wave.open(str(target)) as wav:
        pcm = array.array("h", wav.readframes(wav.getnframes()))
        assert (wav.getnchannels(), wav.getsampwidth(), wav.getframerate()) == (1, 2, 48000)
    peak = max(abs(x) for x in pcm) / 32768
    output_rms = math.sqrt(sum((x / 32768)**2 for x in pcm) / len(pcm))
    return {
        "source": source.name,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "trim_start_seconds": start, "trim_end_seconds": end,
        "target_rms_dbfs": target_db, "gain_db": round(20 * math.log10(gain), 3),
        "duration_ms": len(pcm) * 1000 // 48000,
        "peak_dbfs": round(20 * math.log10(peak), 3),
        "rms_dbfs": round(20 * math.log10(output_rms), 3),
        "output_sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-dir", type=Path, required=True)
    parser.add_argument("--ffmpeg", type=Path, default=ROOT / "tools/ffmpeg.exe")
    args = parser.parse_args()
    output = ROOT / "resources/sfx"
    output.mkdir(parents=True, exist_ok=True)
    report = {}
    for name, source, start, end, target_db in [
        ("whoosh", "paper_tear.mp3", 0.451, 1.095, -16),
        ("ting", "appear.mp3", 0.055, 0.375, -18),
        ("pop", "dragon-studio-pop-402324.mp3", 0.173, 0.375, -18),
    ]:
        report[name] = import_sound(args.ffmpeg, args.source_dir / source,
                                    output / f"{name}.wav", start, end, target_db)
    (output / "sources.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
