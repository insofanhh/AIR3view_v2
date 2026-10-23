"""Timestamped Vietnamese/multilingual ASR; output matches the Rust Word schema."""
import argparse
import json
import os
from pathlib import Path
import sys
import time

from cuda_runtime import error_message, prepare, select_compute

for stream in (sys.stdout, sys.stderr):
    if hasattr(stream, "reconfigure"):
        stream.reconfigure(encoding="utf-8", errors="replace")


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    pending = path.with_suffix(path.suffix + f".{os.getpid()}.tmp")
    pending.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")
    pending.replace(path)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--input")
    parser.add_argument("--output")
    parser.add_argument("--batch", help="JSON array of input/output/language tasks; load model once")
    parser.add_argument("--language", default="vi")
    parser.add_argument("--model", default="large-v3-turbo")
    parser.add_argument("--device", default="cpu", choices=["cpu", "cuda"])
    parser.add_argument("--compute-type", default="auto",
                        choices=["auto", "int8", "int8_float16", "int8_float32", "float16", "float32"])
    parser.add_argument("--check-cuda", action="store_true", help="Check the actual GPU and cuBLAS; no download")
    parser.add_argument("--setup-cuda", action="store_true", help="Prepare the private GPU runtime, then check it")
    args = parser.parse_args()
    if args.check_cuda or args.setup_cuda:
        print(json.dumps({"status": "ready", "device": "cuda", **prepare(install=args.setup_cuda)}, ensure_ascii=False), flush=True)
        return
    if not args.batch and not (args.input and args.output):
        parser.error("Use --batch, or both --input and --output")
    jobs = json.loads(Path(args.batch).read_text(encoding="utf-8")) if args.batch else [
        {"input": args.input, "output": args.output, "language": args.language}
    ]
    # Register DLL directories before importing the backend. GPU is always explicit:
    # never silently report CPU inference as a successful CUDA run.
    runtime = prepare(install=True) if args.device == "cuda" else {}
    import ctranslate2
    from faster_whisper import WhisperModel
    supported = ctranslate2.get_supported_compute_types(args.device, 0)
    compute = select_compute(args.device, args.compute_type, supported)
    started = time.monotonic()
    print(json.dumps({"status": "loading", "model": args.model, "device": args.device,
                      "compute_type": compute}, ensure_ascii=False), flush=True)
    model = WhisperModel(args.model, device=args.device, compute_type=compute, num_workers=1,
                         cpu_threads=min(4, os.cpu_count() or 1))
    actual_device = model.model.device
    if actual_device != args.device:
        raise RuntimeError(f"Whisper chạy trên {actual_device}, không đúng thiết bị yêu cầu {args.device}.")
    model_seconds = round(time.monotonic() - started, 3)
    for job in jobs:
        started = time.monotonic()
        segments, _ = model.transcribe(job["input"], language=job.get("language") or None,
                                       word_timestamps=True, vad_filter=True, beam_size=1)
        words = []
        for segment in segments:
            for word in segment.words or []:
                start, end = round(word.start * 1000), round(word.end * 1000)
                if end > start:
                    words.append({"text": word.word.strip(), "start_ms": start, "end_ms": end})
        output = Path(job["output"])
        write_json(output, words)
        report = {"status": "complete", "model": args.model, "device": actual_device,
                  "compute_type": model.model.compute_type, "backend": ctranslate2.__version__,
                  "word_count": len(words), "model_load_seconds": model_seconds,
                  "transcribe_seconds": round(time.monotonic() - started, 3), **runtime}
        write_json(output.with_suffix(".asr.json"), report)
        print(json.dumps(report, ensure_ascii=False), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        device = "cuda" if "--check-cuda" in sys.argv or "--setup-cuda" in sys.argv else "cpu"
        if "--device" in sys.argv:
            device = sys.argv[sys.argv.index("--device") + 1]
        print(error_message(exc, device), file=sys.stderr, flush=True)
        sys.exit(1)
