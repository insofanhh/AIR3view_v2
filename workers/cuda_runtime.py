"""Optional, app-private Windows CUDA runtime. Never change drivers or system PATH."""
import ctypes
from contextlib import contextmanager
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

# Keep handles alive: Windows removes a DLL search directory when its handle closes.
_dll_directories = []
_libraries = []
BACKEND_VERSION = "4.8.3"
CUBLAS_VERSION = "12.8.4.1"
RUNTIME_ID = f"ctranslate2-{BACKEND_VERSION}-cublas-{CUBLAS_VERSION}-win-x64"


def runtime_installed(directory):
    marker = runtime_dir() / ".ready"
    return (marker.is_file() and marker.read_text(encoding="ascii").strip() == RUNTIME_ID
            and all((directory / name).is_file() for name in ("cublas64_12.dll", "cublasLt64_12.dll")))


@contextmanager
def installation_lock():
    import msvcrt
    directory = runtime_dir()
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / ".install.lock").open("a+b") as lock:
        if lock.tell() == 0:
            lock.write(b"0")
            lock.flush()
        started = time.monotonic()
        while True:
            lock.seek(0)
            try:
                msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
                break
            except OSError:
                if time.monotonic() - started > 1800:
                    raise RuntimeError("Một phiên AIR3view khác đang chuẩn bị CUDA; thử lại sau.")
                time.sleep(0.2)
        try:
            yield
        finally:
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_UNLCK, 1)


def runtime_dir():
    data = os.environ.get("REACTION_DATA_DIR") or str(
        Path(os.environ.get("LOCALAPPDATA", Path.home())) / "ReactionStudio")
    return Path(data) / "runtime" / "asr-cuda12-v1"


def driver_info():
    if sys.platform != "win32":
        raise RuntimeError("AIR3view CUDA runtime hiện hỗ trợ Windows x64. Chọn CPU trên hệ điều hành khác.")
    try:
        # Only load NVIDIA's system driver, never a downloaded nvcuda.dll.
        driver = ctypes.WinDLL(str(Path(os.environ["SystemRoot"]) / "System32" / "nvcuda.dll"))
    except OSError as exc:
        raise RuntimeError("Không thấy driver NVIDIA. CUDA cần GPU NVIDIA; chọn CPU hoặc cài driver NVIDIA phù hợp.") from exc
    version = ctypes.c_int()
    status = driver.cuDriverGetVersion(ctypes.byref(version))
    if status or version.value < 12080:
        raise RuntimeError(
            f"Driver NVIDIA chỉ hỗ trợ CUDA {version.value // 1000}.{version.value % 1000 // 10}. "
            "Bộ Whisper này cần driver hỗ trợ CUDA 12.8 trở lên (Windows 572.61+). "
            "Cập nhật driver NVIDIA rồi mở lại app, hoặc chọn CPU. Không cần cài CUDA Toolkit.")
    status = driver.cuInit(0)
    if status:
        raise RuntimeError(f"Không khởi tạo được NVIDIA CUDA (cuInit={status}). Kiểm tra GPU/driver hoặc chọn CPU.")
    count = ctypes.c_int()
    if driver.cuDeviceGetCount(ctypes.byref(count)) or count.value < 1:
        raise RuntimeError("Không có GPU NVIDIA sẵn sàng cho CUDA. Chọn CPU.")
    name = ctypes.create_string_buffer(256)
    driver.cuDeviceGetName(name, len(name), 0)
    return {"gpu": name.value.decode(errors="replace"), "cuda_driver": version.value,
            "gpu_count": count.value}


def install_runtime():
    target = runtime_dir()
    target.mkdir(parents=True, exist_ok=True)
    if shutil.disk_usage(target).free < 4 * 1024**3:
        raise RuntimeError("Cần ít nhất 4 GB trống tại ổ dữ liệu AIR3view để cài CUDA; model large-v3-turbo cần thêm khoảng 1,6 GB.")
    root = Path(__file__).resolve().parent.parent
    uv = root / "tools" / "uv.exe"
    if not uv.is_file():
        raise RuntimeError("Thiếu tools/uv.exe. Giải nén đầy đủ bản AIR3view Windows để chuẩn bị CUDA.")
    print("Đang tải thư viện CUDA riêng cho Whisper (~568 MB). Có thể hủy và thử lại.", file=sys.stderr, flush=True)
    flags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
    subprocess.run([str(uv), "pip", "install", "--no-config", "--no-cache", "--no-deps", "--reinstall",
                    "--only-binary", ":all:", "--require-hashes", "--python", sys.executable,
                    "--target", str(target), "-r", str(root / "workers" / "requirements-cuda.txt")],
                   check=True, creationflags=flags)
    pending = target / f".ready.{os.getpid()}.tmp"
    pending.write_text(RUNTIME_ID, encoding="ascii")
    pending.replace(target / ".ready")


def prepare(install=False):
    info = driver_info()  # Reject incompatible hardware before any large download.
    from importlib.metadata import version
    backend = version("ctranslate2")
    if backend != BACKEND_VERSION:
        raise RuntimeError(f"CTranslate2 {backend} khác bản được hỗ trợ {BACKEND_VERSION}. Chạy lại setup-workers hoặc dùng Python đi kèm app.")
    directory = runtime_dir() / "nvidia" / "cublas" / "bin"
    if not runtime_installed(directory):
        if not install:
            raise RuntimeError("Chưa có thư viện CUDA riêng cho Whisper. Bấm Chuẩn bị CUDA trong Cài đặt kết nối; lần chạy CUDA đầu cũng tự chuẩn bị.")
        with installation_lock():
            if not runtime_installed(directory):
                install_runtime()
    if not runtime_installed(directory):
        raise RuntimeError("Cài CUDA chưa hoàn tất. Bấm Chuẩn bị CUDA để thử lại.")
    _dll_directories.append(os.add_dll_directory(str(directory)))
    try:
        _libraries.append(ctypes.WinDLL(str(directory / "cublasLt64_12.dll")))
        library = ctypes.WinDLL(str(directory / "cublas64_12.dll"))
        _libraries.append(library)
        handle = ctypes.c_void_p()
        library.cublasCreate_v2.argtypes = [ctypes.POINTER(ctypes.c_void_p)]
        library.cublasDestroy_v2.argtypes = [ctypes.c_void_p]
        status = library.cublasCreate_v2(ctypes.byref(handle))
        if status:
            raise RuntimeError(f"cuBLAS không khởi tạo được (status={status}); kiểm tra VRAM và driver NVIDIA.")
        library.cublasDestroy_v2(handle)
    except OSError as exc:
        raise RuntimeError("Không nạp được cuBLAS 12.8. Thử Chuẩn bị CUDA, kiểm tra driver và Microsoft Visual C++ Runtime x64.") from exc
    import ctranslate2
    if ctranslate2.get_cuda_device_count() < 1:
        raise RuntimeError("CTranslate2 chưa nhận GPU CUDA dù driver đã sẵn sàng. Kiểm tra driver hoặc chọn CPU.")
    info.update(backend=backend, cublas=CUBLAS_VERSION,
                supported_compute_types=sorted(ctranslate2.get_supported_compute_types("cuda", 0)),
                runtime=str(directory))
    return info


def select_compute(device, requested, supported):
    choices = (["int8_float16", "int8_float32", "float16", "float32"] if device == "cuda"
               else ["int8", "int8_float32", "float32"])
    if requested != "auto":
        if requested not in supported:
            raise RuntimeError(f"{device} không hỗ trợ {requested}. Chọn Tự động; hỗ trợ: {', '.join(sorted(supported))}.")
        return requested
    for choice in choices:
        if choice in supported:
            return choice
    raise RuntimeError(f"Không tìm thấy kiểu tính toán phù hợp cho {device}.")


def error_message(exc, device):
    message = str(exc)
    lower = message.lower()
    if device == "cuda" and ("out of memory" in lower or "cuda_error_out_of_memory" in lower):
        return ("CUDA thiếu VRAM. Chọn Tự động / INT8, đóng ứng dụng dùng GPU hoặc chọn model small/CPU. "
                "App không tự chuyển sang CPU khi bạn chọn CUDA. Chi tiết: " + message)
    if any(word in lower for word in ("cublas", "cudnn", "cuda driver", "cuda failed")):
        return "CUDA chưa sẵn sàng. Kiểm tra / Chuẩn bị CUDA trong Cài đặt kết nối. Chi tiết: " + message
    return f"Whisper {device}: {message}"
