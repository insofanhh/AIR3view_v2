"""Loopback-only persistent VieNeu v3 Turbo worker. No browser UI.

Run with the project virtualenv: python workers/vieneu_server.py
Model defaults to pnnbao-ump/VieNeu-TTS-v3-Turbo in the upstream SDK.
"""
from __future__ import annotations
import io
import base64
import binascii
import os
import hashlib
import sys
from pathlib import Path
import tempfile
import threading
from contextlib import asynccontextmanager
from typing import Literal

from fastapi import FastAPI, HTTPException, Header
from fastapi.responses import Response
from pydantic import BaseModel, Field

_engine = None
_lock = threading.Lock()
_worker_sha256 = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def engine():
    global _engine
    if _engine is None:
        from vieneu import Vieneu
        _engine = Vieneu()
    return _engine


@asynccontextmanager
async def lifespan(app):
    # Warm once; failures are surfaced at startup instead of on the first paid workflow.
    engine()
    yield


app = FastAPI(title="AIR3view / VieNeu", lifespan=lifespan)


class Speech(BaseModel):
    model: str = "vieneu-v3-turbo"
    input: str = Field(min_length=1, max_length=10000)
    voice: str = "Minh Quân"
    response_format: str = "wav"
    language: Literal["vi", "en"] = "vi"
    reference_audio: str | None = Field(default=None, max_length=24 * 1024 * 1024)
    reference_denoise: bool = True


@app.get("/health")
def health():
    return {"status": "ready", "model": "pnnbao-ump/VieNeu-TTS-v3-Turbo", "reference_audio": True, "languages": ["vi", "en"], "protocol": 3, "worker_sha256": _worker_sha256, "app": "AIR3view", "pid": os.getpid(), "python": sys.executable}


@app.get("/v1/voices")
def voices():
    with _lock:
        return {"voices": [{"label": label, "id": ident} for label, ident in engine().list_preset_voices()]}


@app.post("/v1/audio/speech")
def speech(body: Speech, authorization: str | None = Header(default=None)):
    expected = os.getenv("VIENEU_API_KEY")
    if expected and authorization != f"Bearer {expected}":
        raise HTTPException(401, "Invalid worker key")
    if body.model != "vieneu-v3-turbo" or body.response_format != "wav":
        raise HTTPException(400, "Use model vieneu-v3-turbo and response_format wav")
    import soundfile as sf
    # Temporary references are removed even if model inference fails. The SDK caches
    # speaker embeddings by audio content, so all scenes can reuse one voice.
    with tempfile.TemporaryDirectory(prefix="reaction-voice-") as temporary:
        reference = None
        if body.reference_audio is not None:
            try:
                raw = base64.b64decode(body.reference_audio, validate=True)
                if len(raw) > 16 * 1024 * 1024:
                    raise ValueError("reference too large")
                with sf.SoundFile(io.BytesIO(raw)) as clip:
                    duration = len(clip) / clip.samplerate
                    if clip.format != "WAV" or not 1 <= duration <= 8.1:
                        raise ValueError("invalid reference format or duration")
                reference = Path(temporary) / "reference.wav"
                reference.write_bytes(raw)
            except (binascii.Error, ValueError, RuntimeError):
                raise HTTPException(400, "Reference must be a valid base64 WAV of 1–8 seconds") from None
        with _lock:
            tts = engine()
            options = {"ref_audio": str(reference), "denoise": body.reference_denoise} if reference else {"voice": body.voice}
            audio = tts.infer(body.input, **options)
            output = io.BytesIO()
            sf.write(output, audio, tts.sample_rate, format="WAV", subtype="PCM_16")
    return Response(output.getvalue(), media_type="audio/wav", headers={"X-Reaction-Voice-Mode": "reference" if reference else "preset", "X-Reaction-Language": body.language})


if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="127.0.0.1", port=int(os.getenv("VIENEU_PORT", "8765")))
