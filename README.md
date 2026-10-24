# AIR3view

Ứng dụng Windows native viết bằng Rust + egui, dùng FFmpeg để xử lý/xuất video. Phiên bản 0.2.4. Preview, timeline và các layer nằm ngay trong app.

## Khởi chạy

**Trên máy phát triển:** chạy EXE trong `target/x86_64-pc-windows-msvc/release` sau khi build, hoặc EXE trong gói sau khi đóng gói. Mỗi bản dùng Python trong `.venv` của chính thư mục runtime đó.

**Trên máy khác (Windows 10/11 x64):**

1. Tải ZIP Windows từ [GitHub Releases](https://github.com/insofanhh/AIR3view_v2/releases/latest), giải nén toàn bộ vào thư mục có quyền ghi.
2. Bấm đúp **`AIR3view.exe`**. App hiển thị tiến trình chuẩn bị trong cửa sổ native: tự tải Python 3.12, cài worker đã khóa phiên bản bằng uv và chờ VieNeu sẵn sàng; không cần Python/Node cài sẵn hoặc quyền quản trị. Có nút Thử lại và mở thư mục nhật ký nếu gặp lỗi. Việc tải/cài chạy ở luồng nền.
3. Cần mạng và khoảng 5–10 GB trống cho runtime, model và media. Model giọng/ASR tải khi sử dụng lần đầu. Key/phiên đăng nhập AI là cấu hình riêng của người sử dụng.
4. Vào **Cài đặt kết nối** để chọn Gemini API hoặc Antigravity CLI đã đăng nhập. Chọn **2D local** nếu không dùng HeyGen.

Giữ EXE cùng workers/tools/DLL. Gói kèm Node.js để tải YouTube. `studio-cli.exe doctor` kiểm tra runtime.

Sau lần khởi chạy thành công đầu tiên, app tạo icon **AIR3view** trên Desktop và Start Menu. Những lần sau mở bằng icon; shortcut trỏ trực tiếp đến EXE, không mở cửa sổ CMD. Khi giải nén bản mới, mở EXE mới một lần để cập nhật shortcut. Giữ thư mục app ở vị trí ổn định sau đó.

App dùng lại VieNeu đang chạy nếu API hỗ trợ giọng tham chiếu và Việt/Anh (protocol 3). Worker do chính phiên app tạo sẽ dừng cùng app, kể cả khi tiến trình GUI bị tắt đột ngột; app không dừng worker của phiên khác. Model giọng được nạp khi tạo giọng lần đầu, nên mở editor/thư viện không phải chờ nạp model. Chỉ một cửa sổ GUI mới được dùng cùng thư mục dữ liệu để tránh xung đột. Dịch vụ giọng từ xa được giữ nguyên theo cấu hình. `Start-AIR3view.cmd` là lối mở tương thích, chỉ chuyển tiếp sang EXE; `Setup-AIR3view.cmd` là công cụ cài runtime thủ công khi cần chẩn đoán. Nếu xóa shortcut, dùng **Cài đặt kết nối → Tạo icon Desktop / Start Menu** để tạo lại.

## Cập nhật và phát hành tự động

- App kiểm tra repo công khai ở luồng nền khi mở và mỗi 15 phút, không khóa UI. Có nút **Cập nhật** cuối menu trái (menu thu gọn: **UPD**), bật/tắt tự kiểm tra và kiểm tra thủ công.
- Có commit mới trên `main`: thông báo bản tải đang chuẩn bị. Khi release mới có ZIP Windows và SHA256: hiện nút **Cập nhật** cùng ghi chú phát hành. Bấm một lần để tải nền, kiểm tra SHA256 của ZIP và từng file trong manifest, tự thay file thay đổi/bổ sung file thiếu ngay trong thư mục app rồi mở lại. Cần mạng, dung lượng cho ZIP, file giải nén và bản sao khôi phục, cùng quyền ghi thư mục app.
- Chỉ cài khi tác vụ đã hoàn tất; tự lưu dự án/cài đặt trước khi đóng. Giữ dự án/cấu hình trong AppData, `.venv`, model, cookie và các file riêng ngoài gói. Python tự bổ sung thư viện khi bộ khóa thay đổi. Lỗi tải/kiểm tra không thay file hiện tại; lỗi cài sẽ khôi phục các file đã thay và mở lại bản cũ. Nhật ký/kết quả ở `%LOCALAPPDATA%\ReactionStudio\logs\update-last.json`; bản sao khi lỗi nằm trong `.air3view-update-…` của thư mục app.
- Máy dùng bản cũ chỉ có nút tải ZIP cần cài bản có tính năng này một lần. Những lần sau dùng nút **Cập nhật**, không cần giải nén thủ công. Khi mất mạng/GitHub giới hạn request, app vẫn hoạt động bình thường.
- `.github/workflows/release.yml`: mỗi push `main` chạy test/Clippy, build EXE với CRT tĩnh, đóng gói và kiểm tra render hai tỉ lệ trước khi công bố release. Cần GitHub Actions được bật và token workflow có `contents: write`.
- `release.json` ghi version/tag/commit/thời gian build; `package-manifest.json` chứa hash các file runtime. Release kèm `SHA256SUMS.txt` cho ZIP. Bản đầu dùng tag theo Cargo, những lần tiếp theo dùng hậu tố `-build.<số workflow>`.

## Dịch vụ

- AI: Antigravity CLI hỗ trợ JSON hoặc Gemini API qua biến môi trường `GEMINI_API_KEY`. Phân tích URL có thể chạy song song với tải; tùy khả năng provider, app chuyển sang transcript + ảnh sau tải.
- Giọng: VieNeu tại `http://127.0.0.1:8765`, model `pnnbao-ump/VieNeu-TTS-v3-Turbo`, có preset và audio tham chiếu. Kịch bản hỗ trợ Tiếng Việt/English; cấu hình giọng cần phù hợp với ngôn ngữ và runtime.
- Nhân vật: 2D local dùng Rhubarb + Pillow hoặc HeyGen Avatar IV qua `HEYGEN_API_KEY`. Nhánh HeyGen đã có kiểm thử hợp đồng bằng giả lập; cần key/số dư để kiểm tra dịch vụ thực tế.
- YouTube: chọn cookie Netscape `.txt` trong Cài đặt kết nối khi nguồn yêu cầu đăng nhập. Cookie phải còn hiệu lực và tài khoản có quyền xem; chỉ truyền khi tải, không gửi cho AI.

## Whisper: CPU / CUDA và large-v3-turbo

Trong **Cài đặt kết nối → Nhận dạng tiếng nói / Whisper**, chọn model `base`, `small` hoặc `large-v3-turbo`, rồi chọn **CPU** hoặc **CUDA / NVIDIA**. Dùng chung cho phân tích transcript và căn từ phụ đề, với cả Gemini và Antigravity. **Lưu cấu hình** sau khi chọn.

- **CPU:** không tải CUDA; Tự động dùng INT8. `large-v3-turbo` chạy trên CPU nhưng chậm hơn GPU và cần đủ RAM.
- **CUDA:** cần Windows x64, GPU NVIDIA và driver hỗ trợ CUDA **12.8 trở lên** (Windows **572.61+**). GPU không tương thích hoặc driver cũ sẽ được báo trước khi tải model/thư viện. Bấm **Kiểm tra CUDA** để kiểm tra driver, GPU, nạp cuBLAS và các kiểu tính toán thực tế.
- **Chuẩn bị CUDA:** tự tải wheel cuBLAS **12.8.4.1** chính thức từ NVIDIA/PyPI, kiểm SHA256 và cài riêng trong `<dữ liệu AIR3view>/runtime/asr-cuda12-v1`. Lần chạy CUDA đầu cũng tự chuẩn bị nếu thiếu. Cần mạng, ít nhất **4 GB trống** để cài thư viện, cộng thêm khoảng **1,6 GB** nếu chưa có model. Có thể hủy và thử lại. Không cần cài CUDA Toolkit hay sửa PATH hệ thống.
- Backend khóa **CTranslate2 4.8.3**; wheel Windows phiên bản này dùng Conv1D CUDA trực tiếp, không cần tải cuDNN. Không đổi backend tùy ý theo hướng dẫn cuDNN của các phiên bản khác.
- **Tự động** ưu tiên `int8_float16` trên GPU hỗ trợ, giảm VRAM so với FP16. Có thể chọn FP16 thủ công khi đủ VRAM. Nếu thiếu VRAM, app báo rõ và giữ nguyên thiết bị đã chọn, không tự chạy CPU âm thầm. GPU 4 GB cần đóng ứng dụng GPU khác và kiểm tra với video thực tế.
- Mỗi transcript mới có file `*.asr.json` cạnh file từ, ghi thiết bị, model, kiểu tính toán thực tế và thời gian; transcript đã cache được dùng lại. Nút kiểm tra CUDA chỉ kiểm tra runtime, không thay thế kiểm thử nhận dạng model.

Bản 0.2.2 đã kiểm thử nhận dạng thật với `large-v3-turbo` trên GTX 1650 Max-Q 4 GB, driver 621.01: CUDA INT8-FP16 và FP16, batch tiếng Anh chọn sẵn/tự nhận diện, cùng audio tiếng Việt. Trên máy này nên chọn **Tự động / INT8**; thời gian và VRAM thực tế thay đổi theo video và ứng dụng đang chạy.

Kiểm thử nhận dạng thật trong thư mục source đã chuẩn bị Python (có thể truyền thư mục gói vào `-PackagePath`):

```powershell
.\scripts\verify-asr.ps1 -PackagePath . -Device cpu -Model large-v3-turbo
.\scripts\verify-asr.ps1 -PackagePath . -Device cuda -Model large-v3-turbo
```

Script tạo audio tiếng Anh thật, nhận dạng batch với ngôn ngữ cố định/tự động và xác minh nội dung, timestamp, model, thiết bị. CI Windows thông thường chỉ kiểm thử CPU; kiểm thử CUDA phải chạy trên GPU NVIDIA thật trước khi công bố hỗ trợ GPU đã được xác minh.

## Workflow

1. **Dự án mới → Biên tập video → Thiết lập dự án**.
2. **01 / Kịch bản:** ngôn ngữ, Độ dài, prompt, quy tắc giữ/loại cảnh, hook và các đoạn gốc xen kẽ.
3. **02 / Hình ảnh:** 9:16/16:9, độ phân giải, nền mờ, vị trí/kích thước nhân vật, dẫn chứng và crop ảnh theo % mỗi cạnh.
4. **03 / Giọng, nhân vật & âm thanh:** chọn giọng/audio tham chiếu, 2D hoặc HeyGen; bật tiếng gốc nền và chỉnh âm lượng.
5. **04 / Phụ đề:** font, size, màu/highlight/viền, vị trí X/Y và khoảng cách đáy.
6. **05 / Video nguồn:** thêm URL YouTube (mỗi dòng một link) hoặc video local. Bấm **Tạo review tự động** để phân tích, tạo kịch bản, giọng, avatar và phụ đề. Thiết lập lưu làm mẫu cho dự án sau.
7. Duyệt cảnh/timeline, chỉnh layer và xem preview. Bấm **Xuất MP4**, chọn thư mục lần đầu; app nhớ nơi lưu. Kết quả: `<thư mục>/AIR3view-<ID>/final.mp4` cùng phụ đề/thông tin nguồn. Đổi ổ tại **Nơi lưu → Đổi thư mục…**.

Tác vụ có checkpoint. Sau lỗi/mở lại app, dùng **Tiếp tục bước dang dở** để dùng lại tài nguyên đã tạo. Đổi nguồn hoặc yêu cầu kịch bản thì tạo mới.

## Biên tập

- Timeline có trim, split, sắp xếp cảnh/layer, ẩn/khóa, volume/fade và Undo/Redo. Chỉnh layer/preview không cần encode lại MP4.
- Thanh công cụ timeline nằm trên một hàng; cửa sổ hẹp có thể cuộn ngang. Nút **SFX** cho phép bật/tắt, chọn Tự động / Whoosh / Pop / Ting và chỉnh âm lượng thẻ thông tin **0–200%**; âm lượng ảnh dẫn chứng có mục riêng. Thiết lập được nhớ cho dự án sau. Âm thanh theo thời điểm thẻ xuất hiện, hỗ trợ preview và xuất MP4; chỉ đổi âm lượng sẽ dùng lại cache hình.
- **Cảnh → LAYER → Đồng bộ thông số với các layer cùng loại**: lấy layer đang chọn làm mẫu; bỏ qua layer/nhóm/cảnh khóa. Hình đồng bộ transform/thứ tự, âm thanh đồng bộ gain/fade. Nội dung, nguồn và thời gian clip giữ riêng.
- Nhân vật kéo thả trong preview hoặc nhập X/Y. Vị trí sub X/Y điều chỉnh ±2000 px theo kích thước đầu ra.
- Thông số layer tự được nhớ khi lưu/tự lưu, tạo dự án mới hoặc đóng app: X/Y, kích thước, xoay, độ hiện, thứ tự, âm lượng/fade, co giãn tự do và tick đồng bộ. Dự án sau dùng lại theo loại layer và cảnh Host/review riêng; các ảnh dẫn chứng giữ vị trí thứ tự riêng. Media, nội dung, timing, trạng thái khóa/ẩn không được chép sang dự án mới.
- Ảnh/số liệu dẫn chứng xuất hiện sau Host, có bố cục/chuyển động/SFX. Khi đổi crop %, dùng **Tạo thẻ / cập nhật ảnh dẫn chứng**. Số liệu cần đối chiếu với nguồn.
- Tiếng gốc nền lấy đoạn review đang chiếu; hook/đoạn gốc xen kẽ có gain riêng. Cảnh Host không tự lấy audio từ đoạn khác.
- Cache hình được dùng lại khi chỉ sửa âm thanh/metadata. Khi xuất, các thay đổi hình được dựng theo phần bị ảnh hưởng; tốc độ phụ thuộc máy và nội dung.
- **Nơi lưu / metadata → Metadata video…**: preset AIR3view, mô phỏng CapCut Desktop hoặc tùy chỉnh. Preset mô phỏng không xác nhận video xuất từ CapCut. Có thể đọc/áp metadata vào MP4 có sẵn bằng mux, không encode hình/tiếng.

## Dữ liệu và log

Dự án/cấu hình: `%LOCALAPPDATA%\ReactionStudio` (giữ tên cũ để tương thích). Log worker nằm trong `logs/vieneu-out.log`, `logs/vieneu-err.log`. Cache model VieNeu dùng file thường trong `runtime/huggingface-regular-v1` để ONNX hoạt động cả khi Windows bật Developer Mode; có thể đổi bằng biến `REACTION_MODEL_CACHE`. Dự án chứa `project.json`, `workflow.json`, `logs/pipeline.jsonl`, media/cache. Xóa dự án qua nút trong Thư viện; dọn source/build không cần xóa AppData.

## Build và đóng gói

Cần Windows x64, 7-Zip (giải nén công cụ khi build từ source), Rust theo `rust-toolchain.toml`, Visual Studio Build Tools với Desktop development with C++ và Windows SDK. `scripts/build.cmd` tự tìm MSVC bằng vswhere.

```powershell
.\scripts\build.cmd
.\scripts\build.cmd clippy --release --all-targets --locked -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package.ps1 -SkipBuild -Archive
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-source.ps1
```

- Bản chạy: `dist/AIR3view/`; ZIP: `dist/AIR3view-0.2.4-windows-x64.zip`. `scripts/package.ps1 -OutputPath <thư mục mới>` cho phép đóng gói riêng khi bản cũ còn runtime local.
- Source sạch: `dist/AIR3view-0.2.4-source.zip`, không chứa cache/build, Python, model, video dự án, cookie hoặc executable công cụ.
- Package dùng danh sách runtime và ghi SHA256 trong `package-manifest.json`. Không sinh bản EXE dự phòng. Có file lạ ở thư mục đích thì script báo lỗi để tránh đưa nhầm vào release.
- Giữ `Cargo.lock`. Unit test nằm trong module Rust, chỉ biên dịch khi chạy `scripts/build.cmd test --release --lib --locked`; không nằm trong EXE release.
- `.gitignore` loại build/dist/runtime/dữ liệu local. Không commit key, cookie hay cấu hình cá nhân.

Khi clone source: chạy `scripts/setup-tools.ps1` để tải đúng phiên bản và xác nhận SHA256 theo `tools/runtime-lock.json`; chạy `scripts/setup-workers.ps1` nếu cần TTS/ASR. Giữ giấy phép trong `tools/`. Trước khi đóng gói release, commit source để ghi đúng SHA vào `release.json`.

### Dọn artifact local

`scripts/clean-workspace.ps1` mặc định chỉ xem danh sách dung lượng; thêm `-Apply` để xóa video/file kiểm thử còn lại và Python bytecode. Thêm `-IncludeLegacyPackage` để xóa bản đóng gói cũ `dist/ReactionStudio` sau khi đóng app cũ. Môi trường `.venv`, công cụ tải về và dữ liệu AppData được giữ lại. Dọn cache Rust riêng bằng `cargo clean` (lần build sau sẽ biên dịch lại).

## CLI

```text
studio-cli doctor
studio-cli download <YouTube URL> <folder>
studio-cli auto-run <project.json>
studio-cli auto-resume <project.json>
studio-cli prepare <project.json>
studio-cli script-only|speech|assets|captions|evidence <project.json>
studio-cli repair-script <project.json> <saved response>
studio-cli render <project.json> <output.mp4>
studio-cli metadata-inspect <input.mp4>
studio-cli metadata-apply <project.json> <input.mp4> <new-output.mp4>
studio-cli heygen-preflight
```

Xem `THIRD_PARTY_NOTICES.md`, `WHATLANG-LICENSE.txt` và giấy phép trong `tools/` trước khi phân phối.
