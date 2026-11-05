//! The COLOR MATCH tab: pick a colour, see which pokemon wear it.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use egui::color_picker::{Alpha, color_picker_color32};
use pokepicker::api::Client;
use pokepicker::color::{Rgb, parse_hex, to_hex};
use pokepicker::index::{BuildReport, Index, Match, Progress, Store};

use crate::theme;

/// Result tiles shown at once; past this the matches are too weak to be interesting.
const MAX_RESULTS: usize = 120;
const TILE: f32 = 96.0;
const TILE_MARGIN: i8 = 6;
/// Side of the picker's saturation/value square when it has its own column.
const PICKER_SIDE: f32 = 290.0;
/// Smaller square when the window is too narrow for side-by-side columns.
const PICKER_SIDE_STACKED: f32 = 250.0;
/// Below this width the picker stacks above the results instead.
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 720.0;

pub enum Event {
    /// The user picked a result; look it up.
    Open(String),
    Status {
        text: String,
        error: bool,
    },
}

struct Build {
    progress: Arc<Progress>,
    rx: Receiver<anyhow::Result<BuildReport>>,
}

pub struct ColorSearch {
    store: Option<Store>,
    index: Option<Index>,
    build: Option<Build>,
    target: Color32,
    hex: String,
    tolerance: f32,
    results: Vec<Match>,
    stale: bool,
    /// `None` marks a sprite that failed to load, so we don't retry every frame.
    thumbs: HashMap<u32, Option<TextureHandle>>,
}

impl Default for ColorSearch {
    fn default() -> Self {
        let store = Store::default_location().ok();
        let index = store.as_ref().and_then(|s| Index::load(s).ok().flatten());
        let target = Color32::from_rgb(0xf8, 0xd0, 0x30);
        Self {
            store,
            index,
            build: None,
            target,
            hex: to_hex(rgb(target)),
            tolerance: 20.0,
            results: Vec::new(),
            stale: true,
            thumbs: HashMap::new(),
        }
    }
}

impl ColorSearch {
    pub fn set_target(&mut self, color: Rgb) {
        self.target = Color32::from_rgb(color[0], color[1], color[2]);
        self.hex = to_hex(color);
        self.stale = true;
    }

    pub fn is_building(&self) -> bool {
        self.build.is_some()
    }

    /// Download any missing sprites and (re)build the colour index in the background.
    pub fn start_build(&mut self, ctx: &egui::Context) -> Option<Event> {
        if self.build.is_some() {
            return None;
        }
        let Some(store) = Store::default_location().ok() else {
            return Some(Event::Status {
                text: "Oops! No cache folder available.".into(),
                error: true,
            });
        };
        let progress = Arc::new(Progress::default());
        let (tx, rx) = channel();
        let (p, ctx) = (progress.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = Client::new().and_then(|c| Index::build(&store, &c, &p));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
        self.build = Some(Build { progress, rx });
        Some(Event::Status {
            text: "Downloading sprites".into(),
            error: false,
        })
    }

    fn poll_build(&mut self) -> Option<Event> {
        let result = self.build.as_ref()?.rx.try_recv().ok()?;
        self.build = None;
        Some(match result {
            Ok(report) => {
                let n = report.index.entries.len();
                self.index = Some(report.index);
                self.thumbs.clear();
                self.stale = true;
                let text = match report.failed {
                    0 => format!("All done! {n} sprites ready for color search."),
                    f => format!("{n} sprites ready; {f} failed. Rebuild later to retry them."),
                };
                Event::Status {
                    text,
                    error: report.failed > 0,
                }
            }
            Err(e) => Event::Status {
                text: format!("Oops! {e:#}"),
                error: true,
            },
        })
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Event> {
        let event = self.poll_build();

        let picked = if ui.available_width() >= SIDE_BY_SIDE_MIN_WIDTH {
            // Picker in a left panel; results fill the remaining space.
            egui::Panel::left("picker")
                .exact_size(PICKER_SIDE + 44.0)
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    right: 12,
                    ..Default::default()
                }))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("picker")
                        .show(ui, |ui| self.picker_card(ui, PICKER_SIDE, false));
                });
            self.body(ui)
        } else {
            self.picker_card(ui, PICKER_SIDE_STACKED, true);
            ui.add_space(12.0);
            self.body(ui)
        };
        picked.or(event)
    }

    /// The colour picker plus hex/closeness controls, in a card.
    fn picker_card(&mut self, ui: &mut egui::Ui, side: f32, controls_beside: bool) {
        theme::card().show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let picker = |ui: &mut egui::Ui, this: &mut Self| {
                ui.scope(|ui| {
                    // egui sizes its inline picker from these spacing values.
                    ui.spacing_mut().slider_width = side;
                    ui.spacing_mut().interact_size.y = 26.0;
                    // Compact styling for the picker's own button row (format toggle,
                    // copy, R/G/B). At the app's default size, "R 255 G 255 B 255" is
                    // wider than the square and spills out of the column.
                    ui.spacing_mut().button_padding = egui::vec2(6.0, 3.0);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let small = egui::FontId::proportional(14.0);
                    let styles = &mut ui.style_mut().text_styles;
                    styles.insert(egui::TextStyle::Button, small.clone());
                    styles.insert(egui::TextStyle::Body, small);
                    if color_picker_color32(ui, &mut this.target, Alpha::Opaque) {
                        this.hex = to_hex(rgb(this.target));
                        this.stale = true;
                    }
                });
            };
            if controls_beside {
                ui.horizontal_top(|ui| {
                    picker(ui, self);
                    ui.add_space(8.0);
                    ui.vertical(|ui| self.controls(ui));
                });
            } else {
                picker(ui, self);
                ui.add_space(8.0);
                self.controls(ui);
            }
        });
    }

    /// Everything below/beside the picker: index status or the results grid.
    fn body(&mut self, ui: &mut egui::Ui) -> Option<Event> {
        if let Some(build) = &self.build {
            let done = build.progress.done.load(Ordering::Relaxed);
            let total = build.progress.total.load(Ordering::Relaxed).max(1);
            theme::card().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label("Collecting sprites…");
                ui.add(
                    egui::ProgressBar::new(done as f32 / total as f32)
                        .text(format!("{done} / {total}"))
                        .fill(theme::HIGHLIGHT),
                );
            });
            ui.ctx().request_repaint_after(Duration::from_millis(100));
            return None;
        }

        if self.index.is_none() {
            let mut event = None;
            theme::card().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label("To search by color, Pokepicker needs every sprite saved locally.");
                ui.label(
                    RichText::new("This is a one-time download of about 6 MB.").color(theme::MUTED),
                );
                ui.add_space(6.0);
                if ui.button("▶ DOWNLOAD SPRITES").clicked() {
                    event = self.start_build(ui.ctx());
                }
            });
            return event;
        }

        if self.stale {
            self.results = self
                .index
                .as_ref()
                .map_or_else(Vec::new, |i| i.search(rgb(self.target), self.tolerance));
            self.stale = false;
        }

        self.results_grid(ui).map(Event::Open)
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("HEX").color(theme::MUTED));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.hex)
                    .desired_width(90.0)
                    .font(egui::TextStyle::Monospace),
            );
            if edit.changed()
                && let Some(c) = parse_hex(&self.hex)
            {
                self.target = Color32::from_rgb(c[0], c[1], c[2]);
                self.stale = true;
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("CLOSENESS").color(theme::MUTED));
        ui.horizontal(|ui| {
            ui.small("exact");
            if ui
                .add(egui::Slider::new(&mut self.tolerance, 5.0..=50.0).show_value(false))
                .changed()
            {
                self.stale = true;
            }
            ui.small("loose");
        });

        if self.index.is_some() {
            ui.add_space(4.0);
            let shown = self.results.len().min(MAX_RESULTS);
            ui.label(RichText::new(format!("{shown} matches")).strong());
        }
    }

    /// Returns the name of a clicked result.
    fn results_grid(&mut self, ui: &mut egui::Ui) -> Option<String> {
        if self.results.is_empty() {
            theme::card().show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(
                    RichText::new("No Pokémon wear that color. Try loosening the closeness!")
                        .color(theme::MUTED),
                );
            });
            return None;
        }

        let mut picked = None;
        let results = std::mem::take(&mut self.results);
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                // Top-aligned so tiles of a row line up (`horizontal_wrapped` centres them).
                let grid = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
                ui.with_layout(grid, |ui| {
                    for m in results.iter().take(MAX_RESULTS) {
                        if self.tile(ui, m).clicked() {
                            picked = Some(m.name.clone());
                        }
                    }
                });
            });
        self.results = results;
        picked
    }

    fn tile(&mut self, ui: &mut egui::Ui, m: &Match) -> egui::Response {
        let thumb = self.thumb(ui.ctx(), m.id);
        // A Frame placed directly in a wrapping layout never wraps; reserving its
        // width with `allocate_ui` first lets the layout move it to the next row.
        let outer_width = TILE + 2.0 * (TILE_MARGIN as f32 + theme::BORDER);
        let frame = ui.allocate_ui(egui::vec2(outer_width, 0.0), |ui| {
            theme::card()
                .inner_margin(egui::Margin::same(TILE_MARGIN))
                .show(ui, |ui| {
                    ui.set_width(TILE);
                    ui.vertical_centered(|ui| {
                        match thumb {
                            Some(tex) => ui.add(
                                egui::Image::new(&tex).fit_to_exact_size(egui::vec2(TILE, TILE)),
                            ),
                            None => {
                                ui.allocate_response(egui::vec2(TILE, TILE), egui::Sense::hover())
                            }
                        };
                        ui.add(
                            egui::Label::new(RichText::new(m.name.to_uppercase()).small().strong())
                                .truncate(),
                        );
                        ui.horizontal(|ui| {
                            let c = m.closest;
                            egui::color_picker::show_color(
                                ui,
                                Color32::from_rgb(c[0], c[1], c[2]),
                                egui::vec2(14.0, 14.0),
                            );
                            ui.small(format!("{:.0}%", m.coverage * 100.0));
                        });
                    });
                })
                .response
        });
        let frame = frame.inner;
        let response = frame
            .interact(egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.hovered() {
            ui.painter().rect_stroke(
                frame.rect,
                8.0,
                egui::Stroke::new(2.5, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        response.on_hover_text(format!("No.{:03} · closest {}", m.id, to_hex(m.closest)))
    }

    fn thumb(&mut self, ctx: &egui::Context, id: u32) -> Option<TextureHandle> {
        let store = self.store.as_ref()?;
        self.thumbs
            .entry(id)
            .or_insert_with(|| {
                let bytes = std::fs::read(store.sprite_path(id)).ok()?;
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                Some(ctx.load_texture(format!("thumb-{id}"), color, TextureOptions::NEAREST))
            })
            .clone()
    }
}

fn rgb(c: Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}
