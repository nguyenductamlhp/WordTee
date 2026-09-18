//! The screens, and the bits of chrome they share.

pub mod home;
pub mod lookup;
pub mod map;
pub mod profile;
pub mod study;

use eframe::egui::{self, Color32, RichText};

use crate::dict::{Band, Pos};
use crate::progress::State;

/// Picks the value for the theme in use.
///
/// The two themes need different colours, not one shared mid-tone: a blue
/// legible on near-black washes out on white, and an amber legible on white
/// goes muddy on black.
fn pick(ui: &egui::Ui, dark: Color32, light: Color32) -> Color32 {
    if ui.visuals().dark_mode { dark } else { light }
}

/// The app's accent, inherited from the original tap-counter build.
pub fn accent(ui: &egui::Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0x4d, 0xb6, 0xf5),
        Color32::from_rgb(0x0b, 0x6f, 0xc2),
    )
}

pub fn good(ui: &egui::Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0x66, 0xbb, 0x6a),
        Color32::from_rgb(0x2e, 0x7d, 0x32),
    )
}

pub fn warn(ui: &egui::Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0xf5, 0xb3, 0x41),
        Color32::from_rgb(0xa5, 0x62, 0x00),
    )
}

pub fn bad(ui: &egui::Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0xef, 0x53, 0x50),
        Color32::from_rgb(0xc6, 0x28, 0x28),
    )
}

/// Secondary text: quiet, but still clear of the contrast floor either way.
pub fn muted(ui: &egui::Ui) -> Color32 {
    pick(ui, Color32::from_gray(0x9a), Color32::from_gray(0x5e))
}

// Spec 2.3's four map colours. These are fills rather than text, so the three
// coloured ones hold up on either theme; only "unexplored" has to flip, since
// it is the empty part of the bar and has to read as background.
pub fn c_known(_ui: &egui::Ui) -> Color32 {
    Color32::from_rgb(0x2e, 0x7d, 0x32)
}

pub fn c_assumed(_ui: &egui::Ui) -> Color32 {
    Color32::from_rgb(0x7c, 0xb3, 0x42)
}

pub fn c_learning(ui: &egui::Ui) -> Color32 {
    pick(
        ui,
        Color32::from_rgb(0xf5, 0xb3, 0x41),
        Color32::from_rgb(0xe8, 0x9c, 0x0e),
    )
}

pub fn c_unexplored(ui: &egui::Ui) -> Color32 {
    pick(ui, Color32::from_gray(0x3c), Color32::from_gray(0xdc))
}

/// The colour a state gets on the map and on its chip.
pub fn state_color(ui: &egui::Ui, state: State) -> Color32 {
    match state {
        State::Known | State::Mastered => c_known(ui),
        State::AssumedKnown => c_assumed(ui),
        State::Learning | State::Review => c_learning(ui),
        State::Unexplored => c_unexplored(ui),
    }
}

/// Blends `color` towards the panel behind it.
///
/// One expression covers both themes: the backdrop is dark in one and light in
/// the other, so the same call gives a deep tint on dark and a pale one on
/// light. Multiplying the colour's own gamma only ever darkens, which is why
/// the chips would have been invisible on white.
fn toward_background(ui: &egui::Ui, color: Color32, amount: f32) -> Color32 {
    let bg = ui.visuals().panel_fill;
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * amount).round() as u8;
    Color32::from_rgb(
        mix(bg.r(), color.r()),
        mix(bg.g(), color.g()),
        mix(bg.b(), color.b()),
    )
}

/// A small filled pill, the app's one repeated label shape.
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    if text.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(toward_background(ui, color, 0.22))
        .stroke(egui::Stroke::new(1.0, toward_background(ui, color, 0.55)))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

pub fn state_chip(ui: &mut egui::Ui, state: State) {
    let color = state_color(ui, state);
    chip(ui, state.label(), color);
}

pub fn pos_chip(ui: &mut egui::Ui, pos: Pos) {
    let color = muted(ui);
    chip(ui, pos.short(), color);
}

/// Spec 1.3's commonness indicator: four named tiers rather than one bar,
/// because an evenly divided 1–25.000 bar makes rank 500 and rank 2.000 look
/// like neighbours.
pub fn band_chip(ui: &mut egui::Ui, band: Band, rank: u32) {
    let color = match band {
        Band::Core => good(ui),
        Band::Advanced => accent(ui),
        Band::Academic => warn(ui),
        Band::Rare => muted(ui),
    };
    let text = match rank {
        0 => band.label().to_owned(),
        r => format!("{} · #{}", band.label(), thousands(r)),
    };
    chip(ui, &text, color);
}

/// 25000 -> "25,000".
///
/// The spec writes its numbers the Vietnamese way (25.000); the interface is
/// in English, so the group separator follows it.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A titled block, used for every section of the word page.
pub fn section<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .color(muted(ui))
            .strong(),
    );
    ui.add_space(3.0);
    body(ui)
}

/// A card with a subtle background, the shape used for senses and questions.
///
/// Returns the frame's response so a caller can make the whole card tappable.
pub fn card<R>(
    ui: &mut egui::Ui,
    accent: Option<Color32>,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let mut frame = egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(10)
        .inner_margin(12);
    if let Some(color) = accent {
        frame = frame.stroke(egui::Stroke::new(1.5, toward_background(ui, color, 0.8)));
    }
    frame.show(ui, body)
}

/// Turns the area a [`card`] drew into a tappable target.
pub fn card_clicked<R>(ui: &mut egui::Ui, card: &egui::InnerResponse<R>) -> bool {
    let response = &card.response;
    ui.interact(response.rect, response.id.with("tap"), egui::Sense::click())
        .clicked()
}

/// A full-width button, big enough for a thumb.
pub fn wide_button(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    ui.add_sized(
        [ui.available_width(), 42.0],
        egui::Button::new(RichText::new(text).color(color).strong()),
    )
}

/// Draws the four-colour progress bar of spec 2.3 for one knowledge block.
///
/// `counts` is indexed by [`State`]. Assumed-known is hatched, as the spec
/// asks, so an inferred segment never reads as a confirmed one.
pub fn progress_bar(ui: &mut egui::Ui, counts: [u32; 6], height: f32) -> egui::Response {
    let total: u32 = counts.iter().sum();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    // Resolved before `ui.painter()` borrows `ui`.
    let empty = c_unexplored(ui);
    // Confirmed first, then inferred, then in progress — left to right.
    let order = [
        (State::Mastered, c_known(ui)),
        (State::Known, c_known(ui)),
        (State::AssumedKnown, c_assumed(ui)),
        (State::Review, c_learning(ui)),
        (State::Learning, c_learning(ui)),
    ];
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, empty);
    if total == 0 {
        return response;
    }
    let mut x = rect.left();
    for (state, color) in order {
        let width = rect.width() * counts[state as usize] as f32 / total as f32;
        if width <= 0.0 {
            continue;
        }
        let part = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(width, height));
        painter.rect_filled(part, 0.0, color);
        if state == State::AssumedKnown {
            hatch(painter, part);
        }
        x += width;
    }
    response
}

/// Diagonal hatching, for the "inferred, not confirmed" segment.
fn hatch(painter: &egui::Painter, rect: egui::Rect) {
    let stroke = egui::Stroke::new(1.0, Color32::from_black_alpha(60));
    let mut x = rect.left() - rect.height();
    while x < rect.right() {
        let a = egui::pos2(x.max(rect.left()), rect.bottom());
        let b = egui::pos2((x + rect.height()).min(rect.right()), rect.top());
        if a.x < rect.right() && b.x > rect.left() {
            painter.line_segment([a, b], stroke);
        }
        x += 5.0;
    }
}

// -------------------------------------------------------------------------
// the bottom bar's icons
// -------------------------------------------------------------------------

/// The four icons in the navigation bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Home,
    Search,
    Study,
    Map,
    Person,
    /// Play the audio at normal speed.
    Speaker,
    /// Play it slowly: the same speaker, one wave instead of two.
    SpeakerSlow,
}

/// Paints `icon` to fill `rect`.
///
/// Drawn rather than typed. Every character the bar tried — 🔍, 🎓, 🗺, 👤 —
/// came from a fallback font in a different typeface, and the map glyph existed
/// in only the crudest of them. Shapes take about as many lines, always match
/// the text colour beside them, scale to any size, and cannot go missing.
pub fn paint_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: Icon,
    color: Color32,
    background: Color32,
) {
    // Everything below is expressed in a unit square and mapped onto `rect`,
    // so the shapes stay in proportion at any size.
    let at = |x: f32, y: f32| rect.min + egui::vec2(rect.width() * x, rect.height() * y);
    let unit = rect.width().min(rect.height());
    let line = egui::Stroke::new((unit * 0.09).max(1.5), color);

    match icon {
        Icon::Home => {
            // A roof over a body, the body sitting under the eaves.
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.5, 0.06), at(0.98, 0.48), at(0.02, 0.48)],
                color,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(0.16, 0.44),
                    at(0.84, 0.44),
                    at(0.84, 0.94),
                    at(0.16, 0.94),
                ],
                color,
                egui::Stroke::NONE,
            ));
            // A doorway, punched out in the panel colour behind it.
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(0.40, 0.62),
                    at(0.60, 0.62),
                    at(0.60, 0.94),
                    at(0.40, 0.94),
                ],
                background,
                egui::Stroke::NONE,
            ));
        }
        Icon::Search => {
            painter.circle_stroke(at(0.42, 0.42), unit * 0.27, line);
            painter.line_segment([at(0.63, 0.63), at(0.88, 0.88)], line);
        }
        Icon::Study => {
            // A mortarboard: the flat top, then the cap under it and a tassel.
            painter.add(egui::Shape::convex_polygon(
                vec![at(0.5, 0.14), at(0.97, 0.38), at(0.5, 0.62), at(0.03, 0.38)],
                color,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(0.24, 0.50),
                    at(0.76, 0.50),
                    at(0.76, 0.72),
                    at(0.24, 0.72),
                ],
                color,
                egui::Stroke::NONE,
            ));
            painter.line_segment([at(0.90, 0.42), at(0.90, 0.78)], line);
        }
        Icon::Map => {
            // The knowledge map is a grid of blocks, so the icon is one too.
            let (cell, gap) = (0.26, 0.11);
            for row in 0..3 {
                for col in 0..3 {
                    let x = 0.06 + col as f32 * (cell + gap);
                    let y = 0.06 + row as f32 * (cell + gap);
                    let square = egui::Rect::from_min_max(at(x, y), at(x + cell, y + cell));
                    // The filled diagonal keeps it from reading as a plain
                    // grid, and echoes the map's own part-filled bars.
                    if (row + col) % 2 == 0 {
                        painter.rect_filled(square, unit * 0.04, color);
                    } else {
                        painter.rect_stroke(
                            square,
                            unit * 0.04,
                            egui::Stroke::new((unit * 0.06).max(1.0), color),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
        Icon::Speaker | Icon::SpeakerSlow => {
            // A speaker: the neck, then the cone.
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(0.04, 0.37),
                    at(0.24, 0.37),
                    at(0.24, 0.63),
                    at(0.04, 0.63),
                ],
                color,
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    at(0.22, 0.38),
                    at(0.48, 0.10),
                    at(0.48, 0.90),
                    at(0.22, 0.62),
                ],
                color,
                egui::Stroke::NONE,
            ));
            // One wave for the slow button, two for the normal one — fewer
            // waves reads as "less", and the tooltip carries the exact speed.
            arc(painter, at(0.46, 0.5), unit * 0.22, -55.0, 55.0, line);
            if icon == Icon::Speaker {
                arc(painter, at(0.46, 0.5), unit * 0.38, -55.0, 55.0, line);
            }
        }
        Icon::Person => {
            painter.circle_filled(at(0.5, 0.28), unit * 0.185, color);
            // Shoulders: a rounded slab, clipped flat at the bottom.
            painter.rect_filled(
                egui::Rect::from_min_max(at(0.18, 0.60), at(0.82, 0.94)),
                unit * 0.30,
                color,
            );
        }
    }
}

/// A circular arc, which the painter has no primitive for.
fn arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    from_deg: f32,
    to_deg: f32,
    stroke: egui::Stroke,
) {
    const STEPS: usize = 14;
    let points = (0..=STEPS)
        .map(|i| {
            let t = from_deg + (to_deg - from_deg) * i as f32 / STEPS as f32;
            let (sin, cos) = t.to_radians().sin_cos();
            center + egui::vec2(cos * radius, sin * radius)
        })
        .collect();
    painter.add(egui::Shape::line(points, stroke));
}

/// A small square button carrying one icon.
pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> egui::Response {
    const SIDE: f32 = 28.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIDE, SIDE), egui::Sense::click());
    // Borrow egui's own hover and press colours so it behaves like a button.
    let visuals = *ui.style().interact(&response);
    ui.painter().rect(
        rect,
        visuals.corner_radius,
        visuals.weak_bg_fill,
        visuals.bg_stroke,
        egui::StrokeKind::Inside,
    );
    paint_icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(SIDE * 0.58)),
        icon,
        visuals.fg_stroke.color,
        visuals.weak_bg_fill,
    );
    response.on_hover_text(tooltip)
}

/// One tab — icon over caption — filling its column.
pub fn tab_button(ui: &mut egui::Ui, icon: Icon, selected: bool, label: &str) -> egui::Response {
    const ICON: f32 = 22.0;
    const CAPTION: f32 = 11.0;

    let height = ICON + CAPTION + 13.0;
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let color = if selected { accent(ui) } else { muted(ui) };
    if selected {
        let fill = toward_background(ui, color, 0.20);
        ui.painter().rect_filled(rect, 9.0, fill);
    }

    let icon_box = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + 5.0 + ICON / 2.0),
        egui::vec2(ICON, ICON),
    );
    // The cut-out in the home icon shows whatever is behind the tab.
    let behind = if selected {
        toward_background(ui, color, 0.20)
    } else {
        ui.visuals().panel_fill
    };
    paint_icon(ui.painter(), icon_box, icon, color, behind);

    // Painted rather than laid out as a widget, so the caption cannot wrap the
    // way the old text-only bar did — it is one line, centred, always.
    ui.painter().text(
        egui::pos2(rect.center().x, rect.bottom() - 5.0),
        egui::Align2::CENTER_BOTTOM,
        label,
        egui::FontId::proportional(CAPTION),
        color,
    );
    response
}

// -------------------------------------------------------------------------
// text to speech (spec 1.4)
// -------------------------------------------------------------------------

/// Is there a voice on this platform?
///
/// Spec 1.4 wants on-device TTS everywhere. The browser has one built in, so
/// the web build speaks; desktop and Android would each need a platform
/// binding (speech-dispatcher, and JNI to `android.speech.tts`), which this
/// build does not carry. The IPA and the example sentences are shown either
/// way, and nothing else depends on audio.
pub fn can_speak() -> bool {
    cfg!(target_arch = "wasm32")
}

/// Speaks `text` at `rate` (spec 1.3 offers 0,75× and 1,0×).
#[cfg(target_arch = "wasm32")]
pub fn speak(text: &str, rate: f32) {
    use eframe::web_sys;

    let Some(synth) = web_sys::window().and_then(|w| w.speech_synthesis().ok()) else {
        return;
    };
    synth.cancel();
    if let Ok(utterance) = web_sys::SpeechSynthesisUtterance::new_with_text(text) {
        utterance.set_lang("en-US");
        utterance.set_rate(rate);
        synth.speak(&utterance);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn speak(_text: &str, _rate: f32) {}

// -------------------------------------------------------------------------
// study reminders (spec 3.6)
// -------------------------------------------------------------------------

/// Can this build raise a notification outside the app?
///
/// Only the browser can, and only while the page is open. A real scheduled
/// push would need a platform service — a notification channel and an alarm on
/// Android, a launch agent on desktop — which this build does not carry. The
/// in-app reminder is shown on every platform regardless.
pub fn can_notify() -> bool {
    cfg!(target_arch = "wasm32")
}

/// Has the user already allowed notifications?
#[cfg(target_arch = "wasm32")]
pub fn notifications_allowed() -> bool {
    web_sys::Notification::permission() == web_sys::NotificationPermission::Granted
}

#[cfg(not(target_arch = "wasm32"))]
pub fn notifications_allowed() -> bool {
    false
}

/// Asks the browser for permission. The prompt is asynchronous; the answer
/// simply shows up in [`notifications_allowed`] on a later frame.
#[cfg(target_arch = "wasm32")]
pub fn request_notifications() {
    let _ = web_sys::Notification::request_permission();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn request_notifications() {}

/// Raises a notification, if this platform has one and the user allowed it.
#[cfg(target_arch = "wasm32")]
pub fn notify(title: &str, body: &str) {
    if !notifications_allowed() {
        return;
    }
    let options = web_sys::NotificationOptions::new();
    options.set_body(body);
    let _ = web_sys::Notification::new_with_options(title, &options);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn notify(_title: &str, _body: &str) {}

/// The two speed buttons of spec 1.3, drawn only where they would work.
pub fn speak_buttons(ui: &mut egui::Ui, text: &str) {
    if !can_speak() {
        return;
    }
    // Painted rather than 🔊/🐢: those live only in the fallback emoji fonts,
    // so they arrive in a different typeface — or, for the turtle, not at all.
    if icon_button(ui, Icon::Speaker, "Play").clicked() {
        speak(text, 1.0);
    }
    if icon_button(ui, Icon::SpeakerSlow, "Play slowly (0.75×)").clicked() {
        speak(text, 0.75);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_grouped() {
        assert_eq!(thousands(1), "1");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(25_000), "25,000");
        assert_eq!(thousands(119_296), "119,296");
    }

    #[test]
    fn icons_paint_at_any_size() {
        // The bar draws these at 24 px, but a degenerate rect must not panic
        // and the shapes are all expressed as fractions of the side.
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            for side in [0.0f32, 1.0, 8.0, 24.0, 96.0] {
                let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(side, side));
                for icon in [
                    Icon::Home,
                    Icon::Search,
                    Icon::Study,
                    Icon::Map,
                    Icon::Person,
                    Icon::Speaker,
                    Icon::SpeakerSlow,
                ] {
                    paint_icon(ui.painter(), rect, icon, Color32::WHITE, Color32::BLACK);
                }
            }
        })
        .drop_without_applying_deltas();
    }

    #[test]
    fn every_state_has_its_own_reading_in_both_themes() {
        // Spec 2.3's legend has to stay distinguishable: confirmed, inferred
        // and in-progress are three different colours — on either theme.
        for dark in [true, false] {
            let ctx = egui::Context::default();
            ctx.set_theme(if dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            ctx.run_ui(Default::default(), |ui| {
                let c = |s| state_color(ui, s);
                assert_eq!(c(State::Known), c(State::Mastered));
                assert_eq!(c(State::Learning), c(State::Review));
                assert_ne!(c(State::Known), c(State::AssumedKnown));
                assert_ne!(c(State::AssumedKnown), c(State::Learning));
                assert_ne!(c(State::Learning), c(State::Unexplored), "dark={dark}");
                // The empty part of the bar must not be mistaken for progress.
                assert_ne!(c(State::Unexplored), ui.visuals().panel_fill, "dark={dark}");
            })
            .drop_without_applying_deltas();
        }
    }
}
