//! Original vector command icons. No font glyphs, external assets or network loading.
use eframe::egui::{
    self, pos2, vec2, Color32, Rect, Sense, Stroke, StrokeKind, TextStyle, WidgetInfo, WidgetType,
};

#[derive(Clone, Copy, Debug)]
pub enum Icon {
    Capture,
    Select,
    Highlight,
    Pixelate,
    Redact,
    Crop,
    Arrow,
    Rectangle,
    Pen,
    Text,
    Step,
    Undo,
    Redo,
    Copy,
    Save,
    Close,
    Cancel,
}

/// One labelled hit target and one accessibility node; the strokes are decorative.
pub fn button(ui: &mut egui::Ui, label: &str, icon: Icon, selected: bool) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 43.0, 34.0_f32.max(galley.size().y + 12.0));
    let (rect, response) = ui.allocate_at_least(size, Sense::click());
    response.widget_info(|| {
        if matches!(
            icon,
            Icon::Capture
                | Icon::Undo
                | Icon::Redo
                | Icon::Copy
                | Icon::Save
                | Icon::Close
                | Icon::Cancel
        ) {
            WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label)
        } else {
            WidgetInfo::selected(WidgetType::Button, ui.is_enabled(), selected, label)
        }
    });
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let stroke = if response.has_focus() {
            ui.visuals().selection.stroke
        } else {
            visuals.bg_stroke
        };
        ui.painter()
            .rect(rect, 6.0, visuals.weak_bg_fill, stroke, StrokeKind::Inside);
        let colour = visuals.text_color();
        paint(
            ui.painter(),
            Rect::from_min_size(
                pos2(rect.left() + 9.0, rect.center().y - 9.0),
                vec2(18.0, 18.0),
            ),
            icon,
            colour,
        );
        ui.painter().galley(
            pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0),
            galley,
            colour,
        );
    }
    response
}

fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, colour: Color32) {
    let point = |x: f32, y: f32| {
        pos2(
            rect.left() + x * rect.width() / 24.0,
            rect.top() + y * rect.height() / 24.0,
        )
    };
    let stroke = Stroke::new(1.5, colour);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    let outline = |x: f32, y: f32, w: f32, h: f32| {
        painter.rect_stroke(
            Rect::from_min_max(point(x, y), point(x + w, y + h)),
            1.0,
            stroke,
            StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Capture => {
            path(&[(8., 3.), (3., 3.), (3., 8.)]);
            path(&[(16., 3.), (21., 3.), (21., 8.)]);
            path(&[(3., 16.), (3., 21.), (8., 21.)]);
            path(&[(16., 21.), (21., 21.), (21., 16.)]);
            line((8., 12.), (16., 12.));
            line((12., 8.), (12., 16.));
        }
        Icon::Select => path(&[(5., 3.), (18., 12.), (12., 13.), (9., 20.), (5., 3.)]),
        Icon::Highlight => {
            path(&[(4., 16.), (13., 4.), (19., 9.), (10., 20.), (4., 16.)]);
            line((3., 21.), (13., 21.));
            line((7., 13.), (13., 18.));
        }
        Icon::Pixelate => {
            for x in [3., 13.] {
                for y in [3., 13.] {
                    outline(x, y, 7., 7.);
                }
            }
        }
        Icon::Redact => {
            outline(2., 6., 20., 13.);
            for y in [9., 12., 15.] {
                line((5., y), (19., y));
            }
        }
        Icon::Crop => {
            path(&[(7., 2.), (7., 17.), (22., 17.)]);
            path(&[(2., 7.), (17., 7.), (17., 22.)]);
        }
        Icon::Arrow => {
            line((4., 20.), (20., 4.));
            path(&[(10., 4.), (20., 4.), (20., 14.)]);
        }
        Icon::Rectangle => outline(3., 5., 18., 14.),
        Icon::Pen => path(&[
            (3., 20.),
            (7., 8.),
            (11., 4.),
            (14., 5.),
            (13., 10.),
            (6., 18.),
            (11., 14.),
            (15., 13.),
            (17., 17.),
            (21., 15.),
        ]),
        Icon::Text => {
            line((4., 4.), (20., 4.));
            line((12., 4.), (12., 21.));
            line((7., 21.), (17., 21.));
        }
        Icon::Step => {
            painter.circle_stroke(point(12., 12.), rect.width() * 0.4, stroke);
            path(&[(9., 9.), (12., 6.), (12., 18.)]);
            line((9., 18.), (15., 18.));
        }
        Icon::Undo => {
            path(&[(9., 4.), (4., 9.), (9., 14.)]);
            path(&[
                (4., 9.),
                (14., 9.),
                (18., 11.),
                (20., 15.),
                (18., 19.),
                (13., 20.),
            ]);
        }
        Icon::Redo => {
            path(&[(15., 4.), (20., 9.), (15., 14.)]);
            path(&[
                (20., 9.),
                (10., 9.),
                (6., 11.),
                (4., 15.),
                (6., 19.),
                (11., 20.),
            ]);
        }
        Icon::Copy => {
            outline(8., 8., 13., 13.);
            path(&[(16., 5.), (16., 2.), (2., 2.), (2., 16.), (5., 16.)]);
        }
        Icon::Save => {
            path(&[
                (3., 3.),
                (17., 3.),
                (21., 7.),
                (21., 21.),
                (3., 21.),
                (3., 3.),
            ]);
            outline(7., 3., 8., 6.);
            outline(7., 14., 10., 7.);
        }
        Icon::Close => {
            path(&[(9., 3.), (20., 3.), (20., 21.), (9., 21.)]);
            line((3., 7.), (12., 16.));
            line((12., 7.), (3., 16.));
        }
        Icon::Cancel => {
            line((5., 5.), (19., 19.));
            line((19., 5.), (5., 19.));
        }
    }
}
