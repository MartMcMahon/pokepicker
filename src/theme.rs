//! Friendly handheld-console look: warm cream paper, deep navy ink,
//! chunky rounded borders, a sunny yellow highlight and a cheerful red accent.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle,
};

pub const PAPER: Color32 = Color32::from_rgb(0xf7, 0xf3, 0xe6);
pub const CARD: Color32 = Color32::WHITE;
pub const INK: Color32 = Color32::from_rgb(0x2b, 0x2f, 0x4a);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x8d, 0xa3);
pub const HIGHLIGHT: Color32 = Color32::from_rgb(0xff, 0xd9, 0x5a);
pub const ACCENT: Color32 = Color32::from_rgb(0xe3, 0x50, 0x4a);
pub const SKY: Color32 = Color32::from_rgb(0xcf, 0xe4, 0xff);
/// Slider rails and scroll bars: a darker cream that reads on both PAPER and CARD.
pub const TRACK: Color32 = Color32::from_rgb(0xdd, 0xd6, 0xc0);

const ROUND: CornerRadius = CornerRadius::same(8);
/// Border width of `card()`.
pub const BORDER: f32 = 2.5;

/// A white dialog-box style card with a thick navy border.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(BORDER, INK))
        .corner_radius(ROUND)
        .inner_margin(Margin::same(14))
}

pub fn apply(ctx: &egui::Context) {
    // Ubuntu (the default proportional font) lacks glyphs like ▼; fall back to Hack.
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .push("Hack".into());
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(16.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(15.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
        ]
        .into();
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.slider_rail_height = 8.0;

        let v = &mut style.visuals;
        *v = egui::Visuals::light();
        v.override_text_color = Some(INK);
        v.panel_fill = PAPER;
        v.window_fill = CARD;
        v.extreme_bg_color = CARD;
        v.window_stroke = Stroke::new(2.5, INK);
        v.window_corner_radius = ROUND;
        v.menu_corner_radius = ROUND;
        v.window_shadow = Shadow::NONE;
        v.popup_shadow = Shadow::NONE;
        v.selection.bg_fill = SKY;
        v.selection.stroke = Stroke::new(2.0, INK);
        v.text_cursor.stroke = Stroke::new(2.0, ACCENT);

        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(6);
            w.bg_fill = CARD;
            w.weak_bg_fill = CARD;
            w.fg_stroke = Stroke::new(1.5, INK);
            w.bg_stroke = Stroke::new(2.0, INK);
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.5, MUTED);
        // egui paints slider rails (and scroll bar handles) with this fill.
        v.widgets.inactive.bg_fill = TRACK;
        v.widgets.hovered.weak_bg_fill = HIGHLIGHT;
        v.widgets.hovered.bg_fill = HIGHLIGHT;
        v.widgets.open.weak_bg_fill = HIGHLIGHT;
        v.widgets.active.weak_bg_fill = ACCENT;
        v.widgets.active.bg_fill = ACCENT;
        // egui also uses this colour for all `.strong()` text, so it must read on cream and white.
        v.widgets.active.fg_stroke = Stroke::new(1.5, INK);
    });
}
