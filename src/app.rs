use std::sync::mpsc::{Receiver, channel};

use eframe::egui::{self, RichText, TextureHandle, TextureOptions};
use pokepicker::api::Pokemon;
use pokepicker::color::Rgb;

use crate::color_ui::{ColorSearch, Event};
use crate::fetch;
use crate::theme;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Lookup,
    Color,
}

struct Loaded {
    mon: Pokemon,
    tex: TextureHandle,
    /// Kept CPU-side so the sprite can be clicked to pick a colour.
    image: egui::ColorImage,
}

pub struct App {
    tab: Tab,
    query: String,
    status: String,
    error: bool,
    current: Option<Loaded>,
    pending: Option<Receiver<fetch::Lookup>>,
    color: ColorSearch,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        Self {
            tab: Tab::Color,
            query: "pikachu".into(),
            status: "Hello, world! Who are you looking for?".into(),
            error: false,
            current: None,
            pending: None,
            color: ColorSearch::new(ctx),
        }
    }

    /// Start a lookup in the background; the result is picked up in `poll`.
    fn fetch(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let (tx, rx) = channel();
        fetch::lookup(&self.query, move |result| {
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.pending = Some(rx);
        self.set_status(
            format!("Searching for {}", self.query.trim().to_uppercase()),
            false,
        );
    }

    fn set_status(&mut self, text: String, error: bool) {
        self.status = text;
        self.error = error;
    }

    fn handle(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Open(name) => {
                self.query = name;
                self.tab = Tab::Lookup;
                self.fetch(ctx);
            }
            Event::Status { text, error } => self.set_status(text, error),
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.pending else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.pending = None;
        match result {
            Ok((mon, image)) => {
                let tex = ctx.load_texture(&mon.name, image.clone(), TextureOptions::NEAREST);
                self.set_status(
                    format!(
                        "Found {}! Click the sprite to match a color.",
                        mon.name.to_uppercase()
                    ),
                    false,
                );
                self.current = Some(Loaded { mon, tex, image });
            }
            Err(e) => self.set_status(format!("Oops! {e:#}"), true),
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            // A web page can't close its own tab.
            #[cfg(not(target_arch = "wasm32"))]
            ui.menu_button("FILE", |ui| {
                if ui.button("QUIT").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("DATA", |ui| {
                if ui.button("FETCH").clicked() {
                    self.tab = Tab::Lookup;
                    self.fetch(ui.ctx());
                }
                if ui.button("CLEAR").clicked() {
                    self.current = None;
                }
                // The web build uses the bundled index; only desktop can rebuild.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    let busy = self.color.is_busy();
                    if ui
                        .add_enabled(!busy, egui::Button::new("REBUILD COLOR INDEX"))
                        .clicked()
                    {
                        self.tab = Tab::Color;
                        if let Some(event) = self.color.start_build(ui.ctx()) {
                            self.handle(ui.ctx(), event);
                        }
                    }
                }
            });
            ui.menu_button("HELP", |ui| {
                ui.label(format!("SOURCE: {}", pokepicker::api::BASE_URL));
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Lookup, RichText::new("LOOKUP").strong());
            ui.selectable_value(
                &mut self.tab,
                Tab::Color,
                RichText::new("COLOR MATCH").strong(),
            );
        });
    }

    fn lookup(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("SEARCH").strong());
            // Leave room for the FETCH button so the row fits on a phone.
            let room = ui.available_width() - 130.0;
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("name or number")
                    .desired_width(room.clamp(80.0, 280.0))
                    .margin(egui::vec2(8.0, 6.0)),
            );
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let busy = self.pending.is_some();
            if (ui
                .add_enabled(!busy, egui::Button::new("▶ FETCH"))
                .clicked()
                || enter)
                && !busy
            {
                self.fetch(ui.ctx());
            }
        });
        ui.add_space(12.0);
        if let Some(color) = self.details(ui) {
            self.color.set_target(color);
            self.tab = Tab::Color;
        }
    }

    /// Returns a colour if the user clicked on the sprite.
    fn details(&self, ui: &mut egui::Ui) -> Option<Rgb> {
        let mut picked = None;
        theme::card().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let Some(Loaded { mon, tex, image }) = &self.current else {
                ui.label(RichText::new("No Pokémon loaded yet. Try a search!").color(theme::MUTED));
                return;
            };
            // Stats beside the sprite, or under it when there's no room (phones).
            let layout = if ui.available_width() >= 420.0 {
                egui::Layout::left_to_right(egui::Align::Min)
            } else {
                egui::Layout::top_down(egui::Align::Min)
            };
            ui.with_layout(layout, |ui| {
                // Sprites are 96x96 pixel art; scale up with nearest-neighbour.
                egui::Frame::new()
                    .fill(theme::SKY)
                    .corner_radius(egui::CornerRadius::same(6))
                    .show(ui, |ui| {
                        let sprite = ui
                            .add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(egui::vec2(192.0, 192.0))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::Crosshair);
                        if sprite.clicked() {
                            picked = sprite
                                .interact_pointer_pos()
                                .and_then(|pos| pixel_at(image, sprite.rect, pos));
                        }
                    });
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("No.{:03}", mon.id)).color(theme::MUTED));
                    ui.heading(RichText::new(mon.name.to_uppercase()).strong());
                    ui.separator();
                    let types: Vec<_> = mon
                        .types
                        .iter()
                        .map(|t| t.kind.name.to_uppercase())
                        .collect();
                    egui::Grid::new("stats")
                        .spacing([16.0, 6.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("TYPE").color(theme::MUTED));
                            ui.label(types.join(" / "));
                            ui.end_row();
                            ui.label(RichText::new("HEIGHT").color(theme::MUTED));
                            ui.monospace(format!("{:.1} m", mon.height as f32 / 10.0));
                            ui.end_row();
                            ui.label(RichText::new("WEIGHT").color(theme::MUTED));
                            ui.monospace(format!("{:.1} kg", mon.weight as f32 / 10.0));
                            ui.end_row();
                        });
                });
            });
        });
        picked
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll(ui.ctx());

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));

        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme::PAPER)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| {
                theme::card()
                    .inner_margin(egui::Margin::symmetric(14, 8))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let color = if self.error {
                            theme::ACCENT
                        } else {
                            theme::INK
                        };
                        let busy = self.pending.is_some() || self.color.is_busy();
                        let cursor = if busy { " …" } else { " ▼" };
                        ui.label(RichText::new(format!("{}{cursor}", self.status)).color(color));
                    });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::PAPER)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| {
                self.tabs(ui);
                ui.add_space(10.0);
                match self.tab {
                    Tab::Lookup => self.lookup(ui),
                    Tab::Color => {
                        if let Some(event) = self.color.ui(ui) {
                            self.handle(ui.ctx(), event);
                        }
                    }
                }
            });
    }
}

/// The opaque sprite pixel under `pos`, given the sprite is drawn into `rect`.
fn pixel_at(image: &egui::ColorImage, rect: egui::Rect, pos: egui::Pos2) -> Option<Rgb> {
    let [w, h] = image.size;
    let uv = (pos - rect.min) / rect.size();
    let x = ((uv.x * w as f32) as usize).min(w - 1);
    let y = ((uv.y * h as f32) as usize).min(h - 1);
    let [r, g, b, a] = image.pixels[y * w + x].to_srgba_unmultiplied();
    (a >= 128).then_some([r, g, b])
}
