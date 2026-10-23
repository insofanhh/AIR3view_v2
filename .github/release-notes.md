## AIR3view 0.2.2: Whisper CUDA và large-v3-turbo

- Chọn model **large-v3-turbo** cùng **CPU** hoặc **CUDA / NVIDIA** ở phần Whisper chung cho Gemini/Antigravity và căn phụ đề.
- Thêm **Kiểm tra CUDA / Chuẩn bị CUDA**, tải thư viện NVIDIA chính thức có kiểm SHA256 vào thư mục dữ liệu riêng. Không cần cài CUDA Toolkit hoặc chép DLL thủ công.
- CUDA cần driver NVIDIA hỗ trợ **CUDA 12.8+** (Windows **572.61+**). Tự động ưu tiên INT8 để giảm VRAM; có FP16 và các kiểu tính toán thủ công. Lỗi driver, thư viện hoặc thiếu VRAM được báo rõ; không tự chuyển sang CPU âm thầm.
- Transcript mới kèm báo cáo model, thiết bị, kiểu tính toán thực tế và thời gian. Dùng lại transcript đã cache, giữ dữ liệu dự án cũ.
- Bổ sung kiểm thử nhận dạng batch thật, tự nhận diện ngôn ngữ và timestamp trước khi phát hành.
- Đã kiểm thử GPU thật trên GTX 1650 Max-Q 4 GB / driver 621.01: `large-v3-turbo` chạy CUDA với INT8-FP16 và FP16, tiếng Anh chọn sẵn/tự nhận diện, cùng audio tiếng Việt. Khuyến nghị **Tự động / INT8** trên GPU 4 GB.

## Tải và chạy trên Windows 10/11 x64

1. Tải **AIR3view-…-windows-x64.zip** và giải nén toàn bộ vào thư mục có quyền ghi.
2. Mở **AIR3view.exe**. App hiển thị tiến trình chuẩn bị môi trường trong cửa sổ riêng, tự tải Python 3.12 và cài bộ worker đã khóa phiên bản, không cần mở CMD hay cài Python/Node thủ công. Sau lần đầu thành công, mở bằng icon **AIR3view** trên Desktop hoặc Start Menu. Cần mạng và khoảng 5–10 GB trống cho runtime, model, cộng thêm dung lượng video.
3. Model giọng/ASR tải khi sử dụng lần đầu. Vào **Cài đặt kết nối**: người dùng tự cấu hình Gemini API key hoặc Antigravity CLI đã đăng nhập. Key/phiên đăng nhập/cookie của tác giả không được phân phối.
4. Nhân vật **2D local** dùng Rhubarb/Pillow, không cần HeyGen. Nếu chọn HeyGen, tự cấu hình tài khoản/key và số dư.

Gói kèm FFmpeg/ffprobe, yt-dlp, Node.js, uv, Rhubarb và giấy phép. Không dùng GPU/CUDA theo mặc định; CUDA là cấu hình nâng cao tùy máy. SmartScreen có thể hỏi xác nhận vì EXE hiện chưa ký chứng thư phát hành.

## Thông báo cập nhật

App kiểm tra GitHub ở luồng nền khi mở và mỗi 15 phút, có nút kiểm tra thủ công ở **Cập nhật** cuối menu trái. Có code mới sẽ hiện thông báo; khi build/release hoàn thành sẽ có nút tải ZIP. Giải nén bản mới vào thư mục mới rồi mở EXE mới một lần để cập nhật shortcut. Dự án/cấu hình trong `%LOCALAPPDATA%\ReactionStudio` được giữ lại. Runtime Python/model có thể tải lại tùy bản và cache trên máy.

Mỗi lần push `main`, GitHub Actions chạy test, build, kiểm tra xuất video rồi mới công bố bản tải mới. Chỉ tải asset từ repo chính thức; đối chiếu SHA256 với `SHA256SUMS.txt` khi cần.

File **source.zip** dành cho phát triển, không phải bản chạy. Chi tiết workflow và thông báo license nằm trong README và THIRD_PARTY_NOTICES của gói.
