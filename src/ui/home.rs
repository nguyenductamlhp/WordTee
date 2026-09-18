//! Home — the screen the app opens on: one English word, four Vietnamese
//! meanings, pick the right one.
//!
//! It is deliberately the lowest-friction way in. There is nothing to start and
//! nothing to finish: a question is always on screen, and answering it moves
//! the same FSRS card a study session would. A right answer lengthens the
//! interval and fills the mastery bar; a wrong one is a lapse, which shortens
//! the interval and drops the bar back.
//!
//! What it asks about is not random. Anything already due comes first, so the
//! quick game doubles as review; only when nothing is waiting does it reach for
//! new words from the Smart Feeding window (spec 2.3).

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::dict::{SenseId, WordId};
use crate::progress::{Source, State};
use crate::quiz::{self, Choice};
use crate::srs::Outcome;
use crate::study;
use crate::ui;

/// Answering slower than this counts as hesitation (spec 3.2).
const SLOW_SECONDS: f64 = 12.0;
const OPTIONS: usize = 4;

/// The question on screen.
struct Question {
    sense: SenseId,
    choice: Choice,
    /// `Context::input(|i| i.time)` when it appeared.
    started: f64,
    picked: Option<usize>,
    /// Mastery before the answer, so the bar can show what moved.
    was: f32,
    now: f32,
}

#[derive(Default)]
pub struct HomeState {
    question: Option<Question>,
    /// Avoids asking about the same headword twice running.
    last: Option<WordId>,
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

    pub fn answered(&self) -> (u32, u32) {
        (self.right, self.asked)
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut HomeState) {
    if state.question.is_none() {
        state.question = build(ctx, state.last);
    }
    if state.question.is_none() {
        return nothing_to_ask(ui, ctx);
    }
    // Copied out before the borrow below, which the widget closures rule out
    // holding alongside `state` itself.
    let (right, asked, run) = (state.right, state.asked, state.run);
    let mut finished: Option<(WordId, bool)> = None;

    let Some(question) = &mut state.question else {
        return;
    };
    let sense = ctx.dict.sense(question.sense);
    let word = ctx.dict.word(sense.word);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(6.0);
        score_line(ui, right, asked, run);
        ui.add_space(8.0);

        // --- the word ---
        ui::card(ui, Some(ui::accent(ui)), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(word.text).size(34.0).strong());
                ui.horizontal(|ui| {
                    if !word.ipa.is_empty() {
                        let color = ui::accent(ui);
                        ui.label(RichText::new(word.ipa).size(15.0).color(color));
                    }
                    ui::speak_buttons(ui, word.text);
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui::pos_chip(ui, sense.pos);
                    ui::band_chip(ui, sense.band(), sense.rank);
                });
                ui.add_space(6.0);
            });
        });

        ui.add_space(10.0);
        let muted = ui::muted(ui);
        ui.label(RichText::new("What does it mean?").size(13.0).color(muted));
        ui.add_space(4.0);

        // --- the four meanings ---
        let answered = question.picked.is_some();
        let mut chose = None;
        for (i, option) in question.choice.options.iter().enumerate() {
            let correct = i == question.choice.answer;
            let color = match question.picked {
                // Once answered, the right one is always shown, so a wrong
                // guess still teaches the word rather than only marking it.
                Some(_) if correct => ui::good(ui),
                Some(p) if p == i => ui::bad(ui),
                Some(_) => ui::muted(ui),
                None => ui.visuals().text_color(),
            };
            if ui::wide_button(ui, option, color).clicked() && !answered {
                chose = Some(i);
            }
            ui.add_space(4.0);
        }
        if let Some(i) = chose {
            grade(ctx, question, i, ctx.now);
        }

        // --- what the answer did ---
        if question.picked.is_some() {
            ui.add_space(8.0);
            mastery_bar(ui, question.was, question.now);
            ui.add_space(10.0);
            let accent = ui::accent(ui);
            let next = ui::wide_button(ui, "Next word", accent);
            if next.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                finished = Some((sense.word, question.picked == Some(question.choice.answer)));
            }
        }
        ui.add_space(24.0);
    });

    if let Some((word, was_right)) = finished {
        state.last = Some(word);
        state.asked += 1;
        state.right += u32::from(was_right);
        state.run = if was_right { state.run + 1 } else { 0 };
        state.question = None;
    }
}

/// Applies the answer to the card, through the same path a session uses.
fn grade(ctx: &mut Ctx, question: &mut Question, picked: usize, now: f64) {
    let sense = ctx.dict.sense(question.sense);
    question.was = ctx.progress.mastery(&sense);
    question.picked = Some(picked);

    let correct = picked == question.choice.answer;
    // Never seen before and answered right: the user knows it, so record that
    // rather than starting them on a word they have just demonstrated.
    if correct && ctx.progress.state(&sense) == State::Unexplored {
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
    question.now = ctx.progress.mastery(&sense);
    ctx.progress.mark_active(ctx.day);
    // Practising here counts as answering the reminder (spec 3.6).
    ctx.progress.mark_reminded(crate::progress::now_secs());
}

/// Score so far, kept small: this is a warm-up, not a test.
fn score_line(ui: &mut egui::Ui, right: u32, asked: u32, run: u32) {
    ui.horizontal(|ui| {
        let color = ui::muted(ui);
        if asked > 0 {
            ui.label(
                RichText::new(format!("{right}/{asked} correct"))
                    .size(12.5)
                    .color(color),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if run >= 3 {
                let good = ui::good(ui);
                ui::chip(ui, &format!("{run} in a row"), good);
            }
        });
    });
}

/// The mastery meter, showing where the answer moved it.
fn mastery_bar(ui: &mut egui::Ui, was: f32, now: f32) {
    let rose = now >= was;
    let color = if rose { ui::good(ui) } else { ui::bad(ui) };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Mastery").size(12.0).color(ui::muted(ui)));
        ui.label(
            RichText::new(format!(
                "{:.0}% {} {:.0}%",
                was * 100.0,
                if rose { "→" } else { "↓" },
                now * 100.0
            ))
            .size(12.0)
            .color(color),
        );
    });
    ui.add(
        egui::ProgressBar::new(now.clamp(0.0, 1.0))
            .desired_height(10.0)
            .fill(color),
    );
}

/// Picks the next item: due first, then learning, then something new.
fn build(ctx: &mut Ctx, skip: Option<WordId>) -> Option<Question> {
    let mut tried = Vec::new();
    for candidate in candidates(ctx) {
        let sense = ctx.dict.sense(candidate);
        if Some(sense.word) == skip || tried.contains(&sense.word) {
            continue;
        }
        tried.push(sense.word);
        if let Some(choice) = quiz::meaning_choice(ctx.dict, ctx.rng, &sense, OPTIONS) {
            let mastery = ctx.progress.mastery(&sense);
            return Some(Question {
                sense: candidate,
                choice,
                started: ctx.now,
                picked: None,
                was: mastery,
                now: mastery,
            });
        }
    }
    None
}

/// Candidate items, best first.
fn candidates(ctx: &mut Ctx) -> Vec<SenseId> {
    let mut out = ctx.progress.due_cards(ctx.day);
    // Then anything in progress, whether or not it is due yet.
    out.extend(
        ctx.progress
            .cards()
            .filter(|(_, c)| c.state.in_study())
            .map(|(id, _)| id),
    );
    // Then new words from the feeding window, so the game keeps going once
    // the queue is empty.
    out.extend(study::suggest(ctx.dict, ctx.progress, 12));
    // And finally anything at all, for a brand-new user with no frontier yet.
    if out.is_empty() {
        let span = ctx.dict.learn_count().min(3_000) as usize;
        for _ in 0..24 {
            let rank = 1 + ctx.rng.below(span) as u32;
            if let Some(sense) = ctx.dict.at_rank(rank) {
                out.push(sense.id);
            }
        }
    }
    out.retain(|id| {
        let sense = ctx.dict.sense(*id);
        sense.teachable()
    });
    out.truncate(40);
    out
}

/// Only reachable if the dictionary could not build a single question.
fn nothing_to_ask(ui: &mut egui::Ui, ctx: &mut Ctx) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("Nothing to practise").size(18.0).strong());
        ui.label(
            RichText::new("Look a word up and add it, and it will show up here.")
                .size(12.5)
                .color(ui::muted(ui)),
        );
        ui.add_space(10.0);
        if ui.button("Go to Look up").clicked() {
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
        rng: Rng,
        shown: Shown,
        toast: Option<crate::app::Toast>,
        goto: Option<crate::app::Tab>,
        open_word: Option<WordId>,
    }

    impl Bench {
        fn new() -> Self {
            Self {
                dict: Dict::load(),
                progress: Progress::default(),
                rng: Rng::seeded(9),
                shown: Shown::default(),
                toast: None,
                goto: None,
                open_word: None,
            }
        }

        fn ctx(&mut self) -> Ctx<'_> {
            Ctx {
                dict: &self.dict,
                progress: &mut self.progress,
                rng: &mut self.rng,
                day: 100,
                shown: &mut self.shown,
                toast: &mut self.toast,
                goto: &mut self.goto,
                open_word: &mut self.open_word,
                now: 0.0,
            }
        }
    }

    #[test]
    fn a_question_is_always_available_even_from_a_cold_start() {
        // A brand-new user has no frontier and no cards, and still has to get
        // a question on the very first frame.
        let mut bench = Bench::new();
        let mut ctx = bench.ctx();
        let question = build(&mut ctx, None).expect("built a question");
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
        let question = build(&mut ctx, None).expect("built");
        assert_eq!(question.sense, due.id, "reached past the due card");
    }

    #[test]
    fn the_same_word_is_not_asked_twice_running() {
        let mut bench = Bench::new();
        bench.progress.apply_placement(1_500, 7.3);
        let mut ctx = bench.ctx();
        let first = build(&mut ctx, None).expect("built");
        let word = ctx.dict.sense(first.sense).word;
        let second = build(&mut ctx, Some(word)).expect("built again");
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
            let mut question = build(&mut ctx, None).expect("built");
            question.sense = target.id;
            question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
            let answer = question.choice.answer;
            let picked = if correct {
                answer
            } else {
                (answer + 1) % OPTIONS
            };
            grade(&mut ctx, &mut question, picked, 0.0);

            if correct {
                assert!(
                    question.now >= question.was,
                    "{} -> {}",
                    question.was,
                    question.now
                );
            } else {
                assert!(
                    question.now < question.was,
                    "{} -> {}",
                    question.was,
                    question.now
                );
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
        let mut question = build(&mut ctx, None).expect("built");
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
        let mut question = build(&mut ctx, None).expect("built");
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
        let mut question = build(&mut ctx, None).expect("built");
        question.sense = target.id;
        question.choice = quiz::meaning_choice(ctx.dict, ctx.rng, &target, OPTIONS).unwrap();
        let answer = question.choice.answer;
        grade(&mut ctx, &mut question, answer, 0.0);
        assert_eq!(ctx.progress.streak, 1);
        assert!(ctx.progress.last_reminded > 0);
    }
}
