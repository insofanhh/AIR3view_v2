#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use reaction_studio::app::StudioApp;

fn main() -> eframe::Result<()> {
    let window_size = std::env::args()
        .find_map(|arg| {
            let value = arg.strip_prefix("--window-size=")?;
            let (w, h) = value.split_once('x')?;
            Some([w.parse::<f32>().ok()?, h.parse::<f32>().ok()?])
        })
        .filter(|size| size.iter().all(|v| v.is_finite()) && size[0] >= 1080. && size[1] >= 720.)
        .unwrap_or([1440., 900.]);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(reaction_studio::branding::APP_NAME)
            .with_icon(reaction_studio::branding::window_icon())
            .with_inner_size(window_size)
            .with_min_inner_size([1080.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        reaction_studio::branding::APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(StudioApp::new(cc)))),
    )
}
