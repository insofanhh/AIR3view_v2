use super::*;
use crate::updates::Status;
impl StudioApp {
    pub(super) fn update_banner(&mut self,ctx:&egui::Context) {
        if let Some(status)=self.updates.banner().cloned() {
            egui::TopBottomPanel::top("update_banner").frame(egui::Frame::new().fill(CARD).inner_margin(10)).show(ctx,|ui|{
                ui.horizontal_wrapped(|ui|{
                    match status {
                        Status::Release{tag,url,download}=>{ui.label(format!("AIR3view {tag} đã sẵn sàng."));ui.hyperlink_to("Tải ZIP Windows",download);ui.hyperlink_to("Chi tiết",url);},
                        Status::Code{commit,url}=>{ui.label(format!("Có code mới ({commit}). Bản tải đang được chuẩn bị."));ui.hyperlink_to("Xem thay đổi",url);},
                        Status::Current=>{},
                    }
                    if ui.small_button("Đóng").clicked(){self.updates.dismiss();}
                });
            });
        }
    }
    pub(super) fn updates_window(&mut self,ctx:&egui::Context) {
        let mut open=self.updates.open;
        let response=egui::Window::new("Phiên bản & cập nhật").open(&mut open).default_width(470.).resizable(false).show(ctx,|ui|{
            ui.heading(format!("AIR3view {}",self.updates.build.version));
            if !self.updates.build.commit.is_empty(){ui.label(format!("Build: {}",self.updates.build.commit.chars().take(8).collect::<String>()));}
            if ui.checkbox(&mut self.updates.automatic,"Tự kiểm tra khi mở app và mỗi 15 phút").changed(){
                if let Err(e)=self.updates.save(){self.updates.error=Some(e.to_string());}
            }
            if ui.add_enabled(!self.updates.checking,egui::Button::new("Kiểm tra cập nhật ngay")).clicked(){self.updates.start();}
            if self.updates.checking {ui.horizontal(|ui|{ui.spinner();ui.label("Đang kiểm tra GitHub…");});}
            if let Some(error)=&self.updates.error {ui.colored_label(MUTED,error);}
            match &self.updates.status {
                Some(Status::Release{tag,url,download})=>{ui.label(format!("Bản mới: {tag}"));ui.hyperlink_to("Tải ZIP Windows",download);ui.hyperlink_to("Ghi chú phát hành",url);},
                Some(Status::Code{url,..})=>{ui.label("Code mới đã được đẩy lên. Chờ build/release để tải bản chạy.");ui.hyperlink_to("Xem thay đổi",url);},
                Some(Status::Current)=>{ui.label("Chưa có bản mới hơn build đang dùng.");},
                None=>{},
            }
            ui.separator();ui.label("Giải nén bản mới vào thư mục mới rồi mở Start-AIR3view.cmd. Dự án trong AppData được giữ lại. Lần đầu chạy cần mạng để thiết lập Python/model và cấu hình dịch vụ AI riêng.");
            ui.hyperlink_to("GitHub AIR3view",crate::updates::REPOSITORY);
        });self.updates.open=open;
        #[cfg(test)]
        if let Some(response)=response {self.hitboxes.insert("updates_window".into(),response.response.rect);}
        #[cfg(not(test))]
        let _=response;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_window_fits_supported_layouts() {
        for size in [Vec2::new(1080.,720.),Vec2::new(1440.,900.)] {
            let ctx=egui::Context::default();let mut app=StudioApp::from_context(&ctx);app.updates.open=true;
            app.updates.status=Some(Status::Release{tag:"v0.2.1".into(),url:crate::updates::REPOSITORY.into(),download:crate::updates::REPOSITORY.into()});
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),..Default::default()},|ctx|app.draw(ctx));}
            let rect=app.hitboxes["updates_window"];assert!(rect.width()<=size.x && rect.height()<=size.y);assert!(rect.left()>=0. && rect.top()>=0. && rect.right()<=size.x+1. && rect.bottom()<=size.y+1.);
        }
    }
}
