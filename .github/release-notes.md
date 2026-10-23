## Tải và chạy trên Windows 10/11 x64

1. Tải **AIR3view-…-windows-x64.zip** và giải nén toàn bộ vào thư mục có quyền ghi.
2. Mở **Start-AIR3view.cmd**. Lần đầu app tự tải Python 3.12 và cài bộ worker đã khóa phiên bản, không cần cài Python/Node thủ công. Có thể chạy **Setup-AIR3view.cmd** để cài trước. Cần mạng và khoảng 5–10 GB trống cho runtime, model, cache/video.
3. Model giọng/ASR tải khi sử dụng lần đầu. Vào **Cài đặt kết nối**: người dùng tự cấu hình Gemini API key hoặc Antigravity CLI đã đăng nhập. Key/phiên đăng nhập/cookie của tác giả không được phân phối.
4. Nhân vật **2D local** dùng Rhubarb/Pillow, không cần HeyGen. Nếu chọn HeyGen, tự cấu hình tài khoản/key và số dư.

Gói kèm FFmpeg/ffprobe, yt-dlp, Node.js, uv, Rhubarb và giấy phép. Không dùng GPU/CUDA theo mặc định; CUDA là cấu hình nâng cao tùy máy. SmartScreen có thể hỏi xác nhận vì EXE hiện chưa ký chứng thư phát hành.

## Thông báo cập nhật

App kiểm tra GitHub ở luồng nền khi mở và mỗi 15 phút, có nút kiểm tra thủ công ở **Cập nhật** cuối menu trái. Có code mới sẽ hiện thông báo; khi build/release hoàn thành sẽ có nút tải ZIP. Giải nén bản mới vào thư mục mới rồi mở launcher. Dự án/cấu hình trong `%LOCALAPPDATA%\ReactionStudio` được giữ lại. Runtime Python/model có thể tải lại tùy bản và cache trên máy.

Mỗi lần push `main`, GitHub Actions chạy test, build, kiểm tra xuất video rồi mới công bố bản tải mới. Chỉ tải asset từ repo chính thức; đối chiếu SHA256 với `SHA256SUMS.txt` khi cần.

File **source.zip** dành cho phát triển, không phải bản chạy. Chi tiết workflow và thông báo license nằm trong README và THIRD_PARTY_NOTICES của gói.
