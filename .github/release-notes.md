## AIR3view 0.2.5: xử lý hàng loạt, mỗi URL một dự án

- Thêm chế độ **Hàng loạt · mỗi URL một dự án**: dán nhiều URL hoặc nhập file `.txt`, lọc link trùng, bỏ chọn từng URL và dùng chung bản chụp thiết lập. Chế độ nhiều nguồn trong một dự án vẫn có sẵn.
- Hàng đợi chạy lần lượt, mỗi dự án có checkpoint riêng; hỗ trợ tạm dừng, tiếp tục, dừng tác vụ hiện tại, đổi thứ tự, bỏ lượt và thử lại mục lỗi. Một URL lỗi riêng không chặn các URL sau; lỗi cấu hình/tài nguyên chung sẽ tạm dừng đợt.
- Tùy chọn **Tự xuất MP4**, thư mục riêng cho từng dự án, kiểm tra kết quả xuất và dùng lại MP4 hợp lệ. Không chạy lại các mục đã hoàn thành khi tiếp tục.
- Tiến trình nặng chạy ngoài GUI; đóng app dừng cây tiến trình do hàng đợi khởi tạo. Bảo vệ dự án đang chạy và chặn cài cập nhật trong khi hàng đợi hoạt động.
- Ảnh dẫn chứng hiển thị đầy đủ, lớn hơn trong card; phân bổ thời điểm xuất hiện trong cảnh. Card sau chồng lên card trước, các card tiếp tục chuyển động nhẹ; card trước được làm mờ để giữ trọng tâm.
- Âm thanh dẫn chứng mặc định **Whoosh**, có chọn kiểu tiếng và âm lượng riêng. Sửa lệch thời điểm SFX sau căn thời lượng giọng và tiếng phát dư cuối video.
- Đã kiểm thử 3 URL thực tế từ tải nguồn, phân tích, tạo giọng/nhân vật đến MP4 9:16; kiểm thử hàng đợi dừng/tiếp tục/thử lại và giao diện cửa sổ hẹp. Bộ kiểm thử Rust: **111 thành công, 1 bỏ qua**, Clippy không có cảnh báo.

**Cập nhật từ 0.2.3/0.2.4:** mở **Cập nhật → Cập nhật** để tự tải, cài và mở lại. Dự án và cấu hình được giữ lại. Bản 0.2.2 trở xuống cần tải ZIP Windows và giải nén toàn bộ một lần để có updater mới.

### Tính năng từ 0.2.4: timeline một hàng và sound effect cho thẻ thông tin

- Gom nút biên tập, phát/dừng, thời gian, Snap, SFX và zoom vào một hàng ngang với chiều cao nút đồng đều. Cửa sổ hẹp hỗ trợ cuộn ngang.
- Nút **SFX** trên timeline: bật/tắt âm thanh thẻ thông tin, chọn **Tự động / Whoosh / Pop / Ting**, chỉnh âm lượng **0–200%**. Có điều chỉnh âm lượng riêng cho ảnh dẫn chứng.
- Nhớ thiết lập âm thanh cho dự án sau; áp dụng trong preview và MP4. Tiếng vào thẻ bám theo timing, ẩn/mute và cắt cảnh không phát lại tiếng giữa thẻ.
- Sửa fade tính theo khối âm thanh làm mất phần đầu tiếng pop ngắn; chuyển sang tính theo từng mẫu âm thanh.
- Đổi âm lượng/tắt SFX dùng lại cache hình; không cần tạo lại giọng hoặc avatar.
- Kiểm thử giao diện, điều khiển âm thanh trực tiếp và MP4 thật: timing, mức âm lượng, mute và dùng lại cache hình.

**Người đang dùng 0.2.3:** mở **Cập nhật → Cập nhật** để tự tải, cài và mở lại. Bản 0.2.2 trở xuống cần tải ZIP Windows và giải nén toàn bộ một lần để có updater mới.

### Tính năng từ 0.2.3: cập nhật bằng một nút và nhớ cấu hình layer

- Khi có Release mới, bấm **Cập nhật**: app tự tải ZIP chính thức, kiểm SHA256 và manifest, bổ sung file thiếu/thay file thay đổi trong thư mục hiện tại rồi mở lại.
- Tự lưu trước khi đóng, chờ tác vụ đang chạy hoàn tất. Giữ dự án, cài đặt, cookie, model và môi trường Python; bổ sung thư viện khi bộ khóa thay đổi.
- Lỗi tải/kiểm tra giữ bản đang dùng. Lỗi cài đặt khôi phục file cũ và mở lại bản cũ; hiển thị kết quả trong cửa sổ cập nhật. Có kiểm thử EXE thật, file khóa, ZIP lỗi, đường dẫn Unicode và khôi phục một phần trong CI.
- Nhớ thông số layer cho dự án tiếp theo, tách bố cục cảnh Host/review và từng ảnh dẫn chứng.

**Lưu ý chuyển phiên bản:** người đang dùng 0.2.2 trở xuống cần tải/giải nén bản có updater này một lần. Sau đó dùng nút **Cập nhật**, không cần tải ZIP thủ công nữa.

### Whisper CUDA và large-v3-turbo

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

App kiểm tra GitHub ở luồng nền khi mở và mỗi 15 phút, có nút kiểm tra thủ công ở **Cập nhật** cuối menu trái. Có code mới sẽ hiện thông báo; khi build/release hoàn thành sẽ có nút **Cập nhật** để tự tải, kiểm tra, cài và mở lại ngay trong thư mục hiện tại. Dự án/cấu hình trong `%LOCALAPPDATA%\ReactionStudio` được giữ lại. Thư mục app cần có quyền ghi và đủ chỗ cho gói tải cùng bản sao khôi phục.

Mỗi lần push `main`, GitHub Actions chạy test, build, kiểm tra xuất video rồi mới công bố bản tải mới. Chỉ tải asset từ repo chính thức; đối chiếu SHA256 với `SHA256SUMS.txt` khi cần.

File **source.zip** dành cho phát triển, không phải bản chạy. Chi tiết workflow và thông báo license nằm trong README và THIRD_PARTY_NOTICES của gói.
