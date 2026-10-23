//! One embedded mark for the sidebar, native window and packaged executable.
pub const APP_NAME: &str = "AIR3view";
pub const ICON_PNG: &[u8] = include_bytes!("../assets/air3view.png");

pub fn window_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(ICON_PNG)
        .expect("Embedded AIR3view icon")
        .into_rgba8();
    eframe::egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

pub fn logo(ctx: &eframe::egui::Context) -> eframe::egui::TextureHandle {
    let icon = window_icon();
    ctx.load_texture(
        "air3view-logo",
        eframe::egui::ColorImage::from_rgba_unmultiplied(
            [icon.width as usize, icon.height as usize],
            &icon.rgba,
        ),
        Default::default(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_is_complete_rgba_and_has_transparent_corners() {
        let icon = super::window_icon();
        assert_eq!((icon.width, icon.height), (512, 512));
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        assert_eq!(icon.rgba[3], 0);
        assert_eq!(icon.rgba[(256 * 512 + 256) * 4 + 3], 255);
        assert_eq!(super::APP_NAME, "AIR3view");
    }
}
