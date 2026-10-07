//! Home — the screen the app opens on: one English word, four Vietnamese
//! meanings, pick the right one.
//!
//! It is deliberately the lowest-friction way in. There is nothing to start and
//! nothing to finish: a question is always on screen, and answering it moves
//! the same FSRS card a study session would. A right answer lengthens the
//! interval and fills the mastery bar; a wrong one is a lapse, which shortens
//! the interval and drops the bar back. Today's session is one tap away above
//! it, for when there is time for more.
//!
//! What it asks about is not random. Anything already due comes first, so the
//! quick game doubles as review; only when nothing is waiting does it reach for
//! new words from the Smart Feeding window (spec 2.3).

use eframe::egui::{self, Align, Layout, Margin, RichText};

use crate::app::Ctx;
use std::collections::VecDeque;

use crate::dict::{SenseId, WordId};
use crate::progress::{Source, State};
use crate::quiz::{self, Choice};
use crate::srs::Outcome;
use crate::study;
use crate::ui::{self, Icon, Kind, Tone, theme};

/// Answering slower than this counts as hesitation (spec 3.2).
const SLOW_SECONDS: f64 = 12.0;
const OPTIONS: usize = 4;
/// How many of the most recent headwords to keep off the table.
const RECENT: usize = 12;
/// How many fresh words to draw on when the due queue runs dry. Wide enough
/// that the choice below has something to choose from.
const FRESH: usize = 40;

/// The question on screen.
struct Question {
    sense: SenseId,
    choice: Choice,
    /// `Context::input(|i| i.time)` when it appeared.
    started: f64,
    picked: Option<usize>,
}

#[derive(Default)]
pub struct HomeState {
    question: Option<Question>,
    /// The last [`RECENT`] headwords asked.
    ///
    /// One word of history is not enough: with a short queue it guarantees a
    /// ping-pong, A then B then A, which is exactly what this used to do.
    recent: std::collections::VecDeque<WordId>,
    asked: u32,
    right: u32,
    /// Consecutive right answers, for a bit of momentum.
    run: u32,
}

impl HomeState {
    /// Throws the current question away, so the next frame builds a fresh one.
    pub fn refresh(&mut self) {
        self.question = None;
    }

    /// Notes a headword as just asked.
    fn remember(&mut self, word: WordId) {
        self.recent.push_back(word);
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    pub fn answered(&self) -> (u32, u32) {
        (self.right, self.asked)
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut HomeState) {
    let streak = ctx.progress.streak;
    ui::screen_header(ui, "Quick practice", |ui| {
        if streak > 0 {
            ui::streak_pill(ui, streak);
        }
    });

    if state.question.is_none() {
        state.question = build(ctx, &state.recent);
    }
    if state.question.is_none() {
        return nothing_to_ask(ui, ctx);
    }
    // Copied out before the borrow below, which the widget closures rule out
    // holding alongside `state` itself.
    let run = state.run;
    let mut finished: Option<(WordId, bool)> = None;

    let Some(question) = &mut state.question else {
        return;
    };
    let sense = ctx.dict.sense(question.sense);
    let word = ctx.dict.word(sense.word);
    let voice = ctx.progress.accent;
    let headword = ctx.progress.casing.apply(word.text);
    let item_state = ctx.progress.state(&sense);

    ui::page(ui, "home", |ui| {
        session_strip(ui, ctx);

        // --- the word ---
        ui::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(theme::display(&headword));
                if !word.ipa.is_empty() || ui::can_speak() {
                    ui::centered_row(ui, "ipa", |ui| {
                        if !word.ipa.is_empty() {
                            let ink2 = ui::palette(ui).ink2;
                            ui.label(theme::body(word.ipa).color(ink2));
                        }
                        ui::speak_buttons(ui, word.text, voice);
                    });
                }
                ui::centered_row(ui, "chips", |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui::pos_chip(ui, sense.pos);
                    ui::band_chip(ui, sense.band(), sense.rank);
                    if item_state != State::Unexplored {
                        ui::state_chip(ui, item_state);
                    }
                });
            });
        });

        // --- the four meanings ---
        ui.horizontal(|ui| {
            let ink2 = ui::palette(ui).ink2;
            ui.label(theme::label("Pick the meaning").color(ink2));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if run >= 3 {
                    ui::chip(ui, &format!("{run} in a row"), Tone::Known);
                }
            });
        });

        let answered = question.picked.is_some();
        let mut chose = None;
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (i, option) in question.choice.options.iter().enumerate() {
                let mark = ui::mark_for(question.picked, i, question.choice.answer);
                if ui::answer_option(ui, i, option, mark).clicked() && !answered {
                    chose = Some(i);
                }
            }
        });
        // 1 to 4 on a keyboard, matching the A to D on the options.
        if !answered {
            let keys = [
                egui::Key::Num1,
                egui::Key::Num2,
                egui::Key::Num3,
                egui::Key::Num4,
            ];
            for (i, key) in keys.into_iter().enumerate() {
                if i < question.choice.options.len() && ui.input(|input| input.key_pressed(key)) {
                    chose = Some(i);
                }
            }
        }
        if let Some(i) = chose {
            grade(ctx, question, i, ctx.now);
        }

        // --- the verdict, and moving on ---
        if let Some(picked) = question.picked {
            let right = picked == question.choice.answer;
            let next = ui::when(
                ctx.progress
                    .card(sense.id)
                    .map_or(0, |card| card.due - ctx.day),
            );
            verdict(ui, right, &next);
            let go = ui::primary_button(ui, "Next word").clicked();
            if go || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                finished = Some((sense.word, right));
            }
        }
    });

    if let Some((word, was_right)) = finished {
        state.remember(word);
        state.asked += 1;
        state.right += u32::from(was_right);
        state.run = if was_right { state.run + 1 } else { 0 };
        state.question = None;
    }
}

/// Today's session, one tap away: what is waiting and a Start button.
fn session_strip(ui: &mut egui::Ui, ctx: &mut Ctx) {
    let (due, new, checks) = ctx.pending;
    if due + new + checks == 0 {
        return;
    }
    let p = ui::palette(ui);
    let mut parts = Vec::new();
    if due > 0 {
        parts.push(plural(due, "review", "reviews"));
    }
    if new > 0 {
        parts.push(format!("{new} new"));
    }
    if checks > 0 {
        parts.push(plural(checks, "check", "checks"));
    }
    ui::card_frame(ui)
        .corner_radius(14)
        .inner_margin(Margin {
            left: 16,
            right: 10,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(theme::label("Today\u{2019}s session"));
                    ui.label(
                        theme::caption(format!("{} waiting", parts.join(" · "))).color(p.ink2),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let start =
                        ui::button(ui, Kind::Primary, None, "Start", egui::vec2(88.0, 44.0));
                    if start.clicked() {
                        *ctx.start_session = true;
                    }
                });
            });
        });
}

/// "1 review", "3 reviews".
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One line saying how the answer went and when the word comes back.
fn verdict(ui: &mut egui::Ui, right: bool, next: &str) {
    let p = ui::palette(ui);
    let (icon, color, text) = if right {
        (
            Icon::Check,
            p.known_ink,
            format!("Right \u{2014} next review {next}."),
        )
    } else {
        (
            Icon::Cross,
            p.wrong_ink,
            format!("Not quite. You\u{2019}ll see it again {next}."),
        )
    };
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
        ui::paint_icon(ui.painter(), rect, icon, color, p.page);
        ui.add(egui::Label::new(theme::label(text).color(color)).wrap());
    });
}

/// Applies the answer to the card, through the same path a session uses.
fn grade(ctx: &mut Ctx, question: &mut Question, picked: usize, now: f64) {
    let sense = ctx.dict.sense(question.sense);
    question.picked = Some(picked);

    let correct = picked == question.choice.answer;
    // Never seen before and answered right: the user knows it, so record that
    // rather than starting them on a word they have just demonstrated. That
    // goes for a word from below the frontier that was only assumed known.
    let untested = matches!(
        ctx.progress.state(&sense),
        State::Unexplored | State::AssumedKnown
    );
    if correct && untested {
        ctx.progress
            .set_state(sense.id, State::Known, Source::Test, ctx.day);
    }
    let outcome = Outcome {
        correct,
        hesitated: now - question.started > SLOW_SECONDS,
        // Recognising a meaning is spec 3.3's level 1.
        level: 1,
    };
    ctx.progress.answer(sense.id, outcome, ctx.day);
    ctx.progress.mark_active(ctx.day);
    // Practising here counts as answering the reminder (spec 3.6).
    ctx.progress.mark_reminded(crate::progress::now_secs());
}

/// Picks the next item: something due if there is one, otherwise a new word.
///
/// Two rules keep it from circling. Nothing asked in the last [`RECENT`] rounds
/// is eligible, and the winner is drawn at random from those that are rather
/// than always being the head of a stable list — a list that is rebuilt in the
/// same order every round will hand back the same word every round.
fn build(ctx: &mut Ctx, recent: &VecDeque<WordId>) -> Option<Question> {
    let pool = candidates(ctx);
    let fresh: Vec<SenseId> = pool
        .iter()
        .copied()
        .filter(|id| !recent.contains(&ctx.dict.sense(*id).word))
        .collect();
    // If the history has swallowed everything — a tiny dictionary, or a very
    // short queue — fall back to the full pool rather than showing nothing.
    let mut choices = if fresh.is_empty() { pool } else { fresh };
    if choices.is_empty() {
        return None;
    }

    ctx.rng.shuffle(&mut choices);
    for candidate in choices {
        let sense = ctx.dict.sense(candidate);
        if let Some(choice) = quiz::meaning_choice(ctx.dict, ctx.rng, &sense, OPTIONS) {
            return Some(Question {
                sense: candidate,
                choice,
                started: ctx.now,
                picked: None,
            });
        }
    }
    None
}

/// Everything worth asking about right now.
///
/// Due cards first and on their own: they are the ones with review value, and
/// mixing new words in while something is overdue would waste the round. Only
/// when nothing is due does this reach for new words.
///
/// What it deliberately does *not* include is every card in progress
/// regardless of its due date. That was the loop: answering a word set its due
/// date days out, and this list put it straight back at the front anyway.
fn candidates(ctx: &mut Ctx) -> Vec<SenseId> {
    let due: Vec<SenseId> = ctx
        .progress
        .due_cards(ctx.day)
        .into_iter()
        .filter(|id| ctx.dict.sense(*id).teachable())
        .collect();
    if !due.is_empty() {
        return due;
    }

    let mut out: Vec<SenseId> = study::feed(ctx.dict, ctx.progress, ctx.rng, FRESH)
        .into_iter()
        .filter(|id| ctx.dict.sense(*id).teachable())
        .collect();
    if out.len() >= OPTIONS {
        return out;
    }

    // Nothing placed and nothing suggested — a first run. Draw from the common
    // end of the list so the first words a user meets are worth knowing.
    let span = ctx.dict.learn_count().min(3_000) as usize;
    for _ in 0..FRESH * 2 {
        if out.len() >= FRESH {
            break;
        }
        let rank = 1 + ctx.rng.below(span) as u32;
        if let Some(sense) = ctx.dict.at_rank(rank)
            && sense.teachable()
            && !out.contains(&sense.id)
        {
            out.push(sense.id);
        }
    }
    out
}

/// Only reachable if the dictionary could not build a single question.
fn nothing_to_ask(ui: &mut egui::Ui, ctx: &mut Ctx) {
    ui::page(ui, "home-empty", |ui| {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            let ink2 = ui::palette(ui).ink2;
            ui.label(theme::heading("Nothing to practise"));
            ui.label(
                RichText::new("Look a word up and add it, and it will show up here.")
                    .size(theme::size::CAPTION)
                    .color(ink2),
            );
        });
        if ui::primary_button(ui, "Go to Look up").clicked() {
            *ctx.goto = Some(crate::app::Tab::Lookup);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::Dict;
    use crate::progress::Progress;
    use crate::quiz::Shown;
    use crate::rng::Rng;

    /// A `Ctx` without a running egui frame, for the non-drawing parts.
    struct Bench {
        dict: Dict,
        progress: Progress,
        google: crate::google::Google,
        rng: Rng,
        shown: Shown,
        toast: Option<crate::app::Toast>,
        goto: Option<crate::app::Tab>,
        open_word: Option<WordId>,
        start_session: bool,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                dict: Dict::load(),
                progress: Progress::default(),
                google: crate::google::Google::default(),
                rng: Rng::seeded(9),
                shown: Shown::default(),
                toast: None,
                goto: None,
                open_word: None,
                start_session: false,
            }
        }

        fn ctx(&mut self) -> Ctx<'_> {
            Ctx {
                dict: &self.dict,
                progress: &mut self.progress,
                google: &mut self.google,
                rng: &mut self.rng,
                day: 100,
                shown: &mut self.shown,
                toast: &mut self.toast,
                goto: &mut self.goto,
                open_word: &mut self.open_word,
                start_session: &mut self.start_session,
                pending: (0, 0, 0),
                now: 0.0,
            }
        }
    }

    /// Plays `rounds` questions the way the screen does — build, answer,
    /// remember — and reports which headwords came up.
    fn play(bench: &mut Bench, rounds: usize, correct: bool) -> Vec<&'static str> {
        let mut state = HomeState::default();
        let mut seen = Vec::new();
        for _ in 0..rounds {
            let mut ctx = bench.ctx();
            let Some(mut question) = build(&mut ctx, &state.recent) else {
                break;
            };
            let sense = ctx.dict.sense(question.sense);
            seen.push(ctx.dict.word(sense.word).text);
            let answer = question.choice.answer;
            let picked = if correct {
                answer
            } else {
                (answer + 1) % OPTIONS
            };
            grade(&mut ctx, &mut question, picked, 0.0);
            state.remember(sense.word);
        }
        seen
    }

    #[test]
    #[ignore = "diagnostic: cargo test -- --ignored --nocapture show_a_session"]
    fn show_a_session() {
        for (label, placed, correct) in [
            ("placed at 2,000, all right", true, true),
            ("placed at 2,000, all wrong", true, false),
            ("brand-new user", false, true),
        ] {
            let mut bench = Bench::new();
            if placed {
                bench.progress.apply_placement(2_000, 7.6);
            }
            let seen = play(&mut bench, 24, correct);
            let distinct: std::collections::BTreeSet<_> = seen.iter().collect();
            println!("\n{label}: {} distinct of {}", distinct.len(), seen.len());
            println!("  {}", seen.join(" "));
        }
    }

    #[test]
    fn it_does_not_loop_over_the_same_few_words() {
        // The bug this test exists for: answering a word put it straight back
        // at the front of the queue, so two words ping-ponged for ever.
        let mut bench = Bench::new();
        bench.progress.apply_placement(2_000, 7.6);
        let seen = play(&mut bench, 30, true);
        assert_eq!(seen.len(), 30, "ran out of questions");

        let distinct: std::collections::BTreeSet<_> = seen.iter().collect();
        assert!(
            distinct.len() >= 20,
            "only {} distinct words in 30 rounds: {:?}",
            distinct.len(),
            seen
        );
    }

    #[test]
    fn a_word_does_not_come_back_immediately_after_being_missed() {
        // Getting one wrong must not pin it to the front of the queue either.
        let mut bench = Bench::new();
        bench.progress.apply_placement(1_200, 7.1);
        let seen = play(&mut bench, 20, false);
        let distinct: std::collections::BTreeSet<_> = seen.iter().collect();
        assert!(
            distinct.len() >= 12,
            "only {} distinct after wrong answers: {:?}",
            distinct.len(),
            seen
        );
    }

    #[test]
    fn no_word_repeats_inside_the_recent_window() {
        let mut bench = Bench::new();
        bench.progress.apply_placement(3_000, 8.0);
        let seen = play(&mut bench, 40, true);
        for (i, word) in seen.iter().enumerate() {
            let window = &seen[i.saturating_sub(RECENT)..i];
            assert!(
                !window.contains(word),
                "{word} repeated within {RECENT}: {seen:?}"
            );
        }
    }

    #[test]
    fn a_question_is_always_available_even_from_a_cold_start() {
        // A brand-new user has no frontier and no cards, and still has to get
        // a question on the very first frame.
        let mut bench = Bench::new();
        let mut ctx = bench.ctx();
        let question = build(&mut ctx, &VecDeque::new()).expect("built a question");
        assert_eq!(question.choice.options.len(), OPTIONS);
        let sense = ctx.dict.sense(question.sense);
        assert_eq!(question.choice.options[question.choice.answer], sense.def);
        assert!(sense.teachable());
    }

    #[test]
    fn due_cards_are_asked_before_new_ones() {
        let mut bench = Bench::new();
        bench.progress.apply_placement(2_000, 7.6);
        let due = bench.dict.at_rank(400).unwrap();
        bench.progress.start_learning(due.id, Source::Manual, 90);

        let mut ctx = bench.ctx();
        let question = build(&mut ctx, &VecDeque::new()).expect("built");
        assert_eq!(question.sense, due.id, "reached past the due card");
    }

    #[test]
    fn a_remembered_word_is_not_offered_again() {
        let mut bench = Bench::new();
        bench.progress.apply_placement(1_500, 7.3);
        let mut ctx = bench.ctx();
        let first = build(&mut ctx, &VecDeque::new()).expect("built");
        let word = ctx.dict.sense(first.sense).word;
        let recent = VecDeque::from(vec![word]);
        let second = build(&mut ctx, &recent).expect("built again");
        assert_ne!(ctx.dict.sense(second.sense).word, word);
    }

    #[test]
    fn a_right_answer_raises_mastery_and_a_wrong_one_lowers_it() {
        for correct in [true, false] {
            let mut bench = Bench::new();
            let target = bench.dict.at_rank(1_200).unwrap();
            // Start from something already part-learned, so there is room to
            // move in both directions.
            bench.progress.start_learning(target.id, Source::Manual, 90);
            for _ in 0..4 {
                let day = bench.progress.card(target.id).unwrap().due;
                bench.progress.answer(
                    target.id,
                    Outcome {
                        correct: true,
                        hesitated: false,
                        level: 2,
                    },
                    day,
                );
            }

            let mut ctx = bench.ctx();
            let mut question = build(&mut ctx, &VecDeque::new()).expect("built");
            question.sense = target.id;
            question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
            let answer = question.choice.answer;
            let picked = if correct {
                answer
            } else {
                (answer + 1) % OPTIONS
            };
            let before = ctx.progress.mastery(&target);
            grade(&mut ctx, &mut question, picked, 0.0);
            let after = ctx.progress.mastery(&target);

            if correct {
                assert!(after >= before, "right answer: {before} -> {after}");
            } else {
                assert!(after < before, "wrong answer: {before} -> {after}");
            }
        }
    }

    #[test]
    fn a_wrong_answer_puts_the_word_back_into_learning() {
        let mut bench = Bench::new();
        let target = bench.dict.at_rank(900).unwrap();
        bench
            .progress
            .set_state(target.id, State::Known, Source::Manual, 90);

        let mut ctx = bench.ctx();
        let mut question = build(&mut ctx, &VecDeque::new()).expect("built");
        question.sense = target.id;
        question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
        let wrong = (question.choice.answer + 1) % OPTIONS;
        grade(&mut ctx, &mut question, wrong, 0.0);
        assert_eq!(ctx.progress.state(&target), State::Learning);
    }

    #[test]
    fn getting_an_unseen_word_right_records_it_as_known() {
        let mut bench = Bench::new();
        let target = bench.dict.at_rank(4_000).unwrap();
        assert_eq!(bench.progress.state(&target), State::Unexplored);

        let mut ctx = bench.ctx();
        let mut question = build(&mut ctx, &VecDeque::new()).expect("built");
        question.sense = target.id;
        question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
        let answer = question.choice.answer;
        grade(&mut ctx, &mut question, answer, 0.0);
        // Not left as Unexplored, and not dropped into Learning either: the
        // user just demonstrated it.
        assert_ne!(ctx.progress.state(&target), State::Unexplored);
        assert!(ctx.progress.mastery(&target) > 0.0);
    }

    #[test]
    fn answering_counts_as_studying() {
        // It keeps the streak and answers the reminder, like a session would.
        let mut bench = Bench::new();
        let target = bench.dict.at_rank(1_000).unwrap();
        let mut ctx = bench.ctx();
        let mut question = build(&mut ctx, &VecDeque::new()).expect("built");
        question.sense = target.id;
        question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
        let answer = question.choice.answer;
        grade(&mut ctx, &mut question, answer, 0.0);
        assert_eq!(ctx.progress.streak, 1);
        assert!(ctx.progress.last_reminded > 0);
    }
}
