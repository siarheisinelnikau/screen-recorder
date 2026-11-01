//! Shared light/dark visual roles and installed Windows typography; no recurring work.
use egui::{self, Color32, FontId, TextStyle};
use screen_recorder::settings::Theme;

#[derive(Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub border: Color32,
    pub control_track: Color32,
    pub primary: Color32,
    pub on_primary: Color32,
    pub selection: Color32,
    pub warning: Color32,
    pub error: Color32,
    pub success: Color32,
}
pub const LIGHT: Palette = Palette {
    background: Color32::from_rgb(244, 246, 249),
    surface: Color32::WHITE,
    text: Color32::from_rgb(29, 37, 51),
    secondary: Color32::from_rgb(91, 104, 123),
    border: Color32::from_rgb(206, 214, 226),
    control_track: Color32::from_rgb(128, 141, 162),
    primary: Color32::from_rgb(49, 84, 220),
    on_primary: Color32::WHITE,
    selection: Color32::from_rgb(226, 234, 255),
    warning: Color32::from_rgb(139, 86, 10),
    error: Color32::from_rgb(179, 36, 63),
    success: Color32::from_rgb(28, 112, 78),
};
pub const DARK: Palette = Palette {
    background: Color32::from_rgb(20, 25, 34),
    surface: Color32::from_rgb(30, 37, 49),
    text: Color32::from_rgb(235, 240, 248),
    secondary: Color32::from_rgb(171, 186, 207),
    border: Color32::from_rgb(66, 80, 102),
    control_track: Color32::from_rgb(91, 110, 138),
    primary: Color32::from_rgb(151, 176, 255),
    on_primary: Color32::from_rgb(18, 29, 64),
    selection: Color32::from_rgb(49, 66, 106),
    warning: Color32::from_rgb(246, 194, 100),
    error: Color32::from_rgb(255, 143, 160),
    success: Color32::from_rgb(117, 218, 172),
};
pub fn for_dark(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}
pub fn palette(ui: &egui::Ui) -> Palette {
    for_dark(ui.visuals().dark_mode)
}
pub fn panel(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::group(ui.style())
        .fill(palette(ui).surface)
        .corner_radius(12)
        .inner_margin(16)
}
pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> egui::Response {
    let track = palette(ui).control_track;
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = track;
        ui.add(slider)
    })
    .inner
}
pub fn choose(ctx: &egui::Context, choice: Theme) {
    ctx.set_theme(match choice {
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
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
    for mode in [egui::Theme::Light, egui::Theme::Dark] {
        let p = for_dark(mode == egui::Theme::Dark);
        ctx.style_mut_of(mode, |style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.spacing.interact_size.y = 34.0;
            style.spacing.scroll = egui::style::ScrollStyle::solid();
            style.spacing.scroll.foreground_color = true;
            for (kind, size) in [
                (TextStyle::Heading, 26.0),
                (TextStyle::Body, 15.0),
                (TextStyle::Button, 15.0),
                (TextStyle::Small, 13.0),
            ] {
                style.text_styles.insert(kind, FontId::proportional(size));
            }
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(14.0));
            let v = &mut style.visuals;
            *v = if mode == egui::Theme::Dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            };
            v.override_text_color = Some(p.text);
            v.weak_text_color = Some(p.secondary);
            v.panel_fill = p.background;
            v.window_fill = p.surface;
            v.extreme_bg_color = p.surface;
            v.selection.bg_fill = p.selection;
            v.selection.stroke = egui::Stroke::new(1.5, p.primary);
            v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.border);
            v.widgets.noninteractive.fg_stroke.color = p.secondary;
            v.widgets.inactive.bg_fill = p.surface;
            v.widgets.inactive.weak_bg_fill = p.surface;
            v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.border);
            v.widgets.inactive.fg_stroke.color = p.text;
            v.widgets.hovered.bg_fill = p.selection;
            v.widgets.hovered.weak_bg_fill = p.selection;
            v.widgets.active.bg_fill = p.selection;
            v.widgets.active.weak_bg_fill = p.selection;
            v.widgets.active.bg_stroke = egui::Stroke::new(1.5, p.primary);
            v.error_fg_color = p.error;
            v.warn_fg_color = p.warning;
        });
    }
    choose(ctx, Theme::Light);
}
