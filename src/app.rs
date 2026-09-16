//! The tap counter UI. Shared by every platform; the entry points live in
//! `lib.rs`, `main.rs`, `android.rs` and `web.rs`.

use eframe::egui;

/// Window title on desktop, launcher label on Android.
pub const APP_NAME: &str = "WordTee";

/// Accent colour used for the counter and the tap ripples.
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x4d, 0xb6, 0xf5);

/// How long a tap ripple stays on screen, in seconds.
const RIPPLE_LIFETIME: f32 = 0.55;

/// How long the counter stays "popped" after a tap, in seconds.
const POP_LIFETIME: f32 = 0.22;

/// An expanding circle drawn where a tap landed, so every tap is visible as
/// well as counted.
struct Ripple {
    pos: egui::Pos2,
    /// Value of `Context::input(|i| i.time)` when the tap happened.
    born: f64,
}

/// Counts taps, and nothing else.
#[derive(Default)]
pub struct WordTeeApp {
    count: u64,
    ripples: Vec<Ripple>,
    /// Time of the most recent tap, used for the counter "pop" animation.
    last_tap: Option<f64>,
}

impl WordTeeApp {
    /// Builds the app and applies the shared look-and-feel.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::configure_style(&cc.egui_ctx);
        Self::default()
    }

    /// The app's look-and-feel. Separate from [`Self::new`] so tests (and any
    /// other embedder) can set it up without an [`eframe::CreationContext`].
    pub fn configure_style(ctx: &egui::Context) {
        ctx.set_theme(egui::ThemePreference::Dark);

        // Nudge every text style up a little: the default sizes are tuned for a
        // mouse pointer and read small under a fingertip.
        ctx.all_styles_mut(|style| {
            for font in style.text_styles.values_mut() {
                font.size *= 1.35;
            }
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
        });
    }

    /// Number of taps counted so far.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// Draws one frame into `ui`.
    ///
    /// Kept separate from the [`eframe::App`] impl so it can be driven without
    /// an [`eframe::Frame`], which is what the tests below do.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                ui.strong(APP_NAME);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(self.count > 0, egui::Button::new("Reset"))
                        .clicked()
                    {
                        self.reset();
                    }
                    ui.label(format!("{} taps", self.count));
                });
            });
        });

        // A zero-margin frame, so the tap area really does reach the edges of
        // the screen instead of stopping at the default panel padding.
        let frame = egui::Frame::central_panel(ui.style()).inner_margin(0);

        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let rect = ui.max_rect();

            // One click-sensitive area covering the whole panel: that is the
            // "screen" the user taps. Claiming it also stops egui from handing
            // the press to anything underneath.
            let area = ui.interact(rect, ui.id().with("tap-area"), egui::Sense::click());
            let hovered = area.contains_pointer();

            // React on press rather than release so the ripple appears under the
            // finger immediately, and so a tap counts even if the finger slides.
            let tap_pos = ui.input(|i| {
                if hovered && i.pointer.any_pressed() {
                    i.pointer.interact_pos()
                } else {
                    None
                }
            });
            if let Some(pos) = tap_pos {
                self.tap(Some(pos), now);
            }

            // Space/Enter are a convenience for desktop and hardware keyboards.
            if ui.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter)) {
                self.tap(Some(rect.center()), now);
            }

            let painter = ui.painter().clone();
            self.paint_counter(&painter, rect, now);
            let rippling = self.paint_ripples(&painter, now);

            let popping = self
                .last_tap
                .is_some_and(|t| (now - t) as f32 <= POP_LIFETIME);
            if rippling || popping {
                ui.ctx().request_repaint();
            }
        });
    }

    /// Registers a tap, optionally at a screen position (keyboard taps have none).
    fn tap(&mut self, at: Option<egui::Pos2>, now: f64) {
        self.count += 1;
        self.last_tap = Some(now);
        if let Some(pos) = at {
            self.ripples.push(Ripple { pos, born: now });
        }
    }

    fn reset(&mut self) {
        self.count = 0;
        self.ripples.clear();
        self.last_tap = None;
    }

    /// Draws the tap ripples and drops the ones that have faded out.
    ///
    /// Returns `true` while a ripple is still visible, so the caller knows it
    /// has to ask for another frame.
    fn paint_ripples(&mut self, painter: &egui::Painter, now: f64) -> bool {
        self.ripples
            .retain(|r| (now - r.born) as f32 <= RIPPLE_LIFETIME);

        for ripple in &self.ripples {
            // `t` runs 0 -> 1 over the ripple's lifetime.
            let t = ((now - ripple.born) as f32 / RIPPLE_LIFETIME).clamp(0.0, 1.0);
            let radius = 12.0 + 110.0 * ease_out(t);
            let alpha = (1.0 - t).powi(2);
            painter.circle_stroke(
                ripple.pos,
                radius,
                egui::Stroke::new(3.0, ACCENT.gamma_multiply(alpha)),
            );
        }

        !self.ripples.is_empty()
    }

    /// Draws the big counter in the middle of `rect`.
    fn paint_counter(&self, painter: &egui::Painter, rect: egui::Rect, now: f64) {
        // A short scale-up right after a tap makes the increment feel physical.
        let pop = self
            .last_tap
            .map(|t| (1.0 - ((now - t) as f32 / POP_LIFETIME).clamp(0.0, 1.0)).powi(2))
            .unwrap_or(0.0);

        let size = (rect.height() * 0.30)
            .min(rect.width() * 0.42)
            .clamp(40.0, 240.0)
            * (1.0 + 0.14 * pop);

        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            self.count.to_string(),
            egui::FontId::proportional(size),
            ACCENT,
        );

        painter.text(
            rect.center() + egui::vec2(0.0, size * 0.62),
            egui::Align2::CENTER_CENTER,
            if self.count == 0 {
                "tap anywhere to start"
            } else {
                "tap anywhere"
            },
            egui::FontId::proportional((size * 0.13).clamp(12.0, 22.0)),
            egui::Color32::from_gray(0x8a),
        );
    }
}

impl eframe::App for WordTeeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

/// Cubic ease-out, so ripples start fast and settle slowly.
fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2, Rect, Vec2, pos2};

    const SCREEN: Vec2 = Vec2::new(400.0, 800.0);

    /// Drives [`WordTeeApp`] frame by frame without a window or a GPU.
    struct Harness {
        ctx: egui::Context,
        app: WordTeeApp,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            WordTeeApp::configure_style(&ctx);
            let mut harness = Self {
                ctx,
                app: WordTeeApp::default(),
                time: 0.0,
            };
            harness.frame(vec![]); // Warm-up pass, so widget rects exist.
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let Self { ctx, app, time } = self;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
                time: Some(*time),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.show(ui))
                .drop_without_applying_deltas();
        }

        /// A press followed by a release at `pos`, as a touchscreen would send it.
        fn tap(&mut self, pos: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::default(),
            };
            self.frame(vec![Event::PointerMoved(pos), button(true)]);
            self.frame(vec![button(false)]);
        }

        fn press_key(&mut self, key: egui::Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::default(),
            }]);
        }
    }

    #[test]
    fn tapping_the_screen_increases_the_counter() {
        let mut h = Harness::new();
        assert_eq!(h.app.count(), 0);

        for expected in 1..=5 {
            h.tap(pos2(200.0, 400.0));
            assert_eq!(h.app.count(), expected);
        }
    }

    #[test]
    fn taps_count_anywhere_in_the_central_panel() {
        let mut h = Harness::new();
        for pos in [
            pos2(4.0, 796.0),                     // bottom-left corner
            pos2(396.0, 796.0),                   // bottom-right corner
            pos2(SCREEN.x / 2.0, SCREEN.y / 2.0), // dead centre
            pos2(1.0, SCREEN.y / 2.0),            // left edge
        ] {
            h.tap(pos);
        }
        assert_eq!(h.app.count(), 4);
    }

    #[test]
    fn the_toolbar_is_not_part_of_the_tap_area() {
        let mut h = Harness::new();
        h.tap(pos2(200.0, 2.0)); // inside the top panel
        assert_eq!(h.app.count(), 0);
    }

    #[test]
    fn space_and_enter_also_count_as_taps() {
        let mut h = Harness::new();
        h.press_key(egui::Key::Space);
        h.press_key(egui::Key::Enter);
        assert_eq!(h.app.count(), 2);
    }

    #[test]
    fn ripples_expire_instead_of_piling_up() {
        let mut h = Harness::new();
        h.tap(pos2(200.0, 400.0));
        assert_eq!(h.app.ripples.len(), 1);

        h.time += f64::from(RIPPLE_LIFETIME) + 0.1;
        h.frame(vec![]);
        assert!(h.app.ripples.is_empty());
    }
}
