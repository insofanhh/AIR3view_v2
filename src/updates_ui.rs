use super::*;
use crate::updates::Status;
impl StudioApp {
    fn update_button(&mut self, ui: &mut egui::Ui, tag: &str, download: &str, checksum: &str) {
        let response = ui.add_enabled(!self.updates.installer.active && !self.busy && !self.batch_view.active && !self.batch_view.creating, egui::Button::new("Cập nhật"));
        #[cfg(test)]
        self.hitboxes.insert("install_update".into(), response.rect);
        if response.on_hover_text("Tự tải, kiểm tra, cài vào thư mục hiện tại và mở lại AIR3view. Chờ tác vụ đang chạy hoàn tất trước khi cập nhật.").clicked() {
            self.updates.installer.start(tag.into(), download.into(), checksum.into());
            self.updates.open = true;
        }
    }
    fn update_progress(&self, ui: &mut egui::Ui) {
        let installer = &self.updates.installer;
        if !installer.message.is_empty() {
            ui.colored_label(if installer.error { Color32::from_rgb(235, 130, 120) } else { MUTED }, &installer.message);
        }
        if installer.active {
            if let Some(fraction) = installer.fraction { ui.add(egui::ProgressBar::new(fraction).show_percentage()); }
            else { ui.spinner(); }
        }
    }
    pub(super) fn advance_update(&mut self, ctx: &egui::Context) {
        let Some(job) = self.updates.installer.ready.clone() else { return; };
        if let Some(started) = self.updates.installer.launching {
            if job.parent().is_some_and(|p| p.join("waiting").is_file()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                let stopped = self.updates.installer.child.as_mut().is_some_and(|c| c.try_wait().ok().flatten().is_some());
                if stopped || started.elapsed() > Duration::from_secs(120) {
                    self.updates.installer.fail(format!("Trình cài cập nhật chưa sẵn sàng. App đang dùng được giữ nguyên. Nhật ký: {}", job.parent().unwrap().join("installer.log").display()));
                    self.updates.open = true;
                }
            }
            return;
        }
        if self.busy || self.batch_view.active || self.batch_view.creating { return; }
        let result = (|| -> anyhow::Result<_> {
            if self.dirty { self.project.save()?; self.dirty = false; }
            self.remember_project_defaults()?;
            self.save_settings()?;
            crate::updater::launch(&job)
        })();
        match result {
            Ok(child) => {
                self.player.stop();
                self.updates.installer.child = Some(child);
                self.updates.installer.launching = Some(Instant::now());
                self.updates.installer.message = "Đang chuẩn bị cài đặt và khởi động lại AIR3view…".into();
            }
            Err(e) => { self.updates.installer.fail(format!("Chưa thể cập nhật: {e:#}")); self.updates.open = true; }
        }
    }
    pub(super) fn update_banner(&mut self,ctx:&egui::Context) {
        if let Some(status)=self.updates.banner().cloned() {
            egui::TopBottomPanel::top("update_banner").frame(egui::Frame::new().fill(CARD).inner_margin(10)).show(ctx,|ui|{
                ui.horizontal_wrapped(|ui|{
                    match status {
                        Status::Release{tag,url,download,checksum}=>{ui.label(format!("AIR3view {tag} đã sẵn sàng."));self.update_button(ui,&tag,&download,&checksum);ui.hyperlink_to("Chi tiết",url);},
                        Status::Code{commit,url}=>{ui.label(format!("Có code mới ({commit}). Bản tải đang được chuẩn bị."));ui.hyperlink_to("Xem thay đổi",url);},
                        Status::Current=>{},
                    }
                    if ui.small_button("Đóng").clicked(){self.updates.dismiss();}
                });
                self.update_progress(ui);
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
            match self.updates.status.clone() {
                Some(Status::Release{tag,url,download,checksum})=>{ui.label(format!("Bản mới: {tag}"));self.update_button(ui,&tag,&download,&checksum);ui.hyperlink_to("Ghi chú phát hành",url);},
                Some(Status::Code{url,..})=>{ui.label("Code mới đã được đẩy lên. Chờ build/release để tải bản chạy.");ui.hyperlink_to("Xem thay đổi",url);},
                Some(Status::Current)=>{ui.label("Chưa có bản mới hơn build đang dùng.");},
                None=>{},
            }
            self.update_progress(ui);
            ui.separator();ui.label("Bấm Cập nhật để tự tải bản chính thức, kiểm tra SHA256 và thay các file thay đổi/thiếu trong thư mục app. App tự lưu dự án và mở lại sau khi cài; giữ cài đặt, model và môi trường Python. Nếu bộ thư viện thay đổi, app tự bổ sung khi khởi chạy. Khi cài lỗi, khôi phục file cũ.");
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
            app.updates.status=Some(Status::Release{tag:"v0.2.1".into(),url:crate::updates::REPOSITORY.into(),download:crate::updates::REPOSITORY.into(),checksum:String::new()});
            for _ in 0..3 {let _=ctx.run(egui::RawInput{screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),..Default::default()},|ctx|app.draw(ctx));}
            let rect=app.hitboxes["updates_window"];assert!(rect.width()<=size.x && rect.height()<=size.y);assert!(rect.left()>=0. && rect.top()>=0. && rect.right()<=size.x+1. && rect.bottom()<=size.y+1.);
        }
    }
    #[test]
    fn active_jobs_disable_install_and_progress_and_errors_fit() {
        let ctx=egui::Context::default(); let mut app=StudioApp::from_context(&ctx);
        for (busy,active,error) in [(true,false,false),(false,true,false),(false,false,true)] {
            app.busy=busy; app.updates.installer.active=active; app.updates.installer.error=error;
            app.updates.installer.message="Đang tải bản mới: 152.3 MB — tiến trình tải, kiểm tra file và giữ dự án hiện tại".into();
            app.updates.installer.fraction=Some(0.65);
            let _=ctx.run(egui::RawInput::default(),|ctx| {
                egui::CentralPanel::default().show(ctx,|ui| {
                    app.update_button(ui,"v9","invalid-url","invalid-checksum");
                    app.update_progress(ui);
                    assert!(ui.min_rect().is_finite());
                });
            });
            assert!(app.updates.installer.ready.is_none());
        }
    }
}
