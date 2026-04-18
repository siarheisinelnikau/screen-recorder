//! Small labeled vector controls. No icon font, animation timer or platform access.
use crate::theme;
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
#[derive(Clone, Copy)]
pub enum Icon {
    Record,
    Edit,
    Settings,
    Display,
    Window,
    Region,
    Mic,
    Sound,
    Cursor,
    Click,
    Zoom,
    Export,
    Folder,
}
fn line(p: &egui::Painter, a: Pos2, b: Pos2, s: Stroke) {
    p.line_segment([a, b], s);
}
pub fn icon(p: &egui::Painter, rect: Rect, kind: Icon, color: Color32) {
    let q = |x: f32, y: f32| rect.min + vec2(x, y) * (rect.width() / 24.0);
    let s = Stroke::new(1.65, color);
    let path = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|&(x, y)| q(x, y)).collect(),
            s,
        ));
    };
    match kind {
        Icon::Display | Icon::Record => {
            p.rect_stroke(
                Rect::from_min_max(q(3., 4.), q(21., 17.)),
                3.,
                s,
                StrokeKind::Inside,
            );
            line(p, q(12., 17.), q(12., 21.), s);
            line(p, q(8., 21.), q(16., 21.), s);
            if matches!(kind, Icon::Record) {
                p.circle_filled(q(12., 10.5), 3. * rect.width() / 24., color);
            }
        }
        Icon::Window => {
            p.rect_stroke(
                Rect::from_min_max(q(3., 4.), q(21., 20.)),
                3.,
                s,
                StrokeKind::Inside,
            );
            line(p, q(3., 9.), q(21., 9.), s);
        }
        Icon::Region => {
            path(&[(9., 3.), (3., 3.), (3., 9.)]);
            path(&[(15., 3.), (21., 3.), (21., 9.)]);
            path(&[(3., 15.), (3., 21.), (9., 21.)]);
            path(&[(21., 15.), (21., 21.), (15., 21.)]);
        }
        Icon::Mic => {
            p.rect_stroke(
                Rect::from_min_max(q(9., 3.), q(15., 14.)),
                4.,
                s,
                StrokeKind::Inside,
            );
            path(&[
                (6., 10.),
                (6., 14.),
                (9., 17.),
                (15., 17.),
                (18., 14.),
                (18., 10.),
            ]);
            line(p, q(12., 17.), q(12., 21.), s);
            line(p, q(9., 21.), q(15., 21.), s);
        }
        Icon::Sound => {
            path(&[
                (10., 5.),
                (5., 9.),
                (2., 9.),
                (2., 15.),
                (5., 15.),
                (10., 19.),
                (10., 5.),
            ]);
            path(&[(15., 7.), (18., 10.), (18., 14.), (15., 17.)]);
            path(&[(19., 4.), (22., 9.), (22., 15.), (19., 20.)]);
        }
        Icon::Cursor => path(&[
            (5., 3.),
            (6., 20.),
            (11., 15.),
            (15., 21.),
            (18., 19.),
            (14., 13.),
            (21., 12.),
            (5., 3.),
        ]),
        Icon::Click => {
            p.circle_stroke(q(12., 12.), 7. * rect.width() / 24., s);
            p.circle_filled(q(12., 12.), 2. * rect.width() / 24., color);
            line(p, q(12., 1.), q(12., 3.), s);
            line(p, q(21., 3.), q(19., 5.), s);
            line(p, q(23., 12.), q(21., 12.), s);
        }
        Icon::Zoom => {
            p.circle_stroke(q(10., 10.), 6.5 * rect.width() / 24., s);
            line(p, q(15., 15.), q(22., 22.), s);
            line(p, q(7., 10.), q(13., 10.), s);
            line(p, q(10., 7.), q(10., 13.), s);
        }
        Icon::Edit => {
            path(&[
                (4., 16.),
                (16., 4.),
                (20., 8.),
                (8., 20.),
                (3., 21.),
                (4., 16.),
            ]);
            line(p, q(13., 7.), q(17., 11.), s);
        }
        Icon::Settings => {
            p.circle_stroke(q(12., 12.), 7. * rect.width() / 24., s);
            p.circle_stroke(q(12., 12.), 2.5 * rect.width() / 24., s);
            for n in 0..8 {
                let a = n as f32 * std::f32::consts::TAU / 8.;
                let d = vec2(a.cos(), a.sin());
                line(
                    p,
                    q(12., 12.) + d * 7. * rect.width() / 24.,
                    q(12., 12.) + d * 10. * rect.width() / 24.,
                    s,
                );
            }
        }
        Icon::Export => {
            path(&[(5., 9.), (5., 21.), (21., 21.), (21., 9.)]);
            path(&[(8., 8.), (13., 3.), (18., 8.)]);
            line(p, q(13., 3.), q(13., 16.), s);
        }
        Icon::Folder => {
            path(&[
                (3., 6.),
                (10., 6.),
                (12., 9.),
                (21., 9.),
                (21., 20.),
                (3., 20.),
                (3., 6.),
            ]);
        }
    }
}
fn frame(
    ui: &mut Ui,
    size: Vec2,
    label: &str,
    selected: bool,
    primary: bool,
) -> (Rect, Response, Color32) {
    let (r, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, label)
    });
    let ink = if primary {
        theme::ON_PRIMARY
    } else if selected {
        theme::PRIMARY
    } else {
        theme::TEXT
    };
    let fill = if primary {
        theme::PRIMARY
    } else if selected || response.hovered() {
        theme::SELECTION
    } else {
        theme::SURFACE
    };
    let opacity = if ui.is_enabled() { 1.0 } else { 0.5 };
    ui.painter().rect(
        r,
        10.,
        fill.gamma_multiply(opacity),
        Stroke::new(
            if response.has_focus() { 2. } else { 1. },
            if selected || response.has_focus() {
                theme::PRIMARY
            } else {
                theme::BORDER
            },
        ),
        StrokeKind::Inside,
    );
    (r, response, ink.gamma_multiply(opacity))
}
pub fn choice(ui: &mut Ui, kind: Icon, label: &str, selected: bool, width: f32) -> Response {
    let (r, response, ink) = frame(ui, vec2(width, 42.), label, selected, false);
    icon(
        ui.painter(),
        Rect::from_min_size(pos2(r.left() + 12., r.center().y - 10.), vec2(20., 20.)),
        kind,
        ink,
    );
    ui.painter().text(
        pos2(r.left() + 42., r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.),
        ink,
    );
    response
}
pub fn action(ui: &mut Ui, kind: Icon, label: &str, width: f32) -> Response {
    let (r, response, ink) = frame(ui, vec2(width, 44.), label, false, true);
    icon(
        ui.painter(),
        Rect::from_min_size(pos2(r.left() + 14., r.center().y - 10.), vec2(20., 20.)),
        kind,
        ink,
    );
    ui.painter().text(
        pos2(r.left() + 44., r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.),
        ink,
    );
    response
}
pub fn toggle(ui: &mut Ui, value: &mut bool, kind: Icon, label: &str, detail: &str) -> Response {
    let (r, mut response, ink) = frame(ui, vec2(ui.available_width(), 66.), label, false, false);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *value, label)
    });
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    icon(
        ui.painter(),
        Rect::from_min_size(r.min + vec2(14., 22.), vec2(22., 22.)),
        kind,
        ink,
    );
    ui.painter().text(
        r.min + vec2(48., 14.),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(15.),
        ink,
    );
    ui.painter().text(
        r.min + vec2(48., 37.),
        egui::Align2::LEFT_TOP,
        detail,
        egui::FontId::proportional(12.),
        theme::SECONDARY,
    );
    let track = Rect::from_center_size(pos2(r.right() - 34., r.center().y), vec2(36., 20.));
    ui.painter().rect_filled(
        track,
        10.,
        if *value {
            theme::PRIMARY
        } else {
            theme::CONTROL_TRACK
        },
    );
    ui.painter().circle_filled(
        pos2(
            if *value {
                track.right() - 10.
            } else {
                track.left() + 10.
            },
            track.center().y,
        ),
        7.,
        Color32::WHITE,
    );
    response
}
pub fn brand(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(34., 34.), Sense::hover());
    ui.painter().rect_filled(r, 9., theme::PRIMARY);
    icon(ui.painter(), r.shrink(6.), Icon::Record, Color32::WHITE);
    ui.label(
        egui::RichText::new("ScreenRecorder")
            .size(21.)
            .color(theme::TEXT),
    );
}
pub fn section(ui: &mut Ui, kind: Icon, label: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(20., 20.), Sense::hover());
        icon(ui.painter(), r, kind, theme::SECONDARY);
        ui.strong(label);
    });
}
pub fn nav(ui: &mut Ui, kind: Icon, label: &str, selected: bool, width: f32) -> Response {
    let (r, response) = ui.allocate_exact_size(vec2(width, 42.), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Tab, ui.is_enabled(), selected, label)
    });
    let color = if selected {
        theme::PRIMARY
    } else {
        theme::SECONDARY
    };
    if selected || response.hovered() {
        ui.painter().rect_filled(r, 10., theme::SELECTION);
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(r, 10., Stroke::new(2., theme::PRIMARY), StrokeKind::Inside);
    }
    icon(
        ui.painter(),
        Rect::from_min_size(r.min + vec2(12., 11.), vec2(20., 20.)),
        kind,
        color,
    );
    ui.painter().text(
        pos2(r.left() + 42., r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(14.),
        color,
    );
    response
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switch_can_be_activated_from_the_keyboard() {
        let ctx = egui::Context::default();
        let mut value = false;
        let input = |events| egui::RawInput {
            focused: true,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(500., 200.))),
            events,
            ..Default::default()
        };
        let mut id = egui::Id::NULL;
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            id = toggle(ui, &mut value, Icon::Mic, "Microphone", "Default input").id;
        });
        let _ = crate::raster::textures(&mut output.textures_delta);
        ctx.memory_mut(|m| m.request_focus(id));
        let mut output = ctx.run_ui(
            input(vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: Some(egui::Key::Space),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]),
            |ui| {
                toggle(ui, &mut value, Icon::Mic, "Microphone", "Default input");
            },
        );
        let _ = crate::raster::textures(&mut output.textures_delta);
        assert!(
            value,
            "Focused switches must support Space, in addition to pointer activation"
        );
    }
}
