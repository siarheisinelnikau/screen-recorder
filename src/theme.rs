//! Shared visual roles. No timers, platform access or rendering is performed here.
use eframe::egui::{self, Color32, FontId, TextStyle};
pub const BACKGROUND: Color32 = Color32::from_rgb(19, 25, 33);
pub const SURFACE: Color32 = Color32::from_rgb(27, 35, 45);
pub const TEXT: Color32 = Color32::from_rgb(237, 242, 247);
pub const SECONDARY: Color32 = Color32::from_rgb(171, 186, 201);
pub const BORDER: Color32 = Color32::from_rgb(65, 80, 95);
pub const ACCENT: Color32 = Color32::from_rgb(122, 222, 216);
pub const PRIMARY: Color32 = Color32::from_rgb(25, 106, 115);
pub const WARNING: Color32 = Color32::from_rgb(231, 199, 125);
pub const ERROR: Color32 = Color32::from_rgb(239, 148, 155);
pub const SUCCESS: Color32 = Color32::from_rgb(141, 218, 178);
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        // Keep overflow discoverable without hover-driven fade animations.
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(22.0));
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(15.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(14.0));
        let visuals = &mut style.visuals;
        visuals.override_text_color = Some(TEXT);
        visuals.weak_text_color = Some(SECONDARY);
        visuals.panel_fill = BACKGROUND;
        visuals.window_fill = SURFACE;
        visuals.selection.bg_fill = PRIMARY;
        visuals.selection.stroke.color = TEXT;
        visuals.widgets.inactive.bg_fill = SURFACE;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 64, 73);
        visuals.widgets.active.bg_fill = PRIMARY;
        visuals.error_fg_color = ERROR;
        visuals.warn_fg_color = WARNING;
    });
}
