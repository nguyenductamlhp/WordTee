//! The app shell: four tabs, the shared context they draw with, and the saved
//! progress underneath.

use eframe::egui::{self, RichText};

use crate::dict::{Dict, WordId};
use crate::progress::{self, Day, Progress, Undo};
use crate::quiz::Shown;
use crate::rng::Rng;
use crate::ui;

/// Window title on desktop, launcher label on Android.
pub const APP_NAME: &str = "WordTee";

/// Key the progress is stored under (eframe's storage: a file on desktop and
/// Android, local storage in the browser).
const STORAGE_KEY: &str = "wordtee.progress";

/// Spec 1.4: how long the Undo offer stays on screen.
const UNDO_SECONDS: f64 = 5.0;

/// The four places you can be.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Tab {
    #[default]
    Lookup,
    Study,
    Map,
    Profile,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Lookup, Self::Study, Self::Map, Self::Profile];

    fn label(self) -> &'static str {
        match self {
            Self::Lookup => "🔍 Tra từ",
            Self::Study => "🎓 Học",
            Self::Map => "🗺 Bản đồ",
            Self::Profile => "👤 Tôi",
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
    pub rng: &'a mut Rng,
    pub day: Day,
    pub shown: &'a mut Shown,
    pub toast: &'a mut Option<Toast>,
    /// Set to jump to another tab after this frame.
    pub goto: &'a mut Option<Tab>,
    /// Set to open a word on the lookup tab.
    pub open_word: &'a mut Option<WordId>,
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
    rng: Rng,
    day: Day,
    shown: Shown,
    tab: Tab,
    toast: Option<Toast>,
    lookup: ui::lookup::LookupState,
    study: ui::study::StudyState,
    map: ui::map::MapState,
    profile: ui::profile::ProfileState,
}

impl Default for WordTeeApp {
    fn default() -> Self {
        let day = progress::today();
        let mut progress = Progress::default();
        progress.roll_to(day);
        Self {
            dict: Dict::load(),
            progress,
            rng: Rng::new(),
            day,
            shown: Shown::default(),
            tab: Tab::default(),
            toast: None,
            lookup: Default::default(),
            study: Default::default(),
            map: Default::default(),
            profile: Default::default(),
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
        app
    }

    /// The app's look and feel. Separate from [`Self::new`] so tests can set it
    /// up without an [`eframe::CreationContext`].
    pub fn configure_style(ctx: &egui::Context) {
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.all_styles_mut(|style| {
            // The default sizes are tuned for a mouse pointer and read small
            // under a fingertip.
            for font in style.text_styles.values_mut() {
                font.size *= 1.15;
            }
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.spacing.item_spacing = egui::vec2(8.0, 6.0);
            style.visuals.selection.bg_fill = ui::ACCENT.gamma_multiply(0.35);
        });
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
        }

        let mut goto = None;
        let mut open_word = None;

        egui::Panel::top("chrome").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(APP_NAME).strong().color(ui::ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(2.0);
                    if self.progress.streak > 0 {
                        ui.label(RichText::new(format!("🔥 {}", self.progress.streak)).size(13.0));
                    }
                    let (due, new, _) = self.study.pending(&self.dict, &self.progress, self.day);
                    if due + new > 0 {
                        ui::chip(ui, &format!("{} thẻ", due + new), ui::WARN);
                    }
                });
            });
            ui.add_space(2.0);
        });

        self.show_tab_bar(ui, &mut goto);
        self.show_toast(ui, now);

        let mut ctx = Ctx {
            dict: &self.dict,
            progress: &mut self.progress,
            rng: &mut self.rng,
            day: self.day,
            shown: &mut self.shown,
            toast: &mut self.toast,
            goto: &mut goto,
            open_word: &mut open_word,
            now,
        };

        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Lookup => ui::lookup::show(ui, &mut ctx, &mut self.lookup),
            Tab::Study => ui::study::show(ui, &mut ctx, &mut self.study),
            Tab::Map => ui::map::show(ui, &mut ctx, &mut self.map),
            Tab::Profile => ui::profile::show(ui, &mut ctx, &mut self.profile),
        });

        if let Some(word) = open_word {
            self.lookup.open(word);
            self.tab = Tab::Lookup;
        } else if let Some(tab) = goto {
            self.tab = tab;
        }
    }

    /// The bottom navigation bar.
    fn show_tab_bar(&mut self, ui: &mut egui::Ui, goto: &mut Option<Tab>) {
        egui::Panel::bottom("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            ui.columns(Tab::ALL.len(), |columns| {
                for (column, tab) in columns.iter_mut().zip(Tab::ALL) {
                    column.vertical_centered_justified(|ui| {
                        let selected = self.tab == tab;
                        let text = RichText::new(tab.label()).size(13.0).color(if selected {
                            ui::ACCENT
                        } else {
                            ui::MUTED
                        });
                        if ui.selectable_label(selected, text).clicked() {
                            *goto = Some(tab);
                        }
                    });
                }
            });
            ui.add_space(4.0);
        });
    }

    /// The message strip, with the Undo button while it is still offered.
    fn show_toast(&mut self, ui: &mut egui::Ui, now: f64) {
        let Some(toast) = &self.toast else { return };
        let age = now - toast.born;
        if age > UNDO_SECONDS * 2.0 {
            self.toast = None;
            return;
        }
        let undoable = toast.undo.is_some() && age <= UNDO_SECONDS;
        let message = toast.message.clone();
        let mut dismiss = false;
        let mut undo = false;

        egui::Panel::bottom("toast").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&message).size(13.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("✕").clicked() {
                        dismiss = true;
                    }
                    if undoable {
                        let left = (UNDO_SECONDS - age).ceil() as u32;
                        if ui.button(format!("Hoàn tác ({left}s)")).clicked() {
                            undo = true;
                        }
                    }
                });
            });
            ui.add_space(4.0);
        });

        if undo {
            if let Some(toast) = self.toast.take()
                && let Some(undo) = toast.undo
            {
                self.progress.undo(undo);
            }
        } else if dismiss {
            self.toast = None;
        } else if undoable {
            // Keep the countdown ticking.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
}

impl eframe::App for WordTeeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.progress);
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

    #[test]
    fn every_tab_renders() {
        let mut h = Harness::new();
        for tab in Tab::ALL {
            h.on(tab);
            assert_eq!(h.app.tab(), tab);
        }
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
        let mut shown = Shown::default();
        let mut ctx = Ctx {
            dict: &h.app.dict,
            progress: &mut h.app.progress,
            rng: &mut rng,
            day: h.app.day,
            shown: &mut shown,
            toast: &mut toast,
            goto: &mut goto,
            open_word: &mut open_word,
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
            h.app.study.begin_session(&h.app.dict, &h.app.progress, day);
            h.settle();
        }
        assert_eq!(levels, vec![1, 2, 3], "card did not climb the levels");
        assert_eq!(h.app.progress.cards().count(), 1);
        assert!(Session::build(&h.app.dict, &h.app.progress, h.app.day).total() > 0);
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
    fn the_map_renders_and_opens_a_block() {
        let mut h = Harness::new();
        h.app.progress.apply_placement(4_500, 8.4);
        h.app
            .progress
            .start_learning(h.item(5_000), Source::Manual, h.app.day);
        h.on(Tab::Map);
        // The overview, then a block drill-down.
        h.app.map.open_block(3);
        h.settle();
        h.app.map.open_block(24);
        h.settle();
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
            .begin_session(&h.app.dict, &h.app.progress, h.app.day);

        h.settle();
        assert_eq!(h.app.day, progress::today());
        assert_eq!(h.app.progress.new_today, 0, "counters did not roll over");
    }
}
