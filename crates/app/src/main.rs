#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Cedar IDE")
            .with_inner_size([1320.0, 880.0])
            .with_min_inner_size([780.0, 540.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Cedar IDE",
        options,
        Box::new(|cc| Ok(Box::new(cedar_app::CedarApp::new(cc)))),
    )
}
