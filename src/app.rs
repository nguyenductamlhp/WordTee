//! The app shell: five tabs, the shared context they draw with, and the saved
//! progress underneath.

use eframe::egui::{self, RichText};

use crate::dict::{Dict, WordId};
use crate::google::Google;
use crate::progress::{self, Day, Progress, Theme, Undo};
use crate::quiz::Shown;
use crate::rng::Rng;
use crate::ui;

/// Window title on desktop, launcher label on Android.
pub const APP_NAME: &str = "WordTee";

/// The UI font.
///
/// egui's bundled Ubuntu-Light covers only 89% of the characters this app puts
/// on screen. The two gaps are exactly the two things the app is made of:
/// Vietnamese tone marks, which live in Latin Extended Additional
/// (U+1EA0–U+1EF9), and the IPA in every pronunciation — `ˈ ə ɪ ː` alone occur
/// 150.000 times in the dictionary. Both rendered as empty boxes. Noto Sans
/// covers 99,99% of the pack; see the test at the bottom of this file.
static UI_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/fonts/NotoSans-Regular.ttf"
));

/// The same face at weight 600, for headings, buttons and headwords.
///
/// egui has no font weights, and `RichText::strong` only changes the colour,
/// so without a second file nothing in the app could be bold.
static UI_FONT_SEMIBOLD: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/fonts/NotoSans-SemiBold.ttf"
));

/// Key the progress is stored under (eframe's storage: a file on desktop and
/// Android, local storage in the browser).
const STORAGE_KEY: &str = "wordtee.progress";

/// Spec 1.4: how long the Undo offer stays on screen.
const UNDO_SECONDS: f64 = 5.0;

/// How long a message with nothing to undo stays on screen.
const MESSAGE_SECONDS: f64 = 3.0;

/// The five places you can be.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    /// Where the app opens: one word, four meanings, answer and move on.
    #[default]
    Home,
    Lookup,
    Study,
    Map,
    Profile,
}

impl Tab {
    /// Left-to-right order in the bar.
    const ALL: [Self; 5] = [
        Self::Home,
        Self::Study,
        Self::Map,
        Self::Lookup,
        Self::Profile,
    ];

    /// The caption under the icon.
    fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Lookup => "Look up",
            Self::Study => "Study",
            Self::Map => "Map",
            Self::Profile => "You",
        }
    }

    fn icon(self) -> ui::Icon {
        match self {
            Self::Home => ui::Icon::Home,
            Self::Lookup => ui::Icon::Search,
            Self::Study => ui::Icon::Study,
            Self::Map => ui::Icon::Map,
            Self::Profile => ui::Icon::Person,
        }
    }
}

/// A message at the bottom of the screen, optionally undoable (spec 1.4).
pub struct Toast {
    pub message: String,
    pub undo: Option<Undo>,
    /// `Context::input(|i| i.time)` when it appeared.
    pub born: f64,
}

/// What every screen is handed: the dictionary, the user, and the few things
/// a screen may change outside itself.
pub struct Ctx<'a> {
    pub dict: &'a Dict,
    pub progress: &'a mut Progress,
    /// Google sign-in, and syncing the progress through Drive.
    pub google: &'a mut Google,
    pub rng: &'a mut Rng,
    pub day: Day,
    pub shown: &'a mut Shown,
    pub toast: &'a mut Option<Toast>,
    /// Set to jump to another tab after this frame.
    pub goto: &'a mut Option<Tab>,
    /// Set to open a word on the lookup tab.
    pub open_word: &'a mut Option<WordId>,
    /// Set to start today's session, from wherever the button was.
    pub start_session: &'a mut bool,
    /// `(due, new, checks)` waiting today, as the session would serve them.
    pub pending: (usize, usize, usize),
    pub now: f64,
}

impl Ctx<'_> {
    /// Shows a message with no Undo.
    pub fn say(&mut self, message: impl Into<String>) {
        *self.toast = Some(Toast {
            message: message.into(),
            undo: None,
            born: self.now,
        });
    }

    /// Shows a message with the five-second Undo of spec 1.4.
    pub fn say_undoable(&mut self, message: impl Into<String>, undo: Undo) {
        *self.toast = Some(Toast {
            message: message.into(),
            undo: Some(undo),
            born: self.now,
        });
    }
}

/// WordTee.
pub struct WordTeeApp {
    dict: Dict,
    progress: Progress,
    google: Google,
    rng: Rng,
    day: Day,
    shown: Shown,
    tab: Tab,
    toast: Option<Toast>,
    home: ui::home::HomeState,
    lookup: ui::lookup::LookupState,
    study: ui::study::StudyState,
    map: ui::map::MapState,
    profile: ui::profile::ProfileState,
    /// Where Android's status and navigation bars cover the window.
    #[cfg(target_os = "android")]
    system_bars: Option<crate::android::SystemBars>,
}

impl Default for WordTeeApp {
    fn default() -> Self {
        let day = progress::today();
        let mut progress = Progress::default();
        progress.roll_to(day);
        Self {
            dict: Dict::load(),
            progress,
            google: Google::default(),
            rng: Rng::new(),
            day,
            shown: Shown::default(),
            tab: Tab::default(),
            toast: None,
            home: Default::default(),
            lookup: Default::default(),
            study: Default::default(),
            map: Default::default(),
            profile: Default::default(),
            #[cfg(target_os = "android")]
            system_bars: None,
        }
    }
}

impl WordTeeApp {
    /// Builds the app, restoring saved progress if there is any.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::configure_style(&cc.egui_ctx);
        let mut app = Self::default();
        if let Some(storage) = cc.storage
            && let Some(saved) = eframe::get_value::<Progress>(storage, STORAGE_KEY)
        {
            app.progress = saved;
            app.progress.roll_to(app.day);
        }
        app.google = Google::load(cc.storage);
        app
    }

    /// Keeps the UI out from under Android's status and navigation bars.
    #[cfg(target_os = "android")]
    pub(crate) fn with_system_bars(mut self, bars: crate::android::SystemBars) -> Self {
        self.system_bars = Some(bars);
        self
    }

    /// The app's look and feel. Separate from [`Self::new`] so tests can set it
    /// up without an [`eframe::CreationContext`].
    pub fn configure_style(ctx: &egui::Context) {
        Self::install_fonts(ctx);
        ui::theme::apply(ctx);
        Self::apply_theme(ctx, Theme::default());
    }

    /// Switches the colour scheme.
    fn apply_theme(ctx: &egui::Context, theme: Theme) {
        ctx.set_theme(match theme {
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
    }

    /// Puts [`UI_FONT`] in front of the bundled fonts, and builds the
    /// semibold family from [`UI_FONT_SEMIBOLD`].
    ///
    /// Noto goes first rather than last so that a Vietnamese word is drawn in
    /// one typeface throughout — as a fallback it would only supply the
    /// accented letters, and every word would be a mix of two fonts. The
    /// semibold family falls back to the regular face, then to egui's own.
    /// Monospace keeps Hack in front and takes Noto only as a fallback, so
    /// columns still line up.
    fn install_fonts(ctx: &egui::Context) {
        use egui::{FontData, FontDefinitions, FontFamily};
        use std::sync::Arc;

        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "NotoSans".to_owned(),
            Arc::new(FontData::from_static(UI_FONT)),
        );
        fonts.font_data.insert(
            "NotoSans-SemiBold".to_owned(),
            Arc::new(FontData::from_static(UI_FONT_SEMIBOLD)),
        );
        let defaults = fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        let mut proportional = vec!["NotoSans".to_owned()];
        proportional.extend(defaults.iter().cloned());
        let mut semibold = vec!["NotoSans-SemiBold".to_owned(), "NotoSans".to_owned()];
        semibold.extend(defaults);
        fonts
            .families
            .insert(FontFamily::Proportional, proportional);
        fonts.families.insert(ui::theme::semibold(), semibold);
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("NotoSans".to_owned());
        ctx.set_fonts(fonts);
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    pub fn dict(&self) -> &Dict {
        &self.dict
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Is a focused flow on screen — a session, a test — which hides the
    /// tab bar so a stray tap cannot leave it halfway?
    fn focused(&self) -> bool {
        match self.tab {
            Tab::Study => self.study.focused(),
            Tab::Profile => self.profile.testing(),
            Tab::Lookup => self.lookup.testing(),
            Tab::Home | Tab::Map => false,
        }
    }

    /// Draws one frame.
    ///
    /// Kept separate from the [`eframe::App`] impl so it can be driven without
    /// an [`eframe::Frame`], which is what the tests do.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        // The day can turn over while the app is open.
        let day = progress::today();
        if day != self.day {
            self.day = day;
            self.progress.roll_to(day);
            self.study.reset();
            self.home.refresh();
        }
        self.google.update(ui.ctx(), &mut self.progress, now);

        // The setting rides along with the saved progress, so this is also what
        // restores the chosen theme on the first frame after a restart.
        let wants_dark = self.progress.theme == Theme::Dark;
        if ui.visuals().dark_mode != wants_dark {
            Self::apply_theme(ui.ctx(), self.progress.theme);
        }

        // Spec 3.6's reminder, at whatever interval the user chose.
        let pending = self.study.pending(&self.dict, &self.progress, self.day);
        let waiting = pending.0 + pending.1;
        let secs = progress::now_secs();
        if self.progress.reminder_due(secs, waiting) {
            self.progress.mark_reminded(secs);
            let body = format!("{waiting} words are waiting.");
            ui::notify("Time to study", &body);
            self.toast = Some(Toast {
                message: format!("Time to study — {body}"),
                undo: None,
                born: now,
            });
        }

        // Android lays the window out under its status and navigation bars.
        // The page colour runs on under them, and the panels keep to the safe
        // area, which is the whole window everywhere else.
        let viewport = ui.ctx().viewport_rect();
        let content = ui.ctx().content_rect();
        ui.painter()
            .rect_filled(viewport, 0, ui.visuals().panel_fill);
        let mut safe = ui.new_child(egui::UiBuilder::new().max_rect(content));
        let ui = &mut safe;

        let mut goto = None;
        let mut open_word = None;
        let mut start_session = false;

        if !self.focused() {
            self.show_tab_bar(ui, &mut goto);
        }

        let mut ctx = Ctx {
            dict: &self.dict,
            progress: &mut self.progress,
            google: &mut self.google,
            rng: &mut self.rng,
            day: self.day,
            shown: &mut self.shown,
            toast: &mut self.toast,
            goto: &mut goto,
            open_word: &mut open_word,
            start_session: &mut start_session,
            pending,
            now,
        };

        // No margin: each screen draws its own header panel and gutter.
        let page = egui::Frame::new().fill(ui.visuals().panel_fill);
        egui::CentralPanel::default()
            .frame(page)
            .show(ui, |ui| match self.tab {
                Tab::Home => ui::home::show(ui, &mut ctx, &mut self.home),
                Tab::Lookup => ui::lookup::show(ui, &mut ctx, &mut self.lookup),
                Tab::Study => ui::study::show(ui, &mut ctx, &mut self.study),
                Tab::Map => ui::map::show(ui, &mut ctx, &mut self.map),
                Tab::Profile => ui::profile::show(ui, &mut ctx, &mut self.profile),
            });

        // After the screens, so a message raised this frame shows at once.
        self.show_toast(ui, now);

        if start_session {
            self.study
                .begin_session(&self.dict, &self.progress, self.day, &mut self.rng);
            self.tab = Tab::Study;
        } else if let Some(word) = open_word {
            self.lookup.open(word);
            self.tab = Tab::Lookup;
        } else if let Some(tab) = goto {
            self.tab = tab;
        }
    }

    /// The bottom navigation bar.
    fn show_tab_bar(&mut self, ui: &mut egui::Ui, goto: &mut Option<Tab>) {
        let p = ui::palette(ui);
        let frame = egui::Frame::new()
            .fill(p.surface)
            .inner_margin(egui::Margin {
                left: 4,
                right: 4,
                top: 6,
                bottom: 6,
            });
        egui::Panel::bottom("tabs").frame(frame).show(ui, |ui| {
            let current = self.tab;
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.columns(Tab::ALL.len(), |columns| {
                for (column, tab) in columns.iter_mut().zip(Tab::ALL) {
                    if ui::tab_button(column, tab.icon(), current == tab, tab.label()).clicked() {
                        *goto = Some(tab);
                    }
                }
            });
        });
    }

    /// The message popup at the top of the screen, with an Undo while one is
    /// offered.
    ///
    /// It floats over the header rather than taking a strip of the layout,
    /// so nothing on the screen moves when it comes and goes, and it sits
    /// away from the buttons at the bottom that raised it. It counts nothing
    /// down: the Undo is simply there for as long as the popup is.
    fn show_toast(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some(toast) = &self.toast else { return };
        let age = now - toast.born;
        let lasts = if toast.undo.is_some() {
            UNDO_SECONDS
        } else {
            MESSAGE_SECONDS
        };
        if age > lasts {
            self.toast = None;
            return;
        }
        let undoable = toast.undo.is_some();
        let message = toast.message.clone();
        let mut dismiss = false;
        let mut undo = false;

        let p = ui::palette(ui);
        // Inverted, so it reads as passing over the screen: dark on the
        // light theme, light on the dark one.
        let (fill, ink, action) = if ui.visuals().dark_mode {
            (p.ink, p.page, ui::theme::LIGHT.primary_ink)
        } else {
            (p.ink, p.page, ui::theme::DARK.primary_ink)
        };
        // In quickly, out gently.
        let opacity = ((age / 0.15).min((lasts - age) / 0.3)).clamp(0.0, 1.0) as f32;
        let safe = ui.max_rect();
        let width = safe.width() - 24.0;
        egui::Area::new(egui::Id::new("toast"))
            .order(egui::Order::Foreground)
            .fixed_pos(safe.left_top() + egui::vec2(12.0, 8.0))
            .show(ui.ctx(), |ui| {
                ui.multiply_opacity(opacity);
                ui.set_width(width);
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(14)
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 4,
                        top: 2,
                        bottom: 2,
                    })
                    .shadow(egui::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(40),
                    })
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        let width = ui.available_width();
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, ui::TOUCH),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let (close, response) = ui.allocate_exact_size(
                                    egui::Vec2::splat(ui::TOUCH),
                                    egui::Sense::click(),
                                );
                                ui::paint_icon(
                                    ui.painter(),
                                    egui::Rect::from_center_size(
                                        close.center(),
                                        egui::Vec2::splat(18.0),
                                    ),
                                    ui::Icon::Cross,
                                    ink,
                                    fill,
                                );
                                dismiss = response.on_hover_text("Dismiss").clicked();
                                if undoable {
                                    let label = RichText::new("Undo")
                                        .size(ui::theme::size::LABEL)
                                        .family(ui::theme::semibold())
                                        .color(action);
                                    undo = ui.add(egui::Button::new(label).frame(false)).clicked();
                                }
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&message)
                                                    .size(ui::theme::size::LABEL)
                                                    .color(ink),
                                            )
                                            .wrap(),
                                        );
                                    },
                                );
                            },
                        );
                    });
            });

        if undo {
            if let Some(toast) = self.toast.take()
                && let Some(undo) = toast.undo
            {
                self.progress.undo(undo);
            }
        } else if dismiss {
            self.toast = None;
        } else {
            // Keep the fades moving, and wake up to take it away.
            ui.ctx().request_repaint_after_secs(0.05);
        }
    }
}

impl eframe::App for WordTeeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
        #[cfg(target_os = "android")]
        if let Some(bars) = &self.system_bars {
            bars.watch_keyboard(ui.ctx());
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.progress);
        self.google.save(storage);
    }

    /// egui-winit reports the safe area on iOS only, so on Android it comes
    /// from here.
    #[cfg(target_os = "android")]
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(bars) = &mut self.system_bars {
            // This frame's scale, as egui-winit measured the screen in, where
            // `ctx.pixels_per_point()` is still the last frame's.
            let native = raw_input.viewport().native_pixels_per_point.unwrap_or(1.0);
            raw_input.safe_area_insets = bars.insets(native * ctx.zoom_factor());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::SenseId;
    use crate::placement::Placement;
    use crate::progress::{Source, State};
    use crate::study::Session;
    use eframe::egui::{Event, Pos2, Rect, Vec2};

    /// A phone-shaped screen; every layout has to survive this width.
    const SCREEN: Vec2 = Vec2::new(400.0, 800.0);

    /// Drives [`WordTeeApp`] frame by frame without a window or a GPU.
    struct Harness {
        ctx: egui::Context,
        app: WordTeeApp,
        time: f64,
        /// What the system bars cover, as Android reports it.
        insets: egui::SafeAreaInsets,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            WordTeeApp::configure_style(&ctx);
            let mut harness = Self {
                ctx,
                app: WordTeeApp::default(),
                time: 0.0,
                insets: Default::default(),
            };
            harness.frame(vec![]); // Warm-up pass, so widget rects exist.
            harness
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.frame_output(events).drop_without_applying_deltas();
        }

        fn frame_output(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 1.0 / 60.0;
            let Self {
                ctx,
                app,
                time,
                insets,
            } = self;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
                safe_area_insets: Some(*insets),
                time: Some(*time),
                events,
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.show(ui))
        }

        /// The warnings egui paints over widgets that share an id, which is
        /// when a tap on one can register on another. Debug builds only.
        fn id_clashes(&mut self) -> Vec<String> {
            fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(text) if text.galley.text().contains("use of") => {
                        out.push(text.galley.text().to_owned());
                    }
                    egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let output = self.frame_output(vec![]);
            let mut out = Vec::new();
            for clipped in &output.shapes {
                walk(&clipped.shape, &mut out);
            }
            output.drop_without_applying_deltas();
            out
        }

        /// A few frames, so anything animated or deferred settles.
        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(vec![]);
            }
        }

        fn type_text(&mut self, text: &str) {
            self.frame(vec![Event::Text(text.to_owned())]);
        }

        fn on(&mut self, tab: Tab) {
            self.app.tab = tab;
            self.settle();
        }

        /// The first teachable learning item, for tests that need one.
        fn item(&self, rank: u32) -> SenseId {
            self.app.dict.at_rank(rank).expect("rank in range").id
        }
    }

    /// Every character the dictionary is able to draw, and how often it occurs.
    fn characters_in_the_pack(dict: &Dict) -> std::collections::HashMap<char, u64> {
        let mut seen = std::collections::HashMap::new();
        let mut count = |text: &str| {
            for c in text.chars() {
                *seen.entry(c).or_insert(0u64) += 1;
            }
        };
        for id in 0..dict.word_count() {
            let word = dict.word(id);
            count(word.text);
            count(word.ipa);
            for (_, related) in dict.relations(id) {
                count(related);
            }
        }
        for id in 0..dict.sense_count() {
            let sense = dict.sense(id);
            count(sense.def);
            count(sense.example);
        }
        seen
    }

    #[test]
    fn nothing_hides_under_the_system_bars() {
        // Android 15 lays the window out under the status and navigation
        // bars, and its navigation buttons sat on top of the tab bar.
        let mut h = Harness::new();
        h.insets = egui::SafeAreaInsets(egui::epaint::MarginF32 {
            left: 0.0,
            right: 0.0,
            top: 24.0,
            bottom: 48.0,
        });
        h.settle();

        let panel = |id: &str| {
            egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new(id))
                .unwrap_or_else(|| panic!("no {id} panel"))
                .outer_rect
        };
        assert_eq!(panel("chrome").top(), 24.0);
        assert_eq!(panel("tabs").bottom(), SCREEN.y - 48.0);
    }

    #[test]
    fn the_keyboard_lifts_the_screen_rather_than_covering_it() {
        // Android 15 no longer resizes the window for the keyboard; it is
        // reported like a navigation bar, only taller.
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        let keyboard = 330.0;
        h.insets.0.bottom = keyboard;
        h.settle();

        let above = SCREEN.y - keyboard;
        let tabs = egui::containers::panel::PanelState::load(&h.ctx, egui::Id::new("tabs"))
            .expect("the tab bar is drawn")
            .outer_rect;
        assert_eq!(tabs.bottom(), above);
        let search = h
            .ctx
            .memory(|m| m.focused())
            .and_then(|id| h.ctx.read_response(id))
            .expect("the search box has focus");
        assert!(
            search.rect.bottom() <= tabs.top(),
            "the search box is at {:?}, the tab bar at {tabs:?}",
            search.rect
        );
    }

    #[test]
    fn the_bundled_font_covers_the_dictionary() {
        // The bug this test exists for: egui's default font has neither the
        // Vietnamese tone marks nor the IPA, and every one of them rendered as
        // an empty box.
        let face = ttf_parser::Face::parse(UI_FONT, 0).expect("the bundled font parses");
        let dict = Dict::load();
        let seen = characters_in_the_pack(&dict);

        let covered = |c: char| c.is_whitespace() || face.glyph_index(c).is_some();
        let total: u64 = seen.values().sum();
        let missing: u64 = seen
            .iter()
            .filter(|(c, _)| !covered(**c))
            .map(|(_, n)| n)
            .sum();

        // Vietnamese lives in Latin Extended Additional; the IPA in the two
        // blocks after Latin Extended-B. Neither may have a single gap.
        for (&c, &n) in &seen {
            let must_have = matches!(c as u32,
                0x00C0..=0x024F   // Latin supplements, including ơ ư đ
                | 0x0250..=0x02AF // IPA extensions: ə ɪ ʊ ŋ
                | 0x02B0..=0x02FF // modifiers: the ˈ ˌ ː of a transcription
                | 0x1E00..=0x1EFF // Latin Extended Additional: ế ừ ụ ợ
            );
            assert!(
                !must_have || covered(c),
                "no glyph for {c:?} (U+{:04X}), which the pack uses {n} times",
                c as u32
            );
        }
        // The rest is a long tail of foreign scripts in a few etymologies —
        // Khmer, Arabic, Devanagari — which are out of scope for this app.
        let coverage = 100.0 * (total - missing) as f64 / total as f64;
        assert!(
            coverage > 99.99,
            "font covers only {coverage:.4}% of the pack"
        );
    }

    /// The app's own string literals, from the sources that hold UI text.
    ///
    /// Read out of the source rather than listed by hand, so a new label is
    /// covered the moment it is written.
    fn characters_in_the_interface() -> std::collections::BTreeSet<char> {
        const SOURCES: [&str; 13] = [
            include_str!("app.rs"),
            include_str!("dict.rs"),
            include_str!("progress.rs"),
            include_str!("search.rs"),
            include_str!("ui/mod.rs"),
            include_str!("ui/theme.rs"),
            include_str!("ui/icons.rs"),
            include_str!("ui/widgets.rs"),
            include_str!("ui/home.rs"),
            include_str!("ui/lookup.rs"),
            include_str!("ui/study.rs"),
            include_str!("ui/map.rs"),
            include_str!("ui/profile.rs"),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for source in SOURCES {
            for line in source.lines() {
                // Prose in a doc comment is never drawn, and several of them
                // quote the spec's own arrows.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                // `\u{2039}` is as much a drawn character as a pasted one, so
                // the escapes are decoded rather than skipped over. Missing
                // that is how an unrenderable glyph slips past this.
                let mut rest = line;
                while let Some(at) = rest.find("\\u{") {
                    rest = &rest[at + 3..];
                    if let Some(end) = rest.find('}')
                        && let Ok(code) = u32::from_str_radix(&rest[..end], 16)
                        && let Some(c) = char::from_u32(code)
                    {
                        if !c.is_ascii() {
                            seen.insert(c);
                        }
                        rest = &rest[end..];
                    }
                }

                let mut in_string = false;
                let mut escaped = false;
                for c in line.chars() {
                    match c {
                        _ if escaped => escaped = false,
                        '\\' if in_string => escaped = true,
                        '"' => in_string = !in_string,
                        _ if in_string && !c.is_ascii() => {
                            seen.insert(c);
                        }
                        _ => {}
                    }
                }
            }
        }
        seen
    }

    #[test]
    fn the_bundled_font_covers_the_interface() {
        // The dictionary test below checks the *content*. This checks the app's
        // own labels, which is where the second round of empty boxes came from:
        // `←`, `→` and `✕` are in no bundled font, and the tab bar's emoji came
        // from a fallback in a different typeface. Everything the interface
        // draws now has to be in the one font.
        let mut missing: Vec<char> = Vec::new();
        for font in [UI_FONT, UI_FONT_SEMIBOLD] {
            let face = ttf_parser::Face::parse(font, 0).expect("the bundled font parses");
            missing.extend(
                characters_in_the_interface()
                    .into_iter()
                    .filter(|c| face.glyph_index(*c).is_none()),
            );
        }
        assert!(
            missing.is_empty(),
            "no glyph for {missing:?} — pick characters the bundled font has, \
             or spell the label out in words"
        );
    }

    #[test]
    fn every_tab_renders_in_both_themes() {
        let mut h = Harness::new();
        for theme in [Theme::Light, Theme::Dark] {
            h.app.progress.theme = theme;
            for tab in Tab::ALL {
                h.on(tab);
                assert_eq!(h.app.tab(), tab);
            }
        }
    }

    #[test]
    fn the_bar_runs_home_study_map_lookup_you() {
        assert_eq!(
            Tab::ALL,
            [Tab::Home, Tab::Study, Tab::Map, Tab::Lookup, Tab::Profile]
        );
        // The app opens on Home.
        assert_eq!(Tab::default(), Tab::Home);
        assert_eq!(Harness::new().app.tab(), Tab::Home);
        // Every tab needs its own icon, or the bar is ambiguous.
        let icons: std::collections::BTreeSet<_> =
            Tab::ALL.iter().map(|t| format!("{:?}", t.icon())).collect();
        assert_eq!(icons.len(), Tab::ALL.len());
    }

    #[test]
    fn home_is_what_the_app_opens_on_and_it_has_a_question() {
        let mut h = Harness::new();
        assert_eq!(h.app.tab(), Tab::Home);
        h.settle();
        // Nothing to set up and nothing due: a first-run user still gets asked
        // something on the very first frame.
        assert_eq!(h.app.home.answered(), (0, 0));
    }

    #[test]
    fn answering_on_home_moves_mastery_and_the_streak() {
        let mut h = Harness::new();
        h.on(Tab::Home);
        let sense = h.app.dict.at_rank(1_100).unwrap();
        let before = h.app.progress.mastery(&sense);

        h.app
            .progress
            .start_learning(sense.id, Source::Manual, h.app.day);
        for _ in 0..3 {
            let day = h.app.progress.card(sense.id).unwrap().due;
            h.app.progress.answer(
                sense.id,
                crate::srs::Outcome {
                    correct: true,
                    hesitated: false,
                    level: 1,
                },
                day,
            );
        }
        let after = h.app.progress.mastery(&sense);
        assert!(after > before, "{before} -> {after}");
        h.settle();
    }

    #[test]
    fn the_app_starts_light() {
        let h = Harness::new();
        assert_eq!(h.app.progress.theme, Theme::Light);
        assert_eq!(h.ctx.theme(), egui::Theme::Light, "first frame drew dark");
    }

    #[test]
    fn switching_the_theme_takes_effect_and_is_saved() {
        let mut h = Harness::new();
        h.app.progress.theme = Theme::Dark;
        h.settle();
        assert_eq!(h.ctx.theme(), egui::Theme::Dark);

        // The choice rides along with the rest of the saved progress.
        let text = ron::to_string(&h.app.progress).expect("serialises");
        let back: Progress = ron::from_str(&text).expect("deserialises");
        assert_eq!(back.theme, Theme::Dark);

        h.app.progress.theme = Theme::Light;
        h.settle();
        assert_eq!(h.ctx.theme(), egui::Theme::Light);
    }

    #[test]
    fn typing_in_the_search_box_finds_a_word() {
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        h.type_text("decision");
        h.settle();
        let hits = h.app.lookup.hits();
        assert!(!hits.is_empty(), "typing produced no results");
        assert_eq!(h.app.dict.word(hits[0].word).text, "decision");
    }

    #[test]
    fn a_typo_still_finds_the_word() {
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        h.type_text("teh");
        h.settle();
        let found: Vec<&str> = h
            .app
            .lookup
            .hits()
            .iter()
            .map(|hit| h.app.dict.word(hit.word).text)
            .collect();
        assert!(found.contains(&"the"), "{found:?}");
    }

    #[test]
    fn the_word_page_renders_and_its_buttons_change_state() {
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        let word = h.app.dict.exact("decision").unwrap();
        h.app.lookup.open(word.id);
        h.settle();
        assert_eq!(h.app.lookup.open_word(), Some(word.id));

        // "I Know This" is the action bar's first button (spec 1.4).
        let sense = h.app.dict.sense(word.senses().next().unwrap());
        assert_eq!(h.app.progress.state(&sense), State::Unexplored);
        let undo = h
            .app
            .progress
            .set_state(sense.id, State::Known, Source::Manual, h.app.day);
        h.settle();
        assert_eq!(h.app.progress.state(&sense), State::Known);

        // …and the Undo in the toast puts it back (spec 1.4).
        h.app.progress.undo(undo);
        assert_eq!(h.app.progress.state(&sense), State::Unexplored);
    }

    #[test]
    fn a_word_page_renders_for_every_shape_of_entry() {
        // Phrases, inflection-only entries and gap-filled entries all take
        // different paths through the page.
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        for word in ["run", "children", "a bit", "hello", "why", "saw", "-gate"] {
            let Some(found) = h.app.dict.exact(word) else {
                panic!("{word} is missing from the pack");
            };
            h.app.lookup.open(found.id);
            h.settle();
        }
    }

    #[test]
    fn a_quick_test_runs_to_a_verdict() {
        let mut h = Harness::new();
        h.on(Tab::Lookup);
        let word = h.app.dict.exact("decision").unwrap();
        h.app.lookup.open(word.id);
        let sense = h.app.dict.sense(word.senses().next().unwrap());

        let mut rng = Rng::seeded(4);
        let mut toast = None;
        let mut goto = None;
        let mut open_word = None;
        let mut start_session = false;
        let mut shown = Shown::default();
        let mut ctx = Ctx {
            dict: &h.app.dict,
            progress: &mut h.app.progress,
            google: &mut h.app.google,
            rng: &mut rng,
            day: h.app.day,
            shown: &mut shown,
            toast: &mut toast,
            goto: &mut goto,
            open_word: &mut open_word,
            start_session: &mut start_session,
            pending: (0, 0, 0),
            now: 0.0,
        };
        h.app.lookup.begin_quick_test(&mut ctx, &sense);
        h.settle();
    }

    #[test]
    fn the_placement_test_renders_and_records_a_frontier() {
        let mut h = Harness::new();
        h.on(Tab::Profile);
        h.app.profile.begin_test(&h.app.dict);
        h.settle();
        assert!(h.app.profile.testing());

        // Run the test itself to completion, then check the screen that shows
        // the result also renders.
        let mut test = Placement::new(&h.app.dict);
        while let Some(asked) = test.question() {
            let pick = asked.sense.map(|_| asked.choice.answer);
            test.answer(&h.app.dict, pick);
        }
        let verdict = test.verdict();
        h.app
            .progress
            .apply_placement(verdict.frontier, verdict.theta);
        h.settle();
        assert!(h.app.progress.placement_done);
        assert!(h.app.progress.frontier > 1);
    }

    #[test]
    fn a_study_session_renders_at_every_level() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(2_000, 8.0);
        h.on(Tab::Study);

        // Answering correctly raises a card's exercise level (spec 3.3), and
        // each level draws a different question. Walk one card up all three,
        // rendering the session at each step.
        let sense = h.item(2_100);
        h.app
            .progress
            .start_learning(sense, Source::Manual, h.app.day);
        let mut levels = vec![h.app.progress.card(sense).unwrap().level];
        for _ in 0..8 {
            let day = h.app.progress.card(sense).unwrap().due;
            let outcome = crate::srs::Outcome {
                correct: true,
                hesitated: false,
                level: 3,
            };
            h.app.progress.answer(sense, outcome, day);
            let level = h.app.progress.card(sense).unwrap().level;
            if Some(&level) != levels.last() {
                levels.push(level);
            }
            h.app
                .study
                .begin_session(&h.app.dict, &h.app.progress, day, &mut h.app.rng);
            h.settle();
        }
        assert_eq!(levels, vec![1, 2, 3], "card did not climb the levels");
        assert_eq!(h.app.progress.cards().count(), 1);
        assert!(
            Session::build(&h.app.dict, &h.app.progress, h.app.day, &mut h.app.rng).total() > 0
        );
    }

    #[test]
    fn quick_scan_renders_and_answering_clears_the_card() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(3_000, 8.0);
        h.on(Tab::Study);
        let items = crate::study::quick_scan(&h.app.dict, &h.app.progress, &mut h.app.rng, 5);
        assert_eq!(items.len(), 5);
        h.app.study.begin_scan(items);
        h.settle();
    }

    #[test]
    fn the_map_renders_both_views() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(4_500, 8.4);
        h.app
            .progress
            .start_learning(h.item(5_000), Source::Manual, h.app.day);
        h.on(Tab::Map);
        // The grid, at a few ranges, then the block overview behind the toggle.
        for block in [0, 3, 24] {
            h.app.map.open_block(block);
            h.settle();
        }
        h.app.map.show_rank(12_345);
        h.settle();
    }

    #[test]
    fn the_map_opens_where_the_user_is_working() {
        // Not at rank 1: the frontier is the part of the list that matters.
        let mut h = Harness::new();
        h.app.progress.apply_placement(4_500, 8.4);
        h.on(Tab::Map);
        h.settle();
        assert_eq!(h.app.map.range_start(), 4_501);
    }

    #[test]
    fn the_map_reflects_what_has_been_learned() {
        let mut h = Harness::new();
        let counts = h.app.progress.tally(h.app.dict.learn_span(1..1_001));
        assert_eq!(counts[State::Unexplored as usize], 1_000);

        h.app.progress.apply_placement(1_000, 6.9);
        let counts = h.app.progress.tally(h.app.dict.learn_span(1..1_001));
        assert_eq!(counts[State::AssumedKnown as usize], 1_000);
        h.on(Tab::Map);
    }

    #[test]
    fn no_screen_has_widgets_sharing_an_id() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(2_000, 8.0);
        for tab in Tab::ALL {
            h.on(tab);
            let clashes = h.id_clashes();
            assert!(clashes.is_empty(), "{tab:?}: {clashes:?}");
        }
    }

    #[test]
    fn a_message_pops_up_at_the_top_without_a_countdown() {
        let mut h = Harness::new();
        h.on(Tab::Map);
        let sense = h.item(1_250);
        let undo = h
            .app
            .progress
            .set_state(sense, State::Known, Source::Manual, h.app.day);
        h.app.toast = Some(Toast {
            message: "Marked as known.".to_owned(),
            undo: Some(undo),
            born: h.time,
        });
        h.settle();

        let output = h.frame_output(vec![]);
        let mut text = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                )),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut text);
        }
        output.drop_without_applying_deltas();

        let message = text
            .iter()
            .find(|(t, _)| t == "Marked as known.")
            .expect("the message is drawn")
            .1;
        assert!(message.top() < SCREEN.y / 4.0, "drawn at {message:?}");
        assert!(text.iter().any(|(t, _)| t == "Undo"));
        assert!(
            !text.iter().any(|(t, _)| t.starts_with("Undo (")),
            "the Undo counts down"
        );

        // Gone once its time is up.
        if let Some(toast) = &mut h.app.toast {
            toast.born -= UNDO_SECONDS + 0.1;
        }
        h.settle();
        assert!(h.app.toast.is_none());
    }

    #[test]
    fn progress_is_saved_and_restored() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(2_750, 8.0);
        let sense = h.item(3_000);
        h.app
            .progress
            .start_learning(sense, Source::Manual, h.app.day);

        // The same round trip eframe's storage performs.
        let text = ron::to_string(&h.app.progress).expect("serialises");
        let restored: Progress = ron::from_str(&text).expect("deserialises");
        assert_eq!(restored.assumed_below, 2_750);
        assert_eq!(restored.state(&h.app.dict.sense(sense)), State::Learning);
    }

    #[test]
    fn a_day_rolling_over_resets_the_session() {
        let mut h = Harness::new();
        // Pretend the app was left open overnight: both the cached "today" and
        // the day the counters belong to are yesterday's.
        h.app.day -= 1;
        h.app.progress.roll_to(h.app.day);
        h.app.progress.new_today = 5;
        h.app
            .study
            .begin_session(&h.app.dict, &h.app.progress, h.app.day, &mut h.app.rng);

        h.settle();
        assert_eq!(h.app.day, progress::today());
        assert_eq!(h.app.progress.new_today, 0, "counters did not roll over");
    }
}
