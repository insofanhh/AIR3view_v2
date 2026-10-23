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

# ONNX external tensor files must stay beside their graphs. HF's symlink cache
# resolves them into separate blob directories on Developer Mode/admin machines.
# Use an app-owned, regular-file cache; do not relax ONNX path validation.
_data_root = Path(os.environ.get("REACTION_DATA_DIR") or
                  Path(os.environ.get("LOCALAPPDATA", str(Path.home() / ".cache"))) / "ReactionStudio")
os.environ["HF_HUB_CACHE"] = str(Path(os.environ.get("REACTION_MODEL_CACHE") or
                                   _data_root / "runtime" / "huggingface-regular-v1"))
os.environ["HF_HUB_DISABLE_SYMLINKS"] = "1"


def _watch_owner():
    """A GUI-owned worker must also exit if its GUI crashes or is terminated."""
    owner = os.getenv("AIR3VIEW_WORKER_OWNER_PID")
    if os.name != "nt" or not owner:
        return
    import ctypes
    from ctypes import wintypes
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.WaitForSingleObject.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    kernel.WaitForSingleObject.restype = wintypes.DWORD
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    handle = kernel.OpenProcess(0x00100000, False, int(owner))
    if not handle:
        # Parent may have exited before Python finished booting.
        if ctypes.get_last_error() == 87:
            os._exit(0)
        return
    try:
        if kernel.WaitForSingleObject(handle, 0xFFFFFFFF) == 0:
            os._exit(0)
    finally:
        kernel.CloseHandle(handle)


threading.Thread(target=_watch_owner, name="air3view-owner", daemon=True).start()

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
    # The native editor can open without downloading/loading a speech model.
    # First synthesis is serialized by _lock and initializes the engine once.
    # CLI/verification workers retain eager initialization unless opted out.
    if os.getenv("AIR3VIEW_LAZY_VOICE") != "1":
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
    return {"status": "ready", "model": "pnnbao-ump/VieNeu-TTS-v3-Turbo", "reference_audio": True, "languages": ["vi", "en"], "protocol": 3, "worker_sha256": _worker_sha256, "app": "AIR3view", "pid": os.getpid(), "python": sys.executable, "session": os.getenv("AIR3VIEW_WORKER_SESSION", ""), "model_loaded": _engine is not None}


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
