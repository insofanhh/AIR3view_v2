## AIR3view 0.2.14

### Che chữ và logo nguồn không dùng AI

- Thêm dò phụ đề nguồn, logo/watermark và thông số camera bằng xử lý ảnh cục bộ; bật/tắt từng loại, độ blur và viền che. Có thể nhập mẫu logo và chỉnh vùng che thủ công.
- Sửa lỗi bỏ sót chữ có viền dày, gồm nhãn OFFICER và phụ đề viền xanh. Quét chưa đầy đủ được tiếp tục thay vì dùng mãi kết quả thiếu; tận dụng proxy và ảnh mẫu đã lưu để giảm công việc quét lại.
- Hiển thị trạng thái quét và số vùng che có hiệu lực tại vị trí xem. Preview, MP4, ảnh dẫn chứng và nền lấy từ nguồn dùng cùng thông số vùng che, theo đúng thời gian nguồn sau thay đổi tốc độ/crop/lật.
- Bộ dò dùng quy tắc hình ảnh nên có thể bỏ sót hoặc đề xuất nhầm. Kiểm tra preview; bật đề xuất hoặc vẽ vùng che khi cần. MP4 đã xuất cần xuất lại để nhận thay đổi.

### Bố cục, kịch bản và thiết lập

- Bố cục video phía trên với ảnh nền upload; crop riêng bốn cạnh, vị trí/zoom và lưu thiết lập layer cho dự án tiếp theo.
- Luồng tải nguồn → transcript + ảnh timestamp → trích sự kiện → viết kịch bản; không gọi Gemini đọc URL ở bước tải. Văn phong US được hướng dẫn trong prompt, không tạo vòng sửa riêng cho cảm xúc avatar; nhân vật 2D tiếp tục nhép lời.
- Khi tắt nhân vật, ưu tiên footage/cue nguồn và chỉ giữ phần Host ngắn. Cải thiện căn thời lượng, lựa chọn ảnh dẫn chứng, lưu vị trí nền/video/sub và khả năng tiếp tục công việc.
- Giữ chức năng xuất duy nhất final.mp4, âm thanh thẻ, thiết lập nhập/xuất và cập nhật tự động từ các bản trước.

**Kiểm thử local:** 273 bài kiểm thử Rust đạt, 4 bài dịch vụ ngoài bỏ qua; Clippy không cảnh báo. Đã kiểm tra dự án thực tế, preview và MP4 1080×1920 tại cảnh từng bị bỏ sót; nhãn/phụ đề gốc được che, PCM âm thanh giữ nguyên. GitHub Actions tiếp tục kiểm thử release, cập nhật/khôi phục, gói cài, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong AIR3view. Cài mới: tải **AIR3view-0.2.14-windows-x64.zip**, giải nén đầy đủ rồi mở **AIR3view.exe**. Dự án/cấu hình vẫn lưu theo máy; cookie, key và dữ liệu dự án không nằm trong gói phát hành. File **source.zip** dành cho mã nguồn.
