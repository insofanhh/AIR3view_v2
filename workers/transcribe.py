"""Timestamped Vietnamese/multilingual ASR; output matches the Rust Word schema."""
import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input")
    parser.add_argument("--output")
    parser.add_argument("--batch", help="JSON array of input/output/language tasks; load model once")
    parser.add_argument("--language", default="vi")
    parser.add_argument("--model", default="large-v3-turbo")
    parser.add_argument("--device", default="cpu", choices=["cpu", "cuda"])
    args = parser.parse_args()
    if not args.batch and not (args.input and args.output):
        parser.error("Use --batch, or both --input and --output")
    jobs = json.loads(Path(args.batch).read_text(encoding="utf-8")) if args.batch else [
        {"input": args.input, "output": args.output, "language": args.language}
    ]
    from faster_whisper import WhisperModel
    model = WhisperModel(args.model, device=args.device, compute_type="int8" if args.device == "cpu" else "float16")
    for job in jobs:
        segments, _ = model.transcribe(job["input"], language=job.get("language") or None,
                                       word_timestamps=True, vad_filter=True, beam_size=1)
        words = []
        for segment in segments:
            for word in segment.words or []:
                start, end = round(word.start * 1000), round(word.end * 1000)
                if end > start:
                    words.append({"text": word.word.strip(), "start_ms": start, "end_ms": end})
        output = Path(job["output"])
        output.parent.mkdir(parents=True, exist_ok=True)
        pending = output.with_suffix(".tmp")
        pending.write_text(json.dumps(words, ensure_ascii=False, indent=2), encoding="utf-8")
        pending.replace(output)


if __name__ == "__main__":
    main()
