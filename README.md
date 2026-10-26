# AIR3view

Ứng dụng Windows native viết bằng Rust + egui, dùng FFmpeg để xử lý/xuất video. Phiên bản 0.2.9. Preview, timeline và các layer nằm ngay trong app.

## Xuất / nhập thiết lập giữa các máy

Trong **Biên tập video**, mở nút **Thiết lập** trên thanh công cụ:

1. **Xuất thiết lập…**: lưu các thông số hiện tại (kể cả chưa lưu dự án) thành một file `.air3preset`. Mặc định kèm ảnh nhân vật, giọng tham chiếu, nhạc và LUT đã chọn. File dùng container riêng của AIR3view, không cần giải nén.
2. Chuyển file sang máy khác, chọn **Thiết lập → Nhập thiết lập…**. Xem tóm tắt, chọn **Áp dụng cho dự án hiện tại** và/hoặc **Ghi nhớ cho các dự án tiếp theo**, rồi bấm **Nhập thiết lập**.
3. Nếu xuất không kèm tài nguyên, chọn lại đúng các file tương ứng trên máy nhận. App kiểm tra mã SHA-256 trước khi nhập. Tài nguyên được chép vào kho asset của app và đổi đường dẫn theo máy nhận.

Gói gồm prompt/quy tắc/thời lượng/ngôn ngữ, hook/cảnh xen kẽ, khung hình/tốc độ/màu/lật/nền mờ, voice/nhân vật, dẫn chứng/SFX, nhạc/tiếng gốc, phụ đề, mẫu thông số/hiệu ứng theo vai trò layer, mute/tick đồng bộ track, snapping và metadata. Khóa track giữ theo dự án đích. URL, mốc chọn nguồn, nội dung cảnh, file sinh ra và timing layer giữ riêng theo dự án; đổi tốc độ có thể điều chỉnh thời lượng cảnh gốc theo cơ chế khớp footage hiện có.

Kết nối API/CLI, key, cookie, đường dẫn công cụ và ổ lưu giữ theo máy nhận. Font không đóng gói; cần cài font hoặc chọn font khác nếu máy nhận chưa có. Khi đổi ngôn ngữ/giọng/nhân vật/kiểu dẫn chứng, chạy lại bước tạo nội dung tương ứng; nhập preset không tự gọi AI hoặc xuất video. Khi dự án/hàng loạt đang xử lý, chờ hoặc tạm dừng trước khi nhập.

Đọc/ghi/hash file chạy nền. Trước khi nhập, cấu hình cũ được sao lưu tại `preset-backups` trong thư mục dữ liệu app. Nếu lưu thất bại, app khôi phục JSON trước khi nhập. Mỗi tài nguyên tối đa 32 MB (ảnh), 64 MB (giọng mẫu), 256 MB (nhạc), 16 MB (LUT); tổng tối đa 512 MB.

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

## Nơi lưu video nguồn YouTube

Trong **Nơi lưu / metadata → Thư mục video nguồn YouTube → Đổi thư mục video nguồn…**, chọn ổ/thư mục muốn tải video gốc về. App ghi nhớ ngay cho dự án sau và các video hàng loạt tiếp theo. Mỗi dự án lưu vào `<thư mục đã chọn>/AIR3view-<ID>/sources/`, gồm video gốc và dữ liệu tải đi kèm. **Dùng thư mục dự án** trở về mặc định `<dự án>/sources/`.

Thiết lập này tách biệt với thư mục xuất MP4. Video đã tải và video nhập từ máy giữ nguyên vị trí; đổi nơi lưu không di chuyển chúng, không chạy lại AI/giọng. Proxy, giọng, avatar và cache vẫn nằm trong dự án. Ổ nguồn đã chọn không còn khả dụng sẽ được báo để kết nối lại hoặc chọn nơi khác. Video nguồn ngoài thư mục dự án được giữ lại khi xóa dự án.

## Nhạc nền, LUT, phản chiếu và chuẩn hóa nguồn

Trong **02 / Hình ảnh đầu ra**, bấm tiêu đề từng nhóm để đóng/mở độc lập: tốc độ, tiếng gốc, khung video, nhân vật, màu/lật và dẫn chứng. Các nhóm âm thanh thẻ/ảnh nằm gọn trong phần dẫn chứng. Đóng nhóm chỉ thu gọn giao diện, không tắt tính năng đã chọn.

Các mục **01 / Kịch bản**, **03 / Giọng, nhân vật & âm thanh**, **04 / Phụ đề** cũng có nhóm con đóng/mở riêng. Trong **Biên tập → Cảnh / Khung / Sub / Dẫn chứng**, bấm mũi tên ở tiêu đề để thu gọn vị trí layer, liên kết/âm lượng, nguồn cảnh, lời thoại, font/màu/vị trí sub hoặc nguồn/nội dung/chuyển động dẫn chứng. Mở/đóng nhóm giữ nguyên thông số đã nhập và không chạy lại pipeline.

- **Thiết lập dự án → 03 / Giọng, nhân vật & âm thanh → Nhạc nền độc lập:** chọn MP3/WAV/M4A/FLAC/OGG, bật/tắt, chỉnh âm lượng, điểm bắt đầu bài, fade vào/ra, lặp nhạc có nối chuyển tiếp và mức giảm nhạc dưới lời host. Có tùy chọn tắt nhạc trong hook/cảnh nghe tiếng gốc. Nhạc nằm trên track **Nhạc nền**, tách khỏi voice, tiếng gốc và SFX; có thể ẩn/tắt tiếng, khóa, cắt/tách và chỉnh timing. Preview chuẩn bị audio ở luồng nền khi phát; tua và xuất dùng cùng bản nhạc đã chuẩn bị. Ducking cập nhật theo các clip voice hiện có khi bắt đầu phát/xuất.
- File nhạc được sao chép vào asset của app, tối đa 256 MB, dài ít nhất 1 giây. Thông số và đường dẫn asset được nhớ cho dự án mới/phiên hàng loạt. Chuyển dự án sang máy khác cần mang theo các asset riêng, không chỉ `project.json`.
- **02 / Hình ảnh → Màu sắc & lật footage:** lật ngang/dọc, LUT Ấm nhẹ/Lạnh nhẹ/Tài liệu nhẹ hoặc nhập `.cube`, chỉnh cường độ. Mặc định chỉ tác động footage và nền video, giữ host/phụ đề/thẻ nguyên chiều. Lật sẽ đảo chữ nằm sẵn trong video nguồn. LUT nhập hỗ trợ bảng 3D 2–65, miền RGB 0–1, tối đa 16 MB; LUT không hợp lệ có thông báo lỗi.
- **Chọn layer → Màu & lật riêng layer → Ghi đè thiết lập dự án** để chỉnh từng layer. Bỏ ghi đè sẽ dùng lại thông số chung. Tick đồng bộ của track áp dụng các thay đổi màu/lật cho layer cùng loại có thể sửa; tôn trọng khóa layer/cảnh/nhóm. Tách/nhân bản giữ thiết lập màu riêng. Thông số màu/lật chung được nhớ cho các dự án sau; ghi đè theo ID thuộc dự án hiện tại.
- **05 / Video nguồn → Chuẩn hóa video đầu vào:** Tự động, Giữ nguyên, Đóng gói lại (remux) hoặc Chuyển mã H.264/AAC; tùy chọn tạo proxy nhẹ. Áp dụng khi tải/nhập nguồn hoặc bấm **Áp dụng cho nguồn hiện có**. Tự động kiểm tra khả năng giải mã, mốc bắt đầu và tín hiệu HDR; HDR được tone map về SDR BT.709. File gốc được giữ nguyên, bản chuẩn hóa/proxy nằm trong cache dự án; báo cáo tại `cache/normalized`. Remux giữ codec nên không thay thế chuyển mã khi codec/container không tương thích.
- Đổi nhạc/âm lượng không tạo lại AI, voice hoặc avatar. Đổi nhạc chỉ dựng lại phần audio, giữ cache hình đã xuất nếu hình ảnh không đổi. Màu/lật làm mất hiệu lực các đoạn hình bị tác động. Dự án cũ mặc định tắt nhạc/LUT/lật, các chỉ số track cũ được giữ nguyên.

## Tăng tốc footage và hook, giữ voice AI 1×

Mở **Thiết lập dự án → Hình ảnh đầu ra → Tốc độ footage gốc / hook** (hoặc bảng **Khung** khi biên tập).

- **Tắt:** mặc định 1×. **Tự động khớp voice:** chọn tốc độ ưu tiên/tối đa từ 1–2×; giảm tốc khi vùng nguồn không đủ. **Cố định:** giữ tốc độ đã chọn; báo lỗi khi thiếu footage hoặc mốc yêu cầu tốc độ khác.
- **Hook** và **Đoạn xen kẽ** có tốc độ riêng. Hook giữ tối thiểu 3 giây đầu ra, đoạn xen kẽ giữ mức tối thiểu đã đặt; tốc độ thực tế có thể thấp hơn mức chọn để giữ giới hạn này.
- Voice AI, avatar, phụ đề, thẻ và SFX theo thời gian đầu ra ở 1×. Hình nguồn, nền video mờ và tiếng gốc đi cùng tốc độ footage; tiếng gốc giữ cao độ bằng `atempo`.
- AI có thể trả mốc hành động kèm cụm từ nguyên văn. App căn cụm từ trên WAV thật rồi ánh xạ mốc nguồn sang thời điểm lời đọc. Mốc tự động xung đột/không tìm thấy được cảnh báo, không được coi là đã khớp; mốc tay không thể khớp sẽ báo lỗi để sửa.
- Chọn cảnh → **Tốc độ hình & mốc khớp lời** để đặt tốc độ riêng, chỉnh mốc nguồn hoặc mốc tay. Đổi tốc độ không tạo lại voice/avatar. Cảnh/track khóa được giữ nguyên. Thông số chung được nhớ cho dự án mới và sao chép vào phiên hàng loạt.
- Không lấy thêm hình vượt vùng đã chọn, vùng loại bỏ hay cuối video. Bật **Thiếu footage: cho phép chuyển Host** nếu muốn dùng nhân vật khi vùng nguồn ngắn hơn lời đọc; nếu tắt, app chỉ rõ cảnh cần mở rộng điểm cắt. Kiểm tra preview và cảnh báo trước khi xuất.

Bản đồ tốc độ, trạng thái mốc và thiết lập được lưu trong dự án; báo cáo pipeline tại `analysis/footage-speed-report.json`. Preview, tua, tách/cắt và xuất MP4 dùng cùng bản đồ thời gian.

## Lọc dự án và phiên hàng loạt theo ngày

- **Thư viện dự án:** chọn ngày tạo hoặc ngày cập nhật, rồi lọc Hôm nay, Hôm qua, 7/30 ngày gần đây, ngày cụ thể hoặc khoảng ngày. Ngày nhập theo **DD/MM/YYYY**, tính theo múi giờ của máy, gồm cả ngày bắt đầu và kết thúc. Danh sách xếp mới nhất trước; **Bỏ lọc** hiện lại tất cả.
- **Xử lý hàng loạt:** lọc theo ngày tạo phiên. Mỗi phiên có ngày giờ, mã riêng, tên, số video và trạng thái, giúp phân biệt các phiên trùng tên. Bộ chọn chỉ hiện các phiên khớp ngày; phiên đang chạy ngoài bộ lọc có nút mở nhanh. Tạo phiên mới sẽ hiện lại tất cả và chọn phiên vừa tạo.
- App nhớ riêng bộ lọc của hai màn hình và cách chọn ngày của thư viện. Ngày tạo dự án cũ dùng ngày tạo thư mục trên máy; sao chép thư mục sang máy khác có thể thay đổi ngày này. Lọc danh sách không làm thay đổi hàng đợi hay dữ liệu dự án.

## Dịch vụ

- AI: Antigravity CLI hỗ trợ JSON hoặc Gemini API qua biến môi trường `GEMINI_API_KEY`. Phân tích URL có thể chạy song song với tải; tùy khả năng provider, app chuyển sang transcript + ảnh sau tải.
- Giọng: VieNeu tại `http://127.0.0.1:8765`, model `pnnbao-ump/VieNeu-TTS-v3-Turbo`, có preset và audio tham chiếu. Kịch bản hỗ trợ Tiếng Việt/English; cấu hình giọng cần phù hợp với ngôn ngữ và runtime.
- Nhân vật: 2D local dùng Rhubarb + Pillow hoặc HeyGen Avatar IV qua `HEYGEN_API_KEY`. Nhánh HeyGen đã có kiểm thử hợp đồng bằng giả lập; cần key/số dư để kiểm tra dịch vụ thực tế.
- YouTube: chọn cookie Netscape `.txt` trong Cài đặt kết nối khi nguồn yêu cầu đăng nhập. Cookie phải còn hiệu lực và tài khoản có quyền xem; chỉ truyền khi tải, không gửi cho AI.

## Nhân vật 2D: cách tạo và chuyển động

**Hiện có:** bốn mẫu Chân dung/bán thân hoodie xám, Studio, Analyst và **Gấu trúc • nón rơm / kính đen / áo tím**. Chọn mẫu tại Cài đặt kết nối hoặc Thiết lập dự án → Giọng, nhân vật & âm thanh → 2D local. Gấu trúc toàn thân có áo choàng tím dài, vạt áo trắng, đai lưng, chuỗi hạt/tua và chân; nhép miệng, gật/nghiêng đầu, cử chỉ tay, chuyển động nhẹ của vạt áo và tua. Khung trong suốt 512×704 giữ đúng tỉ lệ, không kéo giãn nhân vật. Nút chọn ảnh nhân vật thuộc nhánh HeyGen; nhánh **2D local chưa có chức năng import PNG/PSD/Live2D/Spine tùy ý**. Không thay trực tiếp các file đầu bằng ảnh bất kỳ: vị trí miệng, tỉ lệ và khớp vai đang được hiệu chỉnh cho từng mẫu.

Nhánh local hoạt động như sau: **WAV giọng → Rhubarb phân tích khẩu hình → ghép miệng theo thời gian → rig vai/khuỷu/cổ tay và cử chỉ theo lời thoại → video nhân vật nền trong suốt → ghép vào timeline**. Tay/thân dùng các đường chuyển động được lập trình, không phải model AI tự đoán chuyển động từ ảnh. Đầu, miệng và cử chỉ được tính theo timestamp để tua preview và xuất video nhất quán. Hướng bàn tay bám theo cẳng tay, cổ tay chỉ gập nhẹ; quỹ đạo khuỷu giữ ngón tay trong khung.

Để chuẩn bị một nhân vật khác cho tích hợp vào renderer:

1. Vẽ hoặc tạo ảnh nhân vật **bán thân, nhìn chính diện, hai vai rõ, tay tách thân, nền trong suốt**, nên làm việc ở 1024×1024 trở lên; tránh cắt tóc/ngón tay và tránh tay che mặt.
2. Chuẩn bị PSD nhiều layer hoặc PNG riêng cho đầu/tóc, mắt mở/nhắm, lông mày, thân, cánh tay trên, cẳng tay và bàn tay trái/phải. Vẽ đủ phần bị che ở các khớp để khi xoay không lộ lỗ trống. Ảnh dẹt một lớp cần tách và vẽ bù trước.
3. Chuẩn bị bộ miệng **A–H và X nghỉ** theo quy ước Rhubarb, hoặc dùng miệng được vẽ bằng code như các mẫu sẵn. Không vẽ cố định một miệng trên nền mặt rồi đè thêm miệng khác.
4. Gắn điểm xoay ở cổ, vai, khuỷu, cổ tay; thiết lập quan hệ cha/con, chiều bàn tay trái/phải và giới hạn khớp. Hiệu chỉnh vị trí miệng theo mặt mới. Đây là bước rig; cần bổ sung cấu hình và bộ đọc asset vào AIR3view trước khi import được từ UI.
5. Tạo các trạng thái nghỉ, mở hai tay, chỉ tay, nhấn nhịp và bất ngờ; chuyển động có chuẩn bị, nhấn, giữ và trở về nghỉ. Căn khẩu hình bằng audio; căn cử chỉ bằng từ/thời điểm trong kịch bản. Kiểm tra cả các khung chuyển tiếp, không chỉ tư thế đầu/cuối.

Prompt tham khảo để tạo bản thiết kế: “Nhân vật hoạt hình 2D bán thân nhìn chính diện, đường nét rõ và nhất quán, đầu và hai vai đầy đủ, hai cánh tay tách nhẹ khỏi thân, bàn tay đúng giải phẫu với năm ngón, lòng bàn tay và cổ tay đúng chiều, không phối cảnh mạnh, không vật che khuất, nền trong suốt, không chữ, bố cục chừa khoảng trống quanh tóc và bàn tay.” Ảnh tạo ra vẫn cần tách layer và rig; prompt không tự tạo được một model chuyển động.

Có thể dựng rig trong Live2D/Spine rồi viết bộ tích hợp tương ứng, nhưng app hiện chưa đọc trực tiếp các định dạng đó. Tài liệu chuẩn: [Rhubarb mouth shapes](https://github.com/DanielSWolf/rhubarb-lip-sync#mouth-shapes), [Live2D nhập PSD](https://docs.live2d.com/en/cubism-editor-manual/psd-import/), [Spine bones](https://eu.esotericsoftware.com/spine-bones).

**Sau khi cập nhật rig:** video/avatar đã tạo trước đó vẫn là file cũ. Mở dự án → **Tạo giọng + avatar** để tạo lại nhân vật (WAV hợp lệ được dùng lại), kiểm tra preview rồi xuất MP4 lại. Khóa cache của nhân vật có fingerprint của worker/rig nên bản dựng mới không dùng lại video tay cũ.

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

### Hàng loạt: mỗi URL một dự án

1. Trong **Thiết lập dự án → Video nguồn**, chọn **Hàng loạt · mỗi URL một dự án**.
2. Thiết lập kịch bản, độ dài, hình ảnh, giọng, nhân vật và phụ đề một lần; dán mỗi URL trên một dòng hoặc nhập file `.txt`. Link trùng được gộp; sửa các dòng sai trước khi chạy. Mỗi đợt tối đa 200 URL.
3. Có thể bỏ chọn URL, đặt tên đợt và bật **Tự xuất MP4**. App dùng thư mục xuất đã nhớ, mỗi dự án có thư mục MP4 riêng.
4. Bấm **Bắt đầu xử lý hàng loạt**; hoặc **Xếp vào hàng đợi** để chạy sau. Mở **Xử lý hàng loạt** ở menu trái để xem tiến độ từng video, nhật ký và kết quả.

Mỗi URL có project, timeline, media và checkpoint riêng, dùng bản chụp thiết lập lúc tạo đợt; ảnh/giọng tham chiếu được chép vào dự án con. Vùng giữ/loại nguồn cụ thể và timeline của dự án mẫu không được sao chép. Chế độ **Nhiều nguồn · một dự án** vẫn dùng để ghép nhiều URL vào một video.

App xử lý lần lượt một dự án để giới hạn RAM/VRAM. **Tạm dừng sau video hiện tại** chờ video đang chạy xong; **Dừng tác vụ hiện tại** hủy bước hiện tại và tạm dừng đợt. Dùng **Bắt đầu / tiếp tục** để tiếp tục checkpoint; **Thử lại** cho các mục lỗi. Lỗi riêng một URL không chặn URL sau; lỗi cấu hình, xác thực hoặc tài nguyên chung sẽ tạm dừng để xử lý. Lỗi mạng tạm thời được thử lại tối đa một lần sau 30 giây.

**Bỏ khỏi hàng đợi** chỉ bỏ lượt chạy, giữ dữ liệu dự án. Có thể đổi thứ tự các mục đang chờ và mở dự án đã hoàn thành trong khi đợt tiếp tục. Tác vụ nặng thủ công và cài bản cập nhật bị khóa khi hàng đợi chạy. Đóng app sẽ dừng hàng đợi do phiên đó khởi chạy; mở lại và bấm tiếp tục. Không tự xuất lại các mục đã hoàn thành. Khi thử lại bước xuất, MP4 có biên nhận hợp lệ được kiểm tra rồi dùng lại.

**Xuất sau khi đã có timeline:** mỗi dòng trong **Xử lý hàng loạt** có nút **Xuất** (hoặc **Xuất lại**). Nút **Xuất MP4 cả phiên** ở góc phải xuất lần lượt các dự án hợp lệ trong phiên đang chọn. Trước khi xếp hàng, app kiểm tra `project.json`, timeline và media được dùng; nếu thiếu dữ liệu, hiển thị lý do và cho xác nhận bỏ qua các mục đó. Nếu không có dự án hợp lệ thì không chạy worker. Thao tác dùng timeline/giọng/avatar đã lưu, không chạy lại phân tích hay tạo giọng. Lần đầu chọn thư mục lưu, các lần sau dùng nơi đã nhớ; mỗi dự án lưu tại `AIR3view-<ID>/final.mp4`. Nếu MP4 đích đã tồn tại, app yêu cầu **Xác nhận xuất lại**; **Hủy** giữ nguyên file và hàng đợi. Xuất lại tạo MP4 mới từ timeline hiện tại và giữ bản cũ tại `final.previous-….mp4` sau khi bản mới vượt kiểm tra hình/tiếng/thời lượng. Có thể tạm dừng/tiếp tục và thử lại nếu xuất lỗi. CLI `batch-export <batch-id> <output-folder> [item-id]` yêu cầu thêm `--overwrite` khi đã có MP4 đích.

Dữ liệu đợt nằm trong `%LOCALAPPDATA%\ReactionStudio\batches\<ID>`: `batch.json`, bản chụp thiết lập, nhật ký và kết quả từng lần chạy. Giữ thư mục này cùng dữ liệu dự án để tiếp tục sau khi khởi động lại.

## Biên tập

- Timeline có trim, split, sắp xếp cảnh/layer, ẩn/khóa, volume/fade và Undo/Redo. Chỉnh layer/preview không cần encode lại MP4.
- Thanh công cụ timeline nằm trên một hàng; cửa sổ hẹp có thể cuộn ngang. Nút **SFX** cho phép bật/tắt, chọn Tự động / Whoosh / Pop / Ting và chỉnh âm lượng thẻ thông tin **0–200%**; ảnh dẫn chứng có lựa chọn Whoosh / Tự động / Pop / Ting và âm lượng riêng (mặc định Whoosh). Thiết lập được nhớ cho dự án sau. Âm thanh theo thời điểm thẻ xuất hiện, hỗ trợ preview và xuất MP4; chỉ đổi âm lượng sẽ dùng lại cache hình.
- SFX tự động bám theo thời điểm thẻ sau khi giọng TTS làm thay đổi độ dài cảnh; dự án cũ được căn lại khi mở để tránh tiếng hiệu ứng và đuôi đen ngoài phần lời/hình. SFX đã căn tay vẫn giữ thời điểm riêng.
- Phân tích mới dùng nhịp mặc định: hook theo thiết lập, lời bình một ý/cảnh khoảng 8–14 giây, Host ở điểm phân tích/bước ngoặt và tiếng gốc xen kẽ phân bố theo toàn video. Độ dài cảnh thực tế theo WAV; số đoạn gốc và thời lượng vẫn theo thiết lập dự án, không ép cắt ngang câu để đủ giây.
- **Cảnh → LAYER → Đồng bộ thông số với các layer cùng loại**: lấy layer đang chọn làm mẫu; bỏ qua layer/nhóm/cảnh khóa. Hình đồng bộ transform/thứ tự, âm thanh đồng bộ gain/fade. Nội dung, nguồn và thời gian clip giữ riêng.
- Nhân vật kéo thả trong preview hoặc nhập X/Y. Vị trí sub X/Y điều chỉnh ±2000 px theo kích thước đầu ra.
- Thông số layer tự được nhớ khi lưu/tự lưu, tạo dự án mới hoặc đóng app: X/Y, kích thước, xoay, độ hiện, thứ tự, âm lượng/fade, co giãn tự do và tick đồng bộ. Dự án sau dùng lại theo loại layer và cảnh Host/review riêng; các ảnh dẫn chứng giữ vị trí thứ tự riêng. Media, nội dung, timing, trạng thái khóa/ẩn không được chép sang dự án mới.
- Ảnh/số liệu dẫn chứng xuất hiện sau Host, có bố cục/chuyển động/SFX. Ảnh giữ đúng tỉ lệ trong card; chiều cao card thích ứng với ảnh để giảm khoảng trắng. Bố cục **Thẻ chồng** chia đều thời điểm xuất hiện trên cả cảnh; ảnh sau nằm trên ảnh trước, tất cả giữ đến hết cảnh và tiếp tục chuyển động. Ảnh cũ lùi mờ phía sau; giữ thời điểm căn tay. Chọn **Chuyển động khi hiển thị → Zoom vào nhẹ / Trôi nhẹ / Tĩnh** trong thiết lập dẫn chứng. Với dự án cũ hoặc khi đổi crop %, bấm **Tạo thẻ / cập nhật ảnh dẫn chứng** để tạo lại ảnh theo bố cục mới; giữ nguyên giọng/avatar. Số liệu cần đối chiếu với nguồn.
- Tiếng gốc nền lấy đoạn review đang chiếu; hook/đoạn gốc xen kẽ có gain riêng. Cảnh Host không tự lấy audio từ đoạn khác.
- Cache hình được dùng lại khi chỉ sửa âm thanh/metadata. Khi xuất, các thay đổi hình được dựng theo phần bị ảnh hưởng; tốc độ phụ thuộc máy và nội dung.
- **Nơi lưu / metadata → Metadata video…**: preset AIR3view, mô phỏng CapCut Desktop hoặc tùy chỉnh. Preset mô phỏng không xác nhận video xuất từ CapCut. Có thể đọc/áp metadata vào MP4 có sẵn bằng mux, không encode hình/tiếng.

## Dữ liệu và log

Dự án/cấu hình: `%LOCALAPPDATA%\ReactionStudio` (giữ tên cũ để tương thích). Log worker nằm trong `logs/vieneu-out.log`, `logs/vieneu-err.log`. Cache model VieNeu dùng file thường trong `runtime/huggingface-regular-v1` để ONNX hoạt động cả khi Windows bật Developer Mode; có thể đổi bằng biến `REACTION_MODEL_CACHE`. Dự án chứa `project.json`, `workflow.json`, `logs/pipeline.jsonl`, media/cache. Xóa dự án qua nút trong Thư viện; dự án cũng được gỡ khỏi mọi phiên Xử lý hàng loạt và phạm vi xuất của phiên. Giữ lịch sử phiên dù không còn dự án; nếu phiên đang chạy, tạm dừng trước khi xóa. Xóa thất bại giữ lại mục hàng loạt để thử lại. Dọn source/build không cần xóa AppData.

Khi hết dung lượng, màn hình dự án/hàng loạt hiển thị nguyên nhân tiếng Việt và hướng dẫn kiểm tra ổ chứa dự án/cache, giải phóng dung lượng rồi thử lại. Mở **Chi tiết kỹ thuật / thư mục cần kiểm tra** hoặc **Nhật ký** để xem thư mục và lỗi gốc. Hàng đợi tạm dừng khi lỗi dung lượng, không tự thử lại liên tục. Chọn ổ lưu MP4 khác không chuyển cache dự án sang ổ đó.

## Build và đóng gói

Cần Windows x64, 7-Zip (giải nén công cụ khi build từ source), Rust theo `rust-toolchain.toml`, Visual Studio Build Tools với Desktop development with C++ và Windows SDK. `scripts/build.cmd` tự tìm MSVC bằng vswhere.

```powershell
.\scripts\build.cmd
.\scripts\build.cmd clippy --release --all-targets --locked -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package.ps1 -SkipBuild -Archive
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-source.ps1
```

- Bản chạy: `dist/AIR3view/`; ZIP: `dist/AIR3view-0.2.9-windows-x64.zip`. `scripts/package.ps1 -OutputPath <thư mục mới>` cho phép đóng gói riêng khi bản cũ còn runtime local.
- Source sạch: `dist/AIR3view-0.2.9-source.zip`, không chứa cache/build, Python, model, video dự án, cookie hoặc executable công cụ.
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
studio-cli batch-create <template.json> <urls.txt> [output-folder]
studio-cli batch-run <batch-id>
studio-cli batch-action <batch-id> <action-json>
studio-cli prepare <project.json>
studio-cli script-only|speech|assets|captions|evidence <project.json>
studio-cli repair-script <project.json> <saved response>
studio-cli render <project.json> <output.mp4>
studio-cli metadata-inspect <input.mp4>
studio-cli metadata-apply <project.json> <input.mp4> <new-output.mp4>
studio-cli heygen-preflight
```

`batch-create` có thư mục output thì tự xuất MP4; không có thì dừng ở timeline sẵn sàng biên tập. `action-json` có dạng `{"action":"pause"}`, `{"action":"stop"}`, `{"action":"retry_all"}`, hoặc `{"action":"retry","item_id":"<ID>"}`; các thao tác `skip` và `up` cũng dùng `item_id`.

Xem `THIRD_PARTY_NOTICES.md`, `WHATLANG-LICENSE.txt` và giấy phép trong `tools/` trước khi phân phối.
