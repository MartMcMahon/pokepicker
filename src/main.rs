mod app;
mod color_ui;
mod data;
mod fetch;
mod theme;

fn create_app(cc: &eframe::CreationContext<'_>) -> app::App {
    theme::apply(&cc.egui_ctx);
    app::App::new(&cc.egui_ctx)
}

#[cfg(not(target_arch = "wasm32"))]
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
        Box::new(|cc| Ok(Box::new(create_app(cc)))),
    )
}

/// Web entry point: trunk's generated JS calls `main` once the wasm loads.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;

    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("pokepicker"))
        .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .expect("index.html should have <canvas id=\"pokepicker\">");

    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(create_app(cc)))),
            )
            .await
            .expect("failed to start pokepicker");
    });
}
