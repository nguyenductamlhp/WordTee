//! The screens, and the bits of chrome they share.

pub mod lookup;
pub mod map;
pub mod profile;
pub mod study;

use eframe::egui::{self, Color32, RichText};

use crate::dict::{Band, Pos};
use crate::progress::State;

/// The app's accent, inherited from the original tap-counter build.
pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0xb6, 0xf5);
pub const GOOD: Color32 = Color32::from_rgb(0x66, 0xbb, 0x6a);
pub const WARN: Color32 = Color32::from_rgb(0xf5, 0xb3, 0x41);
pub const BAD: Color32 = Color32::from_rgb(0xef, 0x53, 0x50);
pub const MUTED: Color32 = Color32::from_gray(0x8a);

/// Spec 2.3's four map colours.
pub const C_KNOWN: Color32 = Color32::from_rgb(0x2e, 0x7d, 0x32);
pub const C_ASSUMED: Color32 = Color32::from_rgb(0x7c, 0xb3, 0x42);
pub const C_LEARNING: Color32 = Color32::from_rgb(0xf5, 0xb3, 0x41);
pub const C_UNEXPLORED: Color32 = Color32::from_gray(0x3c);

/// The colour a state gets on the map and on its chip.
pub fn state_color(state: State) -> Color32 {
    match state {
        State::Known | State::Mastered => C_KNOWN,
        State::AssumedKnown => C_ASSUMED,
        State::Learning | State::Review => C_LEARNING,
        State::Unexplored => C_UNEXPLORED,
    }
}

/// A small filled pill, the app's one repeated label shape.
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    if text.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(color.gamma_multiply(0.22))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(9)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}

pub fn state_chip(ui: &mut egui::Ui, state: State) {
    chip(ui, state.label(), state_color(state));
}

pub fn pos_chip(ui: &mut egui::Ui, pos: Pos) {
    chip(ui, pos.short(), MUTED);
}

/// Spec 1.3's commonness indicator: four named tiers rather than one bar,
/// because an evenly divided 1–25.000 bar makes rank 500 and rank 2.000 look
/// like neighbours.
pub fn band_chip(ui: &mut egui::Ui, band: Band, rank: u32) {
    let color = match band {
        Band::Core => GOOD,
        Band::Advanced => ACCENT,
        Band::Academic => WARN,
        Band::Rare => MUTED,
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
            .color(MUTED)
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
        frame = frame.stroke(egui::Stroke::new(1.5, color.gamma_multiply(0.7)));
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
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, C_UNEXPLORED);
    if total == 0 {
        return response;
    }

    // Confirmed first, then inferred, then in progress — left to right.
    let order = [
        (State::Mastered, C_KNOWN),
        (State::Known, C_KNOWN),
        (State::AssumedKnown, C_ASSUMED),
        (State::Review, C_LEARNING),
        (State::Learning, C_LEARNING),
    ];
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

/// The two speed buttons of spec 1.3, drawn only where they would work.
pub fn speak_buttons(ui: &mut egui::Ui, text: &str) {
    if !can_speak() {
        return;
    }
    if ui.button("🔊").on_hover_text("Play (1.0×)").clicked() {
        speak(text, 1.0);
    }
    if ui
        .button("🐢")
        .on_hover_text("Play slowly (0.75×)")
        .clicked()
    {
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
    fn every_state_has_its_own_reading() {
        // Spec 2.3's legend must stay distinguishable: confirmed, inferred and
        // in-progress are three different colours.
        assert_eq!(state_color(State::Known), state_color(State::Mastered));
        assert_eq!(state_color(State::Learning), state_color(State::Review));
        assert_ne!(state_color(State::Known), state_color(State::AssumedKnown));
        assert_ne!(
            state_color(State::AssumedKnown),
            state_color(State::Learning)
        );
        assert_ne!(state_color(State::Learning), state_color(State::Unexplored));
    }
}
