//! Study — the daily session (spec 3.6) and Quick Scan (spec 2.2).
//!
//! The session order is the spec's: overdue reviews hardest-first, then new
//! items from Smart Feeding, then at most two spot-checks on things marked
//! known. Grades are never asked for — spec 3.2 derives them from how the
//! exercise went, which is what [`Outcome`] carries.
//!
//! A session is a focused flow: the tab bar steps aside, and every answer is
//! shown — right or wrong, with the right answer — before Continue moves on.

use eframe::egui::{self, Align, Layout, RichText, Stroke, text::LayoutJob};

use crate::app::Ctx;
use crate::dict::{Sense, SenseId};
use crate::progress::{BACKLOG_FACTOR, QUICK_SCAN_DAILY, Source, State};
use crate::quiz::{Choice, Cloze};
use crate::srs::{Grade, Outcome};
use crate::study::{self, Exercise, Session, Task};
use crate::ui::{self, FieldState, Icon, Tone, theme};

/// Answering slower than this counts as hesitation, which spec 3.2 grades as
/// Hard rather than Good.
const SLOW_SECONDS: f64 = 12.0;

/// What happens to the session once the verdict has been read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum After {
    Advance,
    /// Spec 3.3: a missed item comes round again this session.
    Requeue,
}

/// The question on screen and how it is going.
struct Active {
    task: Task,
    exercise: Exercise,
    typed: String,
    picked: Option<usize>,
    /// The user asked for the first letter, or took their time.
    hesitated: bool,
    /// `Context::input(|i| i.time)` when the question appeared.
    started: f64,
    /// Set once graded: the card shows the verdict until Continue.
    verdict: Option<bool>,
    after: After,
    /// What the verdict sheet says about when the word comes back.
    note: String,
    /// The answer field has been given focus once.
    focused: bool,
}

/// `(due, new, checks)`, the three numbers the session card shows.
type Pending = (usize, usize, usize);

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Mode {
    #[default]
    Idle,
    Session,
    Scan,
}

#[derive(Default)]
pub struct StudyState {
    mode: Mode,
    session: Option<Session>,
    active: Option<Active>,
    scan: Vec<SenseId>,
    /// How many cards the scan started with, for its progress bar.
    scan_total: usize,
    /// The meaning is showing on the scan card.
    reveal: bool,
    /// Counters for the end-of-session summary.
    answered: u32,
    right: u32,
    /// The frontier check below runs once per session, not once per frame.
    frontier_checked: bool,
    /// Cached answer for [`Self::pending`], and the revision it was computed
    /// at. The app asks for this on every frame of every tab, and building
    /// a session is not free — Smart Feeding scores 500 candidates, each with
    /// a word-family lookup.
    pending: std::cell::Cell<Option<(u64, Pending)>>,
}

impl StudyState {
    /// Dropped when the day turns over.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Is a session or a scan running? The tab bar hides while one is.
    pub fn focused(&self) -> bool {
        self.mode != Mode::Idle
    }

    /// Starts today's session, as the Start buttons do.
    pub fn begin_session(
        &mut self,
        dict: &crate::dict::Dict,
        progress: &crate::progress::Progress,
        day: crate::progress::Day,
        rng: &mut crate::rng::Rng,
    ) {
        self.session = Some(Session::build(dict, progress, day, rng));
        self.mode = Mode::Session;
        self.active = None;
        self.answered = 0;
        self.right = 0;
        self.frontier_checked = false;
    }

    /// Starts a Quick Scan run over `items`.
    pub fn begin_scan(&mut self, items: Vec<SenseId>) {
        self.scan_total = items.len();
        self.scan = items;
        self.reveal = false;
        self.mode = Mode::Scan;
    }

    fn leave(&mut self) {
        self.mode = Mode::Idle;
        self.session = None;
        self.active = None;
        self.scan.clear();
    }

    /// `(due, new, checks)` waiting right now.
    pub fn pending(
        &self,
        dict: &crate::dict::Dict,
        progress: &crate::progress::Progress,
        day: crate::progress::Day,
    ) -> Pending {
        if let Some(session) = &self.session {
            return session.counts();
        }
        let rev = progress.revision();
        if let Some((cached_rev, counts)) = self.pending.get()
            && cached_rev == rev
        {
            return counts;
        }
        // Any rng will do: it only picks which new slots dip below the
        // frontier, and that never changes the counts.
        let counts = Session::build(dict, progress, day, &mut crate::rng::Rng::seeded(0)).counts();
        self.pending.set(Some((rev, counts)));
        counts
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    match state.mode {
        Mode::Idle => home(ui, ctx, state),
        Mode::Session => session_page(ui, ctx, state),
        Mode::Scan => scan_page(ui, ctx, state),
    }
}

// -------------------------------------------------------------------------
// the tab's own page
// -------------------------------------------------------------------------

fn home(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    let streak = ctx.progress.streak;
    ui::screen_header(ui, "Study", |ui| {
        if streak > 0 {
            ui::streak_pill(ui, streak);
        }
    });
    let (due, new, checks) = ctx.pending;
    let p = ui::palette(ui);

    ui::page(ui, "study", |ui| {
        if !ctx.progress.has_level() {
            ui::card_frame(ui)
                .fill(p.primary_soft)
                .stroke(Stroke::NONE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(theme::heading("Find your level"));
                    ui.add(
                        egui::Label::new(
                            theme::caption(
                                "A short test finds where your vocabulary ends, so new \
                                 words start at the right level. Or pick a level yourself.",
                            )
                            .color(p.ink2),
                        )
                        .wrap(),
                    );
                    ui.add_space(8.0);
                    if ui::primary_button(ui, "Test or choose a level").clicked() {
                        *ctx.goto = Some(crate::app::Tab::Profile);
                    }
                });
        }

        // --- today's session ---
        ui::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(theme::heading("Today\u{2019}s session"));
            ui.label(theme::caption("Reviews first, then new words").color(p.ink2));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let width = ((ui.available_width() - 16.0) / 3.0).floor();
                ui::stat_tile(ui, &due.to_string(), "Reviews", p.primary_ink, width);
                ui::stat_tile(ui, &new.to_string(), "New", p.ink, width);
                ui::stat_tile(ui, &checks.to_string(), "Checks", p.known_ink, width);
            });
            ui.add_space(12.0);
            let goal = ctx.progress.daily_goal.max(1);
            let learned = ctx.progress.new_today;
            ui.horizontal(|ui| {
                ui.label(
                    theme::caption("New words today")
                        .color(p.ink2)
                        .family(theme::semibold()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(theme::caption(format!("{learned} / {goal}")).color(p.ink2));
                });
            });
            ui::progress_track(ui, learned as f32 / goal as f32, 8.0, p.primary);

            // Spec 3.6: the debt notice when reviews have piled up.
            if ctx.progress.backlog_is_heavy(ctx.day) {
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        theme::caption(format!(
                            "Reviews have piled up past {BACKLOG_FACTOR}\u{d7} the daily goal, so \
                             new words are paused until the backlog is down."
                        ))
                        .color(p.warn),
                    )
                    .wrap(),
                );
            }
            ui.add_space(12.0);
            if due + new + checks > 0 {
                if ui::primary_button(ui, "Start session").clicked() {
                    state.begin_session(ctx.dict, ctx.progress, ctx.day, ctx.rng);
                }
            } else {
                ui.add(
                    egui::Label::new(
                        theme::caption(
                            "All done for today. Come back tomorrow, or run a quick scan.",
                        )
                        .color(p.ink2),
                    )
                    .wrap(),
                );
            }
        });

        streak_card(ui, ctx);

        // --- spec 2.2's Gap Filling ---
        let scan_left = QUICK_SCAN_DAILY.saturating_sub(ctx.progress.scanned_today);
        let placed = ctx.progress.frontier > 1;
        ui::card(ui, |ui| {
            ui.horizontal(|ui| {
                tile(ui, Icon::Scan, p.known_soft, p.known_ink);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(theme::heading("Quick scan"));
                    ui.add(
                        egui::Label::new(
                            theme::caption("Confirm words the app assumed you already know.")
                                .color(p.ink2),
                        )
                        .wrap(),
                    );
                });
            });
            ui.add_space(8.0);
            ui.add_enabled_ui(scan_left > 0 && placed, |ui| {
                let label = format!("Scan {scan_left} words");
                if ui::secondary_button(ui, &label).clicked() {
                    let items =
                        study::quick_scan(ctx.dict, ctx.progress, ctx.rng, scan_left as usize);
                    state.begin_scan(items);
                }
            });
            if !placed {
                ui.label(
                    theme::caption("Take the placement test or choose a level first.")
                        .color(p.ink2),
                );
            } else if scan_left == 0 {
                ui.label(theme::caption("That\u{2019}s today\u{2019}s scan done.").color(p.ink2));
            }
        });
    });
}

/// An icon on a 40-point rounded tile.
fn tile(ui: &mut egui::Ui, icon: Icon, fill: egui::Color32, ink: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(40.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 12.0, fill);
    ui::paint_icon(
        ui.painter(),
        egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(22.0)),
        icon,
        ink,
        fill,
    );
}

/// The streak, and the last seven days as dots.
///
/// The dots are not labelled with weekdays: the app's day is a UTC day, and
/// a Vietnamese morning is still the previous UTC day, so "M T W" would be
/// wrong for seven hours of every day. Only today is named.
fn streak_card(ui: &mut egui::Ui, ctx: &mut Ctx) {
    let p = ui::palette(ui);
    let streak = ctx.progress.streak;
    let last = ctx.progress.last_active;
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            tile(ui, Icon::Ball, p.streak_soft, p.streak);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(theme::heading(if streak == 0 {
                    "No streak yet".to_owned()
                } else {
                    format!("{streak}-day streak")
                }));
                ui.label(
                    theme::caption(if streak == 0 {
                        "Study today to start one".to_owned()
                    } else {
                        format!("Best {} days", ctx.progress.best_streak)
                    })
                    .color(p.ink2),
                );
            });
        });
        ui.add_space(10.0);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 40.0 + 18.0),
            egui::Sense::hover(),
        );
        // Inset by today's ring, so it does not reach the card's edge.
        let step = (rect.width() - 42.0) / 6.0;
        for i in 0..7 {
            let day = ctx.day - 6 + i;
            let studied = streak > 0 && day <= last && day > last - streak as i64;
            let center = egui::pos2(rect.left() + 21.0 + step * i as f32, rect.top() + 20.0);
            let painter = ui.painter();
            if studied {
                painter.circle_filled(center, 16.0, p.streak);
            } else {
                painter.circle_stroke(center, 15.0, Stroke::new(1.5, p.line));
            }
            if day == ctx.day {
                painter.circle_stroke(center, 19.0, Stroke::new(2.0, p.streak));
                painter.text(
                    egui::pos2(center.x, rect.bottom()),
                    egui::Align2::CENTER_BOTTOM,
                    "Today",
                    egui::FontId::new(12.0, theme::semibold()),
                    p.streak_ink,
                );
            }
        }
    });
}

// -------------------------------------------------------------------------
// the session
// -------------------------------------------------------------------------

/// The chip over a question: what kind of item, and what kind of question.
fn task_label(task: Task, exercise: &Exercise) -> String {
    let what = match exercise {
        Exercise::Study => return "New word".to_owned(),
        Exercise::Meaning(_) => "Pick the meaning",
        Exercise::PickWord(_) => "Pick the word",
        Exercise::Fill(_) => "Fill the gap",
        Exercise::Spell => "Spell it",
    };
    let kind = match task {
        Task::Review(_) => "Review",
        Task::New(_) => "New word",
        Task::Verify(_) => "Spot-check",
    };
    format!("{kind} · {what}")
}

fn session_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    let Some(session) = &mut state.session else {
        state.mode = Mode::Idle;
        return;
    };
    let Some(task) = session.current() else {
        return summary(ui, ctx, state);
    };
    let (done, total) = (session.done(), session.total());
    let fraction = done as f32 / total.max(1) as f32;
    if ui::focus_header(ui, fraction, &format!("{done} / {total}")) {
        state.leave();
        return;
    }

    // Build the question for this task the first time we see it.
    if state.active.as_ref().is_none_or(|a| a.task != task) {
        let sense = ctx.dict.sense(task.sense());
        let earned = match task {
            // Spec 3.3: a brand-new item is read, not tested.
            Task::New(_) => 0,
            // Spec 3.5: checks are level-2 exercises.
            Task::Verify(_) => 2,
            Task::Review(id) => ctx.progress.card(id).map_or(1, |c| c.level),
        };
        // Whatever the card has earned, only ask what the user allows.
        let level = if earned == 0 {
            0
        } else {
            ctx.progress.challenges.level_for(earned)
        };
        state.active = Some(Active {
            task,
            exercise: study::exercise(ctx.dict, ctx.rng, &sense, level, ctx.shown),
            typed: String::new(),
            picked: None,
            hesitated: false,
            started: ctx.now,
            verdict: None,
            after: After::Advance,
            note: String::new(),
            focused: false,
        });
    }
    let sense = ctx.dict.sense(task.sense());

    // --- the bottom of the screen: the decision, Check, or the verdict ---
    let mut checked: Option<bool> = None;
    let mut move_on = false;
    if let Some(active) = &mut state.active {
        match (&active.exercise, active.verdict) {
            (Exercise::Study, _) => {
                let (knew, learn) = ui::action_bar(ui, "actions", ui::decision_buttons);
                if knew {
                    ctx.progress
                        .set_state(sense.id, State::Known, Source::Manual, ctx.day);
                }
                if learn {
                    ctx.progress
                        .start_learning(sense.id, Source::Study, ctx.day);
                }
                if knew || learn {
                    state.answered += 1;
                    state.right += 1;
                    move_on = true;
                }
            }
            (exercise, Some(right)) => {
                let detail = if right {
                    String::new()
                } else {
                    format!("Answer: {}", answer_text(ctx, exercise, &sense))
                };
                let note = if !right && matches!(exercise, Exercise::Fill(_) | Exercise::Spell) {
                    format!(
                        "You typed \u{201c}{}\u{201d}. {}",
                        active.typed.trim(),
                        active.note
                    )
                } else {
                    active.note.clone()
                };
                let title = if right { "Correct" } else { "Not quite" };
                if ui::feedback_sheet(ui, right, title, &detail, &note) {
                    move_on = true;
                }
            }
            (exercise @ (Exercise::Fill(_) | Exercise::Spell), None) => {
                let filled = !active.typed.trim().is_empty();
                ui::action_bar(ui, "actions", |ui| {
                    ui.add_enabled_ui(filled, |ui| {
                        if ui::primary_button(ui, "Check").clicked() {
                            checked = Some(typed_is_right(ctx, exercise, &sense, &active.typed));
                        }
                    });
                });
            }
            _ => {}
        }
    }

    // --- the question ---
    let voice = ctx.progress.accent;
    if let Some(active) = &mut state.active {
        ui::page(ui, "session", |ui| {
            let tone = match active.task {
                Task::Verify(_) => Tone::Known,
                Task::New(_) => Tone::Neutral,
                Task::Review(_) => Tone::Primary,
            };
            ui::chip(ui, &task_label(active.task, &active.exercise), tone);
            if let Task::Verify(_) = active.task {
                let ink2 = ui::palette(ui).ink2;
                ui.label(theme::caption("Checking a word you marked as known").color(ink2));
            }
            match active.exercise.clone() {
                Exercise::Study => new_word_card(ui, ctx, &sense, voice),
                Exercise::Meaning(choice) => {
                    word_prompt(ui, ctx, &sense, "What does it mean?", voice);
                    if let Some(i) = options(ui, &choice, active.picked) {
                        active.picked = Some(i);
                        checked = Some(i == choice.answer);
                    }
                }
                Exercise::PickWord(choice) => {
                    meaning_prompt(ui, &sense, "Which word means this?");
                    if let Some(i) = options(ui, &choice, active.picked) {
                        active.picked = Some(i);
                        checked = Some(i == choice.answer);
                    }
                }
                Exercise::Fill(gap) => {
                    gap_prompt(ui, &gap, active.verdict);
                    let hint = format!("starts with \u{201c}{}\u{201d}\u{2026}", gap.hint);
                    if answer_box(ui, active, &hint) {
                        checked = Some(gap.accepts(&active.typed));
                    }
                }
                Exercise::Spell => {
                    meaning_prompt(ui, &sense, "Write the word for this meaning");
                    if answer_box(ui, active, "type the English word\u{2026}") {
                        checked = Some(study::spelling_accepts(ctx.dict, &sense, &active.typed));
                    }
                    if active.verdict.is_none() {
                        first_letter_hint(ui, ctx, active, &sense);
                    }
                }
            }
        });
    }

    if let Some(right) = checked
        && state.active.as_ref().is_some_and(|a| a.verdict.is_none())
    {
        grade(ctx, state, right);
    }
    if move_on {
        advance(state);
    }
}

/// The right answer, spelled out for the verdict sheet.
fn answer_text(ctx: &Ctx, exercise: &Exercise, sense: &Sense) -> String {
    match exercise {
        Exercise::Meaning(choice) | Exercise::PickWord(choice) => choice
            .options
            .get(choice.answer)
            .cloned()
            .unwrap_or_default(),
        Exercise::Fill(gap) => gap.answer.clone(),
        Exercise::Spell | Exercise::Study => ctx.dict.word(sense.word).text.to_owned(),
    }
}

fn typed_is_right(ctx: &Ctx, exercise: &Exercise, sense: &Sense, typed: &str) -> bool {
    match exercise {
        Exercise::Fill(gap) => gap.accepts(typed),
        Exercise::Spell => study::spelling_accepts(ctx.dict, sense, typed),
        _ => false,
    }
}

/// Records the answer on the card and decides what Continue will do. The
/// verdict sheet shows from the next frame.
fn grade(ctx: &mut Ctx, state: &mut StudyState, correct: bool) {
    let Some(active) = &mut state.active else {
        return;
    };
    active.hesitated |= ctx.now - active.started > SLOW_SECONDS;
    active.verdict = Some(correct);
    state.answered += 1;
    state.right += u32::from(correct);

    let (task, level, hesitated) = (active.task, active.exercise.level(), active.hesitated);
    match task {
        // A new card is read, not graded; its buttons set the state.
        Task::New(_) => {}
        Task::Verify(id) => {
            ctx.progress.verify(id, correct, ctx.day);
            active.note = if correct {
                "Still known.".to_owned()
            } else {
                "It goes back into your reviews.".to_owned()
            };
        }
        Task::Review(id) => {
            let outcome = Outcome {
                correct,
                hesitated,
                level,
            };
            let (grade, _) = ctx.progress.answer(id, outcome, ctx.day);
            // Spec 3.6: a leech is set aside for three days rather than
            // drilled until it sticks.
            let leech = ctx.progress.card(id).is_some_and(|c| c.is_leech());
            let next = ctx.progress.card(id).map_or(0, |c| c.due - ctx.day);
            if leech {
                active.note = "This one keeps slipping, so it is set aside for 3 days.".to_owned();
            } else if grade == Grade::Again {
                active.after = After::Requeue;
                active.note = "It comes back later in this session.".to_owned();
            } else {
                active.note = format!("Next review {}.", ui::when(next));
            }
        }
    }
}

/// Moves the session on past the current task.
fn advance(state: &mut StudyState) {
    let after = state.active.as_ref().map_or(After::Advance, |a| a.after);
    if let Some(session) = &mut state.session {
        match after {
            After::Advance => session.advance(),
            After::Requeue => session.requeue(),
        }
    }
    state.active = None;
}

/// A brand-new card, read in full: spec 3.3 level 1, first sight.
fn new_word_card(ui: &mut egui::Ui, ctx: &mut Ctx, sense: &Sense, voice: crate::progress::Accent) {
    let word = ctx.dict.word(sense.word);
    let headword = ctx.progress.casing.apply(word.text);
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            ui.label(theme::display(&headword));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui::speak_buttons(ui, word.text, voice);
            });
        });
        if !word.ipa.is_empty() {
            ui.label(theme::body(word.ipa).color(p.ink2));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui::pos_chip(ui, sense.pos);
            ui::band_chip(ui, sense.band(), sense.rank);
        });
        ui.add_space(4.0);
        ui.separator();
        ui.add(egui::Label::new(meaning(sense.def)).wrap());
        if !sense.example.is_empty() {
            example(ui, sense.example, voice);
            ctx.shown.mark(sense.id);
        }
    });
    ui.label(
        theme::caption("Already know this word? Say so, and it won\u{2019}t be taught.")
            .color(p.ink2),
    );
}

/// A meaning at the size a card leads with.
fn meaning(def: &str) -> RichText {
    RichText::new(def).size(20.0).family(theme::semibold())
}

/// An example sentence, with a speaker where there is a voice.
fn example(ui: &mut egui::Ui, text: &str, voice: crate::progress::Accent) {
    let ink2 = ui::palette(ui).ink2;
    ui.horizontal(|ui| {
        let width = ui.available_width() - if ui::can_speak() { 52.0 } else { 0.0 };
        ui.allocate_ui_with_layout(egui::vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.add(egui::Label::new(RichText::new(text).size(15.0).italics().color(ink2)).wrap())
        });
        ui::speak_button(ui, text, voice);
    });
}

/// The prompt for "what does this word mean": the word, centred.
fn word_prompt(
    ui: &mut egui::Ui,
    ctx: &Ctx,
    sense: &Sense,
    ask: &str,
    voice: crate::progress::Accent,
) {
    let word = ctx.dict.word(sense.word);
    let headword = ctx.progress.casing.apply(word.text);
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.label(theme::label(ask).color(p.ink2));
            ui.label(theme::display(&headword));
            if !word.ipa.is_empty() || ui::can_speak() {
                ui::centered_row(ui, "ipa", |ui| {
                    if !word.ipa.is_empty() {
                        ui.label(theme::body(word.ipa).color(p.ink2));
                    }
                    ui::speak_buttons(ui, word.text, voice);
                });
            }
        });
    });
}

/// The prompt that shows a meaning and asks for the word.
fn meaning_prompt(ui: &mut egui::Ui, sense: &Sense, ask: &str) {
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(theme::label(ask).color(p.ink2));
        ui.add(egui::Label::new(meaning(sense.def)).wrap());
        ui::pos_chip(ui, sense.pos);
    });
}

/// A sentence with its gap; once answered, the gap shows the answer.
fn gap_prompt(ui: &mut egui::Ui, gap: &Cloze, verdict: Option<bool>) {
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(theme::label("Complete the sentence").color(p.ink2));
        let font = egui::FontId::new(20.0, theme::semibold());
        let plain = egui::TextFormat::simple(font.clone(), p.ink);
        let (middle, color) = match verdict {
            None => (gap.blank(), p.primary_ink),
            Some(true) => (gap.answer.clone(), p.known_ink),
            Some(false) => (gap.answer.clone(), p.wrong_ink),
        };
        let mut job = LayoutJob::default();
        job.append(&gap.before, 0.0, plain.clone());
        job.append(
            &middle,
            0.0,
            egui::TextFormat {
                font_id: font,
                color,
                underline: Stroke::new(2.0, color),
                ..Default::default()
            },
        );
        job.append(&gap.after, 0.0, plain);
        ui.add(egui::Label::new(job).wrap());
    });
}

/// The four options. Returns the one picked this frame, if any.
fn options(ui: &mut egui::Ui, choice: &Choice, picked: Option<usize>) -> Option<usize> {
    let mut chose = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        for (i, option) in choice.options.iter().enumerate() {
            let mark = ui::mark_for(picked, i, choice.answer);
            if ui::answer_option(ui, i, option, mark).clicked() && picked.is_none() {
                chose = Some(i);
            }
        }
    });
    if picked.is_none() {
        let keys = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
        ];
        for (i, key) in keys.into_iter().enumerate() {
            if i < choice.options.len() && ui.input(|input| input.key_pressed(key)) {
                chose = Some(i);
            }
        }
    }
    chose
}

/// The typed-answer box. Returns true when Enter submitted it.
fn answer_box(ui: &mut egui::Ui, active: &mut Active, hint: &str) -> bool {
    let state = match active.verdict {
        None => FieldState::Typing,
        Some(true) => FieldState::Right,
        Some(false) => FieldState::Wrong,
    };
    let field = ui::answer_field(ui, "answer", &mut active.typed, hint, state);
    if active.verdict.is_none() && !active.focused {
        field.request_focus();
        active.focused = true;
    }
    active.verdict.is_none()
        && !active.typed.trim().is_empty()
        && field.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// Spec 3.3 level 3's hint: the first letter, at the price of a hesitation.
fn first_letter_hint(ui: &mut egui::Ui, ctx: &Ctx, active: &mut Active, sense: &Sense) {
    let p = ui::palette(ui);
    if active.hesitated {
        let first = ctx.dict.word(sense.word).text.chars().next().unwrap_or('?');
        ui.label(theme::label(format!("Starts with \u{201c}{first}\u{201d}")).color(p.warn));
    } else {
        let width = ui.available_width().min(200.0);
        let hint = ui::button(
            ui,
            ui::Kind::Secondary,
            None,
            "Show first letter",
            egui::vec2(width, ui::TOUCH),
        );
        if hint.clicked() {
            active.hesitated = true;
        }
    }
}

fn summary(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    ctx.progress.mark_active(ctx.day);

    // Spec 2.3, rule 1: the frontier moves on once at least 90% of the 1.000
    // items just past it have been looked at. The end of a session is when
    // that can have become true.
    if !state.frontier_checked {
        state.frontier_checked = true;
        let start = ctx.progress.frontier;
        let end = (start + 1_000).min(ctx.dict.learn_count() + 1);
        let counts = ctx.progress.tally(ctx.dict.learn_span(start..end));
        let total: u32 = counts.iter().sum();
        if total > 0 {
            let explored = total - counts[State::Unexplored as usize];
            let before = ctx.progress.frontier;
            ctx.progress
                .advance_frontier(explored as f32 / total as f32);
            if ctx.progress.frontier != before {
                ctx.say(format!(
                    "New band unlocked: starting at #{}.",
                    ui::thousands(ctx.progress.frontier)
                ));
            }
        }
    }
    let mut back = false;
    ui::bar_header(ui, |_| {});
    let p = ui::palette(ui);
    ui::page(ui, "summary", |ui| {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(72.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(rect.center(), 36.0, p.known_soft);
            ui::paint_icon(
                ui.painter(),
                egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(36.0)),
                Icon::Check,
                p.known_ink,
                p.known_soft,
            );
            ui.add_space(8.0);
            ui.label(theme::title("Session complete"));
            ui.label(theme::caption("That\u{2019}s everything due today.").color(p.ink2));
        });
        ui.add_space(8.0);
        let rate = (state.right * 100).checked_div(state.answered).unwrap_or(0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let width = ((ui.available_width() - 16.0) / 3.0).floor();
            ui::stat_tile(ui, &state.answered.to_string(), "Questions", p.ink, width);
            ui::stat_tile(ui, &format!("{rate}%"), "Correct", p.known_ink, width);
            ui::stat_tile(
                ui,
                &ctx.progress.streak.to_string(),
                "Day streak",
                p.streak_ink,
                width,
            );
        });
        ui.add_space(8.0);
        back = ui::primary_button(ui, "Back to Study").clicked();
    });
    if back {
        state.leave();
    }
}

// -------------------------------------------------------------------------
// Quick Scan (spec 2.2)
// -------------------------------------------------------------------------

fn scan_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    let left = state.scan.len();
    let done = state.scan_total.saturating_sub(left);
    let fraction = done as f32 / state.scan_total.max(1) as f32;
    if ui::focus_header(ui, fraction, &format!("{left} left")) {
        state.leave();
        return;
    }

    let Some(&id) = state.scan.first() else {
        ctx.progress.mark_active(ctx.day);
        let mut back = false;
        ui::page(ui, "scan-done", |ui| {
            ui.add_space(32.0);
            ui.vertical_centered(|ui| {
                ui.label(theme::title("Scan complete"));
                let ink2 = ui::palette(ui).ink2;
                ui.label(
                    theme::caption("Anything you didn\u{2019}t know is now in your reviews.")
                        .color(ink2),
                );
            });
            ui.add_space(8.0);
            back = ui::primary_button(ui, "Back to Study").clicked();
        });
        if back {
            state.leave();
        }
        return;
    };

    // Spec 3.1: a "don't know" here goes straight into Learning.
    let sense = ctx.dict.sense(id);
    let (knew, learn) = ui::action_bar(ui, "actions", ui::decision_buttons);
    if knew {
        ctx.progress
            .set_state(sense.id, State::Known, Source::Manual, ctx.day);
    }
    if learn {
        ctx.progress
            .start_learning(sense.id, Source::Manual, ctx.day);
    }
    if knew || learn {
        ctx.progress.scanned_today += 1;
        state.scan.remove(0);
        state.reveal = false;
        return;
    }

    let word = ctx.dict.word(sense.word);
    let headword = ctx.progress.casing.apply(word.text);
    let voice = ctx.progress.accent;
    let p = ui::palette(ui);
    ui::page(ui, "scan", |ui| {
        ui.label(theme::label("Do you know this word?").color(p.ink2));
        ui::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.add_space(8.0);
                ui.label(theme::display(&headword));
                if !word.ipa.is_empty() || ui::can_speak() {
                    ui::centered_row(ui, "ipa", |ui| {
                        if !word.ipa.is_empty() {
                            ui.label(theme::body(word.ipa).color(p.ink2));
                        }
                        ui::speak_buttons(ui, word.text, voice);
                    });
                }
                ui::centered_row(ui, "chips", |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui::pos_chip(ui, sense.pos);
                    ui::band_chip(ui, sense.band(), sense.rank);
                });
                ui.add_space(8.0);
            });
        });
        let label = if state.reveal {
            "Hide the meaning"
        } else {
            "Show the meaning"
        };
        let icon = if state.reveal {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        };
        let width = ui.available_width();
        if ui::button(
            ui,
            ui::Kind::Ghost,
            Some(icon),
            label,
            egui::vec2(width, ui::TOUCH),
        )
        .clicked()
        {
            state.reveal = !state.reveal;
        }
        if state.reveal {
            ui::card(ui, |ui| {
                ui.add(egui::Label::new(theme::body_strong(sense.def)).wrap());
                if !sense.example.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(sense.example)
                                .size(15.0)
                                .italics()
                                .color(p.ink2),
                        )
                        .wrap(),
                    );
                }
            });
        }
    });
}
