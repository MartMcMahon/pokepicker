mod app;
mod color_ui;
mod fetch;
mod theme;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("POKEPICKER")
            .with_inner_size([960.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "pokepicker",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::default()))
        }),
    )
}
