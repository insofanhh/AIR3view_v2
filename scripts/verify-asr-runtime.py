"""CPU-run guards for CUDA setup. This is NOT a GPU inference test."""
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "workers"))
import cuda_runtime


class RuntimeGuards(unittest.TestCase):
    def test_low_vram_prefers_quantization_without_changing_device(self):
        self.assertEqual(cuda_runtime.select_compute("cuda", "auto", {"float16", "int8_float16"}), "int8_float16")
        self.assertEqual(cuda_runtime.select_compute("cuda", "auto", {"float32", "int8_float32"}), "int8_float32")
        self.assertEqual(cuda_runtime.select_compute("cpu", "auto", {"float32", "int8"}), "int8")
        with self.assertRaisesRegex(RuntimeError, "không hỗ trợ"):
            cuda_runtime.select_compute("cpu", "float16", {"int8", "float32"})

    def test_incompatible_driver_never_downloads_or_imports_backend(self):
        with patch.object(cuda_runtime, "driver_info", side_effect=RuntimeError("old driver")), \
                patch.object(cuda_runtime, "install_runtime") as install:
            with self.assertRaisesRegex(RuntimeError, "old driver"):
                cuda_runtime.prepare(install=True)
            install.assert_not_called()

    def test_interrupted_install_is_not_marked_ready(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "nvidia/cublas/bin"
            directory.mkdir(parents=True)
            for name in ("cublas64_12.dll", "cublasLt64_12.dll"):
                (directory / name).touch()
            with patch.object(cuda_runtime, "runtime_dir", return_value=root):
                self.assertFalse(cuda_runtime.runtime_installed(directory))
                (root / ".ready").write_text("old backend", encoding="ascii")
                self.assertFalse(cuda_runtime.runtime_installed(directory))
                (root / ".ready").write_text(cuda_runtime.RUNTIME_ID, encoding="ascii")
                self.assertTrue(cuda_runtime.runtime_installed(directory))

    def test_cpu_memory_failure_does_not_claim_cuda(self):
        self.assertIn("CUDA thiếu VRAM", cuda_runtime.error_message(RuntimeError("out of memory"), "cuda"))
        self.assertIn("Whisper cpu", cuda_runtime.error_message(RuntimeError("out of memory"), "cpu"))


if __name__ == "__main__":
    unittest.main()
