//! The app's icons, painted with egui's own shapes.
//!
//! Drawn rather than typed. Every character the bar once tried — 🔍, 🎓,
//! 🗺, 👤 — came from a fallback font in a different typeface, and the map
//! glyph existed in only the crudest of them. Shapes always match the colour
//! beside them, scale to any size, and cannot go missing.
//!
//! They are outline icons on a 24-unit grid with a 1.8-unit stroke, so they
//! sit at the weight of the text next to them; a few carry one filled part
//! (the map's last square, the streak's ball) as their accent.

use eframe::egui::{self, Color32, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // the tab bar
    Home,
    Study,
    Map,
    Search,
    Person,
    // actions
    Speaker,
    Check,
    Cross,
    Plus,
    ChevronLeft,
    ChevronRight,
    ChevronDown,
    ChevronUp,
    // settings rows and cards
    Ball,
    Bell,
    Target,
    Sun,
    Moon,
    Letters,
    Lines,
    Warning,
    Trend,
    Arrow,
    Info,
    Sync,
    Scan,
}

/// Paints `icon` to fill `rect`, in `color`.
///
/// `background` is what is behind it, for the one icon (the moon) that is
/// cut out of a filled shape.
pub fn paint_icon(
    painter: &egui::Painter,
    rect: Rect,
    icon: Icon,
    color: Color32,
    background: Color32,
) {
    // Everything below is written in 24-unit coordinates and mapped onto
    // `rect`, so the shapes keep their proportions at any size.
    let unit = rect.width().min(rect.height()) / 24.0;
    let origin = rect.center() - egui::vec2(12.0 * unit, 12.0 * unit);
    let at = |x: f32, y: f32| origin + egui::vec2(x * unit, y * unit);
    let stroke = Stroke::new((1.8 * unit).max(1.2), color);
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let closed = |points: &[(f32, f32)]| {
        painter.add(Shape::closed_line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    let ring = |x: f32, y: f32, r: f32| painter.circle_stroke(at(x, y), r * unit, stroke);
    let dot = |x: f32, y: f32, r: f32| painter.circle_filled(at(x, y), r * unit, color);

    match icon {
        Icon::Home => {
            closed(&[
                (4.0, 10.5),
                (12.0, 4.0),
                (20.0, 10.5),
                (20.0, 20.0),
                (4.0, 20.0),
            ]);
            line(&[(10.0, 20.0), (10.0, 15.0), (14.0, 15.0), (14.0, 20.0)]);
        }
        Icon::Study => {
            // A mortarboard: the board, the cap under it, the tassel.
            closed(&[(2.5, 9.0), (12.0, 4.5), (21.5, 9.0), (12.0, 13.5)]);
            line(&[
                (6.5, 11.2),
                (6.5, 15.5),
                (9.0, 17.6),
                (12.0, 18.3),
                (15.0, 17.6),
                (17.5, 15.5),
                (17.5, 11.2),
            ]);
            line(&[(21.5, 9.0), (21.5, 14.0)]);
        }
        Icon::Map => {
            // The knowledge map is a grid of squares, so the icon is one too;
            // the filled one echoes the map's coloured squares.
            for (x, y) in [(4.0, 4.0), (13.0, 4.0), (4.0, 13.0), (13.0, 13.0)] {
                let square = Rect::from_min_max(at(x, y), at(x + 7.0, y + 7.0));
                if (x, y) == (13.0, 13.0) {
                    painter.rect_filled(square, 1.5 * unit, color);
                } else {
                    painter.rect_stroke(square, 1.5 * unit, stroke, egui::StrokeKind::Middle);
                }
            }
        }
        Icon::Search => {
            ring(11.0, 11.0, 6.5);
            line(&[(16.0, 16.0), (20.0, 20.0)]);
        }
        Icon::Person => {
            ring(12.0, 8.5, 3.8);
            line(&[
                (4.5, 20.0),
                (6.0, 17.3),
                (8.8, 15.2),
                (12.0, 14.5),
                (15.2, 15.2),
                (18.0, 17.3),
                (19.5, 20.0),
            ]);
        }
        Icon::Speaker => {
            closed(&[
                (4.0, 9.5),
                (7.5, 9.5),
                (12.0, 5.5),
                (12.0, 18.5),
                (7.5, 14.5),
                (4.0, 14.5),
            ]);
            arc(painter, at(12.6, 12.0), 4.0 * unit, -44.0, 44.0, stroke);
            arc(painter, at(12.6, 12.0), 7.5 * unit, -44.0, 44.0, stroke);
        }
        Icon::Check => line(&[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)]),
        Icon::Cross => {
            line(&[(6.5, 6.5), (17.5, 17.5)]);
            line(&[(17.5, 6.5), (6.5, 17.5)]);
        }
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::ChevronLeft => line(&[(14.5, 5.5), (8.0, 12.0), (14.5, 18.5)]),
        Icon::ChevronRight => line(&[(9.5, 5.5), (16.0, 12.0), (9.5, 18.5)]),
        Icon::ChevronDown => line(&[(6.0, 9.5), (12.0, 15.5), (18.0, 9.5)]),
        Icon::ChevronUp => line(&[(6.0, 14.5), (12.0, 8.5), (18.0, 14.5)]),
        Icon::Ball => {
            // The logo's ball on its tee: the streak's mark.
            dot(12.0, 8.0, 5.0);
            painter.add(Shape::convex_polygon(
                vec![
                    at(8.5, 14.5),
                    at(15.5, 14.5),
                    at(13.5, 17.0),
                    at(10.5, 17.0),
                ],
                color,
                Stroke::NONE,
            ));
            painter.rect_filled(
                Rect::from_min_max(at(10.5, 16.8), at(13.5, 21.0)),
                0.0,
                color,
            );
        }
        Icon::Bell => {
            let mut points = vec![at(6.0, 16.5), at(6.0, 11.0)];
            points.extend(arc_points(at(12.0, 11.0), 6.0 * unit, 180.0, 360.0));
            points.extend([at(18.0, 16.5), at(19.5, 18.0), at(4.5, 18.0), at(6.0, 16.5)]);
            painter.add(Shape::line(points, stroke));
            line(&[(10.0, 20.8), (14.0, 20.8)]);
        }
        Icon::Target => {
            ring(12.0, 12.0, 8.0);
            ring(12.0, 12.0, 3.5);
        }
        Icon::Sun => {
            ring(12.0, 12.0, 4.0);
            for i in 0..8 {
                let (sin, cos) = (i as f32 * 45.0f32).to_radians().sin_cos();
                line(&[
                    (12.0 + cos * 7.0, 12.0 + sin * 7.0),
                    (12.0 + cos * 9.0, 12.0 + sin * 9.0),
                ]);
            }
        }
        Icon::Moon => {
            // A crescent: a disc with a second disc taken out of it.
            painter.circle_filled(at(11.5, 12.5), 7.5 * unit, color);
            painter.circle_filled(at(15.5, 9.5), 6.5 * unit, background);
        }
        Icon::Letters => {
            // "Aa": a capital A beside a lowercase one.
            line(&[(3.5, 18.0), (8.0, 6.0), (12.5, 18.0)]);
            line(&[(5.2, 13.5), (10.8, 13.5)]);
            ring(17.0, 14.5, 3.0);
            line(&[(20.0, 11.5), (20.0, 18.0)]);
        }
        Icon::Lines => {
            line(&[(5.0, 6.5), (19.0, 6.5)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
            line(&[(5.0, 17.5), (14.0, 17.5)]);
        }
        Icon::Warning => {
            closed(&[(12.0, 4.0), (21.0, 19.5), (3.0, 19.5)]);
            line(&[(12.0, 10.0), (12.0, 14.0)]);
            dot(12.0, 16.8, 1.1);
        }
        Icon::Trend => line(&[(4.0, 18.0), (9.0, 12.0), (13.0, 15.0), (20.0, 7.0)]),
        Icon::Arrow => {
            line(&[(5.0, 12.0), (18.0, 12.0)]);
            line(&[(13.0, 7.0), (18.0, 12.0), (13.0, 17.0)]);
        }
        Icon::Info => {
            ring(12.0, 12.0, 8.5);
            line(&[(12.0, 11.0), (12.0, 16.5)]);
            dot(12.0, 7.9, 1.1);
        }
        Icon::Sync => {
            // Two arrows chasing each other round a circle.
            arc(painter, at(12.0, 12.0), 7.0 * unit, 200.0, 330.0, stroke);
            arc(painter, at(12.0, 12.0), 7.0 * unit, 20.0, 150.0, stroke);
            line(&[(18.1, 4.9), (18.1, 8.5), (14.6, 8.5)]);
            line(&[(5.9, 19.1), (5.9, 15.5), (9.4, 15.5)]);
        }
        Icon::Scan => {
            for y in [7.0, 12.0, 17.0] {
                line(&[(4.0, y), (6.5, y)]);
            }
            line(&[(10.0, 7.5), (11.5, 9.0), (15.0, 5.5)]);
            line(&[(10.0, 12.0), (20.0, 12.0)]);
            line(&[(10.0, 17.0), (20.0, 17.0)]);
        }
    }
}

/// The points of a circular arc, which the painter has no primitive for.
fn arc_points(center: Pos2, radius: f32, from_deg: f32, to_deg: f32) -> Vec<Pos2> {
    const STEPS: usize = 16;
    (0..=STEPS)
        .map(|i| {
            let t = from_deg + (to_deg - from_deg) * i as f32 / STEPS as f32;
            let (sin, cos) = t.to_radians().sin_cos();
            center + egui::vec2(cos * radius, sin * radius)
        })
        .collect()
}

fn arc(painter: &egui::Painter, center: Pos2, radius: f32, from: f32, to: f32, stroke: Stroke) {
    painter.add(Shape::line(arc_points(center, radius, from, to), stroke));
}
