## AIR3view 0.2.12: SFX rõ ràng hơn, dẫn chứng và phục hồi dự án

- Tách rõ âm thanh **Thẻ thông tin / số liệu** và **Ảnh dẫn chứng**; thêm **Nghe**, **Đồng bộ loại SFX** và **Áp dụng lại cho nhóm**. Nút SFX nằm đầu thanh timeline để dễ tìm ở cửa sổ nhỏ.
- Thay bộ âm thanh Whoosh bằng **Paper tear**, Ting bằng **Appear**, Pop bằng bản mới đã cắt khoảng lặng. Cập nhật thư viện âm cho dự án cũ, giữ file âm riêng và các chỉnh sửa hình/giọng. MP4 đã xuất cần xuất lại.
- Đổi SFX khi preview layer đang phát không khởi động lại hình/voice; clip sao chép giữ loại cue, tôn trọng khóa và timing đã chỉnh tay. Xuất lại sau khi chỉ đổi âm dùng lại cache hình.
- Bổ sung đối chiếu ảnh dẫn chứng với nguồn, trạng thái chọn/loại và chọn lại ảnh. Kiểm tra ảnh cho cảnh Host trước khi báo hoàn tất; lỗi giữ checkpoint giọng/avatar để tiếp tục xử lý.
- Cải thiện lưu và phục hồi tài nguyên giọng/ảnh dùng chung giữa các dự án, bảo vệ tài nguyên còn được dự án khác sử dụng khi xóa.
- Cập nhật prompt True Crime / Bodycam, hướng dẫn lời nói tiếng Anh, đồng bộ cảm xúc và giới hạn thẻ. Quy tắc văn phong được xử lý theo hướng hướng dẫn; các kiểm tra dữ kiện, thời lượng và cấu trúc vẫn được giữ.
- Bổ sung phục hồi thời lượng thiếu và các kiểm thử hồi quy cho pipeline, dự án cũ, preview và MP4.

**Kiểm thử trên máy phát triển:** 223 kiểm thử Rust đạt, 2 bài kiểm thử ngoài bỏ qua trong lượt chạy thường; kiểm thử riêng SFX trên 8 dự án, 96 trường hợp chọn âm theo nhóm và 16 đoạn MP4 thử nghiệm. Đã đối chiếu âm thanh, cache hình, UI 1080×720 / 1440×900 và kiểm thử cập nhật/khôi phục. GitHub Actions tiếp tục chạy toàn bộ bộ kiểm thử, Clippy, updater, đóng gói, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, cài và mở lại. Người cài mới tải **AIR3view-0.2.12-windows-x64.zip**, giải nén đầy đủ rồi mở **AIR3view.exe**. Dữ liệu và cấu hình giữ theo máy; key, cookie và dữ liệu dự án không nằm trong gói phát hành.

### Tính năng từ 0.2.11: sửa lỗi tạo hàng đợi với nhân vật 2D và giọng tham chiếu

- Sửa lỗi **Không tạo được hàng đợi: Không đọc được ảnh/giọng tham chiếu** khi chọn nhân vật **2D local** nhưng cấu hình còn đường dẫn ảnh HeyGen cũ đã bị xóa. Hàng đợi chỉ sao chép tài nguyên đang dùng; nhân vật 2D không phụ thuộc ảnh HeyGen cũ.
- Giọng tham chiếu đang bật vẫn được sao chép riêng vào từng dự án con. Bỏ đường dẫn giọng khi tắt tham chiếu và đường dẫn ảnh khi tắt nhân vật; giữ nguyên thiết lập dự án mẫu.
- Nếu file đang dùng không đọc được, thông báo nêu rõ **giọng tham chiếu** hoặc **ảnh nhân vật HeyGen**, đường dẫn và hướng dẫn chọn lại trong **Giọng & nhân vật**. Phân biệt lỗi đọc file nguồn với lỗi lưu bản sao trong dự án.
- Kiểm thử local: **168 kiểm thử Rust thành công, 1 bỏ qua**, Clippy sạch; tạo phiên 3 URL bằng cấu hình thực tế từng gây lỗi, đối chiếu file giọng trong các dự án con, tạo giọng thật bằng VieNeu và kiểm tra EXE đã cài khởi chạy thành công. CI tiếp tục kiểm tra updater/khôi phục, gói cài, render, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, cài và mở lại; sau đó tạo lại phiên hàng loạt. Không cần upload lại giọng tham chiếu nếu file vẫn tồn tại. Dự án và cấu hình cũ được giữ nguyên. Người cài mới tải bản **windows-x64.zip**, giải nén đầy đủ rồi mở **AIR3view.exe**.

### Tính năng từ 0.2.10: sửa lỗi xuất thiết lập với nhân vật 2D

- Sửa lỗi **Xuất thiết lập** báo thiếu ảnh HeyGen cũ khi đang dùng nhân vật **2D local**. Nhân vật 2D có sẵn được chuyển bằng mã lựa chọn, không phụ thuộc đường dẫn ảnh của nhà cung cấp khác.
- Bỏ các đường dẫn ảnh không được dùng khi tắt nhân vật hoặc chọn HeyGen bằng avatar ID; bỏ file giọng tham chiếu/nhạc nền khi các mục đó đang tắt. Dự án gốc giữ nguyên; tài nguyên đang bật vẫn được kiểm tra đầy đủ.
- Áp dụng cho cả gói kèm tài nguyên và gói chỉ chứa thiết lập. Giữ nguyên định dạng `.air3preset` và khả năng nhập các gói đã xuất trước đó.
- Kiểm thử local: **165 kiểm thử Rust thành công, 1 bỏ qua**, Clippy sạch; đã xuất, kiểm tra và nhập lại bằng cấu hình lưu thực tế có đường dẫn ảnh cũ bị thiếu. GitHub Actions tiếp tục kiểm tra updater/khôi phục, gói cài, render, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, cài và mở lại; sau đó xuất thiết lập lại. Dự án và cấu hình cũ được giữ nguyên. Người cài mới tải bản **windows-x64.zip**, giải nén đầy đủ rồi mở **AIR3view.exe**.

### Tính năng từ 0.2.9: xuất/nhập thiết lập và đồng bộ xóa dự án

- Thêm **Biên tập video → Thiết lập → Xuất thiết lập / Nhập thiết lập**. Chuyển các thông số kịch bản, ngôn ngữ, thời lượng, hình ảnh, tốc độ, giọng/nhân vật, dẫn chứng, âm thanh, phụ đề, mẫu layer, track và metadata giữa các máy bằng một file `.air3preset`.
- Mặc định kèm ảnh nhân vật, giọng tham chiếu, nhạc và LUT tùy chỉnh; kiểm tra SHA-256 và đổi đường dẫn theo kho asset của máy nhận. Gói không kèm tài nguyên cho phép chọn lại đúng file trước khi nhập.
- Xem tóm tắt trước khi nhập, chọn áp dụng cho dự án hiện tại và/hoặc ghi nhớ cho dự án tiếp theo. Sao lưu cấu hình trước khi thay đổi và khôi phục nếu lưu thất bại. Đọc/ghi/hash file chạy nền; khóa thao tác khi đang xử lý dự án/hàng loạt.
- Giữ kết nối, key, đăng nhập, cookie và nơi lưu theo máy nhận. Giữ URL, nội dung cảnh và timing layer theo dự án; đổi tốc độ footage có thể điều chỉnh thời lượng cảnh gốc. Đổi ngôn ngữ/giọng/nhân vật/kiểu dẫn chứng cần tạo lại nội dung tương ứng. Font không đóng gói, cần cài font hoặc chọn font có trên máy nhận.
- Mẫu màu/lật theo vai trò layer và thông số mute/snapping/tick đồng bộ được nhớ cho dự án mới. Khóa track giữ theo dự án đích. Bố cục các hộp thoại và thanh công cụ được kiểm tra ở cửa sổ nhỏ/lớn.
- Xóa dự án trong **Thư viện dự án** cũng gỡ dự án khỏi mọi phiên **Xử lý hàng loạt** và phạm vi xuất MP4 liên quan. Danh sách cập nhật ngay và giữ đúng sau khi mở lại. Giữ lịch sử phiên trống; nếu xóa thất bại thì giữ mục để thử lại. Tạm dừng phiên đang chạy trước khi xóa.
- Kiểm thử local: **163 kiểm thử Rust thành công, 1 bỏ qua**, Clippy không cảnh báo; có kiểm thử chuyển tài nguyên sang đường dẫn khác, khôi phục khi file bị khóa, thao tác UI và xuất MP4 thật sau khi nhập preset. CI tiếp tục kiểm tra updater/khôi phục, gói cài, render, TTS, avatar và Whisper trước khi công bố release.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, kiểm tra, cài và mở lại. Người cài mới tải bản **windows-x64.zip**, giải nén đầy đủ rồi mở **AIR3view.exe**. Dự án và cấu hình cũ được giữ nguyên.

### Tính năng từ 0.2.8: nhạc nền, màu footage và giao diện thiết lập gọn hơn

- Thêm **Nhạc nền độc lập**: chọn file nhạc, âm lượng, điểm bắt đầu, fade, lặp với nối chuyển tiếp, tự giảm nhạc dưới lời host và tắt nhạc trong đoạn nghe tiếng gốc. Track nhạc riêng hỗ trợ chỉnh sửa trên timeline; nhớ lựa chọn cho dự án sau.
- Thêm **Màu sắc & lật footage**: LUT có sẵn hoặc nhập `.cube`, chỉnh cường độ, lật ngang/dọc; có ghi đè cho từng layer và đồng bộ theo loại layer. Preview và MP4 dùng cùng hiệu ứng.
- Thêm **Chuẩn hóa video đầu vào**: tự động, giữ nguyên, đóng gói lại hoặc chuyển mã H.264/AAC; tùy chọn proxy nhẹ. Giữ file gốc, kiểm tra thời lượng và xử lý nguồn xoay, HDR hoặc không có audio.
- Cache hình và âm thanh độc lập: thay nhạc/âm lượng dùng lại hình đã dựng; thay LUT/lật chỉ cập nhật các phần hình bị ảnh hưởng.
- Các nhóm con trong **Thiết lập dự án** và **Biên tập → Cảnh / Khung / Sub / Dẫn chứng** có thể đóng/mở riêng; giữ nguyên thông số và trạng thái bật/tắt. Chỉnh bố cục hẹp, vị trí sub và chiều cao các khối thuộc tính cảnh.
- Bỏ dòng mã nguồn/timestamp ở chân mọi thẻ dẫn chứng; dữ liệu nguồn và đối chiếu vẫn lưu trong dự án. **Dự án cũ:** sau khi cập nhật, bấm **Tạo thẻ / cập nhật** rồi xuất MP4 lại để áp dụng mẫu thẻ mới; không cần tạo lại giọng/nhân vật.
- Bao gồm sửa xuất MP4 hàng loạt: kiểm tra từng dự án và yêu cầu xác nhận khi xuất lại file đã tồn tại.
- Kiểm thử local trước khi đẩy: **149 kiểm thử Rust thành công, 1 bỏ qua**, Clippy không cảnh báo; đã kiểm tra bố cục UI 1080×720 / 1440×900 và MP4 dùng mẫu thẻ không có dòng nguồn. GitHub Actions tiếp tục kiểm tra gói cài, cập nhật/khôi phục, render, TTS, avatar và Whisper trước khi công bố release.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, kiểm tra, cài và mở lại. Dự án/cấu hình được giữ nguyên. Bản ZIP không kèm nhạc mẫu, dữ liệu dự án, cookie hay thông tin đăng nhập của tác giả. Khi mang dự án sang máy khác, cần mang theo các file nhạc/LUT/giọng tham chiếu riêng.

### Tính năng từ 0.2.7: tốc độ footage gốc và hook độc lập với giọng AI

- Thêm **Tốc độ footage gốc / hook** tại **Thiết lập dự án → Hình ảnh đầu ra** và bảng **Khung**: tắt, tự động hoặc tốc độ cố định; điều chỉnh riêng hook và các đoạn gốc xen kẽ. Nhớ thiết lập cho dự án mới và xử lý hàng loạt.
- Giữ tốc độ giọng AI, nhân vật, phụ đề, dẫn chứng và SFX; tiếng gốc đi theo tốc độ footage. Preview và MP4 dùng cùng phép ánh xạ thời gian nguồn.
- Điều chỉnh từng cảnh tại **Tốc độ hình & mốc khớp lời**; hỗ trợ các mốc hành động/cụm từ và báo cáo `analysis/footage-speed-report.json`. Kiểm tra giới hạn đoạn nguồn, vùng bắt buộc giữ/loại và thời lượng tối thiểu; báo xung đột khi không đủ footage.
- Sửa đường âm thanh 1× để không đi qua bộ đổi tốc độ; nâng phiên bản cache nhận dạng/xuất để tạo lại dữ liệu cần thiết.
- Đã kiểm thử hai URL YouTube thực tế từ tải, phân tích, tạo giọng/nhân vật tới MP4 dọc và ngang ở 360p; hook 1,25×, giọng AI giữ nguyên tốc độ và không lệch thời gian trong đoạn đo. Đạt 136 kiểm thử Rust, 1 bỏ qua; Clippy không cảnh báo trước khi chuẩn bị phát hành. CI tiếp tục kiểm tra bộ cài, cập nhật/khôi phục, render, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, cài và mở lại. Dự án và cấu hình được giữ lại. Nếu dự án báo mất audio tham chiếu, chọn lại audio tại **Thiết lập dự án → Giọng, nhân vật & âm thanh**; bản này chưa sửa cơ chế kế thừa đường dẫn audio cũ.

### Tính năng từ 0.2.6: thư mục video nguồn, bộ lọc ngày và nhân vật 2D

- Thêm chọn **Thư mục video nguồn YouTube** trong **Nơi lưu / metadata**; ghi nhớ cho các dự án mới và video tiếp theo trong hàng đợi. Mỗi dự án có thư mục nguồn riêng để tránh ghi đè. Có nút trở về nơi lưu mặc định của dự án.
- Giữ nguyên đường dẫn video đã tải; thay nơi lưu không yêu cầu phân tích, tạo giọng hoặc avatar lại. Báo rõ khi ổ lưu không khả dụng. Thư mục xuất MP4 được chọn độc lập.
- Thêm bộ lọc ngày cho **Thư viện dự án** và **Xử lý hàng loạt**, sắp xếp các phiên chạy rõ ràng; điều chỉnh bố cục ngang, đặt **Theo** và **Lọc ngày** cùng hàng.
- Thêm nhân vật **Gấu trúc** với phần thân dưới, chuyển động và nhép miệng; sửa hướng cổ tay, ngón tay và giữ đúng tỉ lệ nhân vật trong preview/video.
- Làm rõ lỗi thiếu dung lượng và cách xử lý; hàng đợi tạm dừng khi gặp lỗi tài nguyên chung. Cập nhật icon khi menu được thu gọn.
- Đã đạt **128 kiểm thử Rust, 1 bỏ qua**, Clippy không cảnh báo; kiểm tra **4.010 tư thế tay** và cài/khôi phục updater thành công. Kiểm thử lưu/khôi phục cấu hình, đường dẫn Unicode, thư mục nguồn độc lập và bố cục menu. Luồng tải được kiểm thử với dữ liệu mạng giả lập; probe, proxy và giải mã khung hình dùng FFmpeg thật. Quy trình phát hành tiếp tục kiểm tra render, TTS, avatar và Whisper trước khi công bố.

**Cập nhật:** bấm **Cập nhật** trong app để tự tải, cài và mở lại. Dự án và cấu hình được giữ lại. Video nguồn đặt ngoài thư mục dự án vẫn được giữ khi xóa dự án.

### Tính năng từ 0.2.5: xử lý hàng loạt, mỗi URL một dự án

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
