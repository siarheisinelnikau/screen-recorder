//! Shared visual roles and installed Windows typography; no recurring work.
use egui::{self, Color32, FontId, TextStyle};
pub const BACKGROUND: Color32 = Color32::from_rgb(244, 246, 249);
pub const SURFACE: Color32 = Color32::WHITE;
pub const TEXT: Color32 = Color32::from_rgb(29, 37, 51);
pub const SECONDARY: Color32 = Color32::from_rgb(91, 104, 123);
pub const BORDER: Color32 = Color32::from_rgb(206, 214, 226);
pub const CONTROL_TRACK: Color32 = Color32::from_rgb(128, 141, 162);
pub const PRIMARY: Color32 = Color32::from_rgb(49, 84, 220);
pub const ON_PRIMARY: Color32 = Color32::WHITE;
pub const SELECTION: Color32 = Color32::from_rgb(226, 234, 255);
pub const WARNING: Color32 = Color32::from_rgb(139, 86, 10);
pub const ERROR: Color32 = Color32::from_rgb(179, 36, 63);
pub const SUCCESS: Color32 = Color32::from_rgb(28, 112, 78);
pub fn panel(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::group(ui.style())
        .fill(SURFACE)
        .corner_radius(12)
        .inner_margin(16)
}
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = CONTROL_TRACK;
        ui.add(slider)
    })
    .inner
}
pub fn apply(ctx: &egui::Context) {
    // Use OS-installed fonts without redistributing proprietary font files.
    #[cfg(windows)]
    if let Ok(font) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system-ui".into(),
            std::sync::Arc::new(egui::FontData::from_owned(font)),
        );
        fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "system-ui".into());
        ctx.set_fonts(fonts);
    }
    ctx.set_theme(egui::ThemePreference::Light);
    ctx.set_visuals(egui::Visuals::light());
    ctx.style_mut_of(egui::Theme::Light, |style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 34.0;
        // Keep overflow discoverable without hover-driven fade animations.
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.scroll.foreground_color = true;
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(26.0));
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
        visuals.extreme_bg_color = SURFACE;
        visuals.selection.bg_fill = SELECTION;
        visuals.selection.stroke = egui::Stroke::new(1.5, PRIMARY);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        visuals.widgets.noninteractive.fg_stroke.color = SECONDARY;
        visuals.widgets.inactive.bg_fill = SURFACE;
        visuals.widgets.inactive.weak_bg_fill = SURFACE;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
        visuals.widgets.inactive.fg_stroke.color = SECONDARY;
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(234, 239, 250);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(234, 239, 250);
        visuals.widgets.active.bg_fill = SELECTION;
        visuals.widgets.active.weak_bg_fill = SELECTION;
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, PRIMARY);
        visuals.error_fg_color = ERROR;
        visuals.warn_fg_color = WARNING;
    });
}
