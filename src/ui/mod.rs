//! The screens, and what they are built from.
//!
//! - [`theme`]: colours by role, the type scale, egui's style
//! - [`icons`]: the painted icon set
//! - [`widgets`]: cards, buttons, chips, rows, headers and sheets
//!
//! Each screen module draws one tab.

pub mod home;
pub mod icons;
pub mod lookup;
pub mod map;
pub mod profile;
pub mod study;
pub mod theme;
pub mod widgets;

pub use icons::{Icon, paint_icon};
pub use theme::{Palette, palette};
pub use widgets::*;

use crate::progress::Accent;

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

/// "today", "tomorrow", "in 4 days" — when a card comes round again.
pub fn when(days: i64) -> String {
    match days {
        i64::MIN..=0 => "later today".to_owned(),
        1 => "tomorrow".to_owned(),
        n => format!("in {n} days"),
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
pub fn speak(text: &str, rate: f32, accent: Accent) {
    use eframe::web_sys;

    let Some(synth) = web_sys::window().and_then(|w| w.speech_synthesis().ok()) else {
        return;
    };
    synth.cancel();
    if let Ok(utterance) = web_sys::SpeechSynthesisUtterance::new_with_text(text) {
        utterance.set_lang(accent.tag());
        utterance.set_rate(rate);
        synth.speak(&utterance);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn speak(_text: &str, _rate: f32, _accent: Accent) {}

/// The two speed buttons of spec 1.3, drawn only where they would work.
///
/// The slow one says "0.75×" in so many words. It used to be the same
/// speaker with one sound wave instead of two, explained only by a tooltip
/// — and a phone has no hover, so it had no tooltip either.
pub fn speak_buttons(ui: &mut eframe::egui::Ui, text: &str, accent: Accent) {
    if !can_speak() {
        return;
    }
    if icon_button(ui, Icon::Speaker, "Play", IconStyle::Soft).clicked() {
        speak(text, 1.0, accent);
    }
    if pill_button(ui, "0.75\u{d7}", "Play slowly").clicked() {
        speak(text, 0.75, accent);
    }
}

/// One plain speaker, for an example sentence.
pub fn speak_button(ui: &mut eframe::egui::Ui, text: &str, accent: Accent) {
    if can_speak() && icon_button(ui, Icon::Speaker, "Play the example", IconStyle::Plain).clicked()
    {
        speak(text, 1.0, accent);
    }
}

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

#[cfg(test)]
mod tests {
    use super::theme::{DARK, LIGHT};
    use super::*;
    use crate::progress::State;
    use eframe::egui::{self, Color32};

    /// Every piece of text a frame painted, with where its middle is.
    fn painted_text(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    out.push((text.galley.text().to_owned(), rect));
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// One frame of `body` on a screen `width` wide.
    fn frame(
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
        body: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.0),
            )),
            events,
            ..Default::default()
        };
        let mut body = body;
        let mut output = ctx.run_ui(input, |ui| body(ui));
        // Nothing renders here, so the font atlas has nowhere to go.
        output.textures_delta.clear();
        output
    }

    fn tap(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    #[test]
    fn a_switch_answers_only_for_itself() {
        // The settings screen's shape: switch rows in sibling groups, which
        // is exactly where controls once shared ids.
        let ctx = egui::Context::default();
        crate::WordTeeApp::configure_style(&ctx);
        let mut on = [true; 4];
        let draw = |ui: &mut egui::Ui, on: &mut [bool; 4]| {
            settings_group(ui, "Learning", |ui| {
                switch_row(ui, Icon::Ball, "Streak alerts", &mut on[0]);
                switch_row(ui, Icon::Lines, "Word examples", &mut on[1]);
            });
            settings_group(ui, "Review cards", |ui| {
                switch_row(ui, Icon::Speaker, "Pronounce on show", &mut on[2]);
                switch_row(ui, Icon::Warning, "Hard word alert", &mut on[3]);
            });
        };
        frame(&ctx, 400.0, vec![], |ui| draw(ui, &mut on));
        let output = frame(&ctx, 400.0, vec![], |ui| draw(ui, &mut on));
        let text = painted_text(&output);
        assert!(
            !text.iter().any(|(t, _)| t.contains("use of")),
            "egui reported a widget id clash"
        );

        // Tap the third row's switch, which sits against the right edge.
        let row = text
            .iter()
            .find(|(t, _)| t == "Pronounce on show")
            .expect("the row is drawn")
            .1;
        let at = egui::pos2(400.0 - 40.0, row.center().y);
        frame(
            &ctx,
            400.0,
            vec![egui::Event::PointerMoved(at), tap(at, true)],
            |ui| draw(ui, &mut on),
        );
        frame(&ctx, 400.0, vec![tap(at, false)], |ui| draw(ui, &mut on));
        assert_eq!(on, [true, true, false, true]);
    }

    #[test]
    fn a_wide_pill_goes_under_its_label_not_over_it() {
        let rates = ["Off", "Every 4h", "Every 8h", "Once a day"];
        let ctx = egui::Context::default();
        crate::WordTeeApp::configure_style(&ctx);
        let place = |width: f32| {
            frame(&ctx, width, vec![], |ui| {
                settings_group(ui, "Learning", |ui| {
                    choice_row(ui, Icon::Bell, "Reminders", &rates, 3);
                });
            });
            let output = frame(&ctx, width, vec![], |ui| {
                settings_group(ui, "Learning", |ui| {
                    choice_row(ui, Icon::Bell, "Reminders", &rates, 3);
                });
            });
            let text = painted_text(&output);
            let find = |what: &str| text.iter().find(|(t, _)| t == what).unwrap().1;
            (find("Reminders"), find("Off"), find("Once a day"))
        };

        // Room enough: one line.
        let (label, first, _) = place(800.0);
        assert!((label.center().y - first.center().y).abs() < 4.0);
        assert!(first.left() > label.right());

        // A phone: the pill wraps below, clear of the label.
        let (label, first, last) = place(320.0);
        assert!(first.top() > label.bottom(), "{label:?} vs {first:?}");
        assert!(last.right() <= 320.0, "the pill runs off the screen");
    }

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
        // A degenerate rect must not panic, and the shapes are all expressed
        // as fractions of the side.
        let ctx = egui::Context::default();
        ctx.run_ui(Default::default(), |ui| {
            for side in [0.0f32, 1.0, 8.0, 24.0, 96.0] {
                let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(side, side));
                for icon in [
                    Icon::Home,
                    Icon::Study,
                    Icon::Map,
                    Icon::Search,
                    Icon::Person,
                    Icon::Speaker,
                    Icon::Check,
                    Icon::Cross,
                    Icon::Plus,
                    Icon::ChevronLeft,
                    Icon::ChevronRight,
                    Icon::ChevronDown,
                    Icon::ChevronUp,
                    Icon::Ball,
                    Icon::Bell,
                    Icon::Target,
                    Icon::Sun,
                    Icon::Moon,
                    Icon::Letters,
                    Icon::Lines,
                    Icon::Warning,
                    Icon::Trend,
                    Icon::Arrow,
                    Icon::Info,
                    Icon::Sync,
                    Icon::Scan,
                ] {
                    paint_icon(ui.painter(), rect, icon, Color32::WHITE, Color32::BLACK);
                }
            }
        })
        .drop_without_applying_deltas();
    }

    /// WCAG relative luminance.
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
    fn contrast(a: Color32, b: Color32) -> f32 {
        let (hi, lo) = (
            luminance(a).max(luminance(b)),
            luminance(a).min(luminance(b)),
        );
        (hi + 0.05) / (lo + 0.05)
    }

    const FLOOR: f32 = 4.5;

    fn legible(what: &str, ink: Color32, bg: Color32) {
        let ratio = contrast(ink, bg);
        assert!(ratio >= FLOOR, "{what} is {ratio:.2}:1, under {FLOOR}");
    }

    #[test]
    fn every_text_colour_is_legible_where_it_is_used() {
        // The fills are bright on purpose — the teal reads at 2.2:1 as text
        // on white — so every text colour is its own, darker entry. This is
        // the guard against reaching for the fill because it matches.
        for (theme, p) in [("light", LIGHT), ("dark", DARK)] {
            for (name, ink) in [
                ("ink", p.ink),
                ("ink2", p.ink2),
                ("ink3", p.ink3),
                ("primary_ink", p.primary_ink),
                ("known_ink", p.known_ink),
                ("wrong_ink", p.wrong_ink),
                ("warn", p.warn),
            ] {
                for (surface, bg) in [("page", p.page), ("surface", p.surface)] {
                    legible(&format!("{theme}: {name} on {surface}"), ink, bg);
                }
            }
            // The washes, with the text that goes on them.
            for (name, ink, bg) in [
                ("ink2 on sunken", p.ink2, p.sunken),
                ("ink on sunken", p.ink, p.sunken),
                ("primary_ink on primary_soft", p.primary_ink, p.primary_soft),
                ("known_ink on known_soft", p.known_ink, p.known_soft),
                ("wrong_ink on wrong_soft", p.wrong_ink, p.wrong_soft),
                ("ink on known_soft", p.ink, p.known_soft),
                ("ink on wrong_soft", p.ink, p.wrong_soft),
                ("streak_ink on streak_soft", p.streak_ink, p.streak_soft),
                ("on_primary on primary", p.on_primary, p.primary),
                ("on_known on known", p.on_known, p.known),
                ("on_wrong on wrong", p.on_wrong, p.wrong),
            ] {
                legible(&format!("{theme}: {name}"), ink, bg);
            }
        }
    }

    #[test]
    fn every_state_chip_can_be_read() {
        // The bug this exists for: chips were written in the state's *fill*
        // colour, so "New" sat at 1.2:1 and "Known" at 2:1.
        for (theme, p) in [("light", LIGHT), ("dark", DARK)] {
            for state in [
                State::Unexplored,
                State::AssumedKnown,
                State::Known,
                State::Learning,
                State::Review,
                State::Mastered,
            ] {
                let (fill, ink, _, _) = state_chip_colors(&p, state);
                legible(&format!("{theme}: {state:?} chip"), ink, fill);
            }
        }
    }

    #[test]
    fn the_style_paints_from_the_palette() {
        for (dark, p) in [(false, LIGHT), (true, DARK)] {
            let ctx = egui::Context::default();
            crate::WordTeeApp::configure_style(&ctx);
            ctx.set_theme(if dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            ctx.run_ui(Default::default(), |ui| {
                assert_eq!(ui.visuals().panel_fill, p.page);
                assert_eq!(ui.visuals().text_color(), p.ink);
                assert_eq!(palette(ui).page, p.page);
            })
            .drop_without_applying_deltas();
        }
    }

    #[test]
    fn the_map_fills_are_visible_against_what_they_sit_on() {
        // These are fills, not text, so they answer to a lower bar — but the
        // empty part of a bar still has to look like part of a bar.
        for (theme, p) in [("light", LIGHT), ("dark", DARK)] {
            for (name, color) in [
                ("known", p.known),
                ("assumed", p.assumed),
                ("learning", p.learning),
                ("unexplored", p.unexplored),
            ] {
                for (surface, bg) in [("page", p.page), ("surface", p.surface)] {
                    let ratio = contrast(color, bg);
                    assert!(
                        ratio >= 1.2,
                        "{theme}: {name} is {ratio:.2}:1 on the {surface}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_state_has_its_own_reading_in_both_themes() {
        // Spec 2.3's legend has to stay distinguishable: confirmed, inferred
        // and in-progress are three different colours — on either theme.
        for dark in [true, false] {
            let ctx = egui::Context::default();
            crate::WordTeeApp::configure_style(&ctx);
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

    #[test]
    fn a_wrong_answer_marks_the_mistake_and_the_right_option() {
        let (answer, chosen) = (2, 0);
        assert_eq!(mark_for(None, 0, answer), Mark::Plain);
        assert_eq!(mark_for(Some(chosen), chosen, answer), Mark::Wrong);
        assert_eq!(mark_for(Some(chosen), answer, answer), Mark::Right);
        // The untouched options step back.
        for i in [1, 3] {
            assert_eq!(mark_for(Some(chosen), i, answer), Mark::Faded, "option {i}");
        }
        // A right pick marks only itself as right.
        assert_eq!(mark_for(Some(answer), answer, answer), Mark::Right);
        assert_eq!(mark_for(Some(answer), 0, answer), Mark::Faded);
    }

    #[test]
    fn a_review_date_reads_as_words() {
        assert_eq!(when(0), "later today");
        assert_eq!(when(1), "tomorrow");
        assert_eq!(when(4), "in 4 days");
    }
}
