//! You — the placement test (spec 2.2), the settings spec 3.2 and 3.6 expose,
//! and the data credits spec 4.3 requires.

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::placement::{FALSE_ALARM_LIMIT, MAX_ITEMS, Placement, Verdict};
use crate::progress::{Accent, Casing, Progress, Reminders, State, Theme};
use crate::ui;

#[derive(Default)]
pub struct ProfileState {
    test: Option<Placement>,
    /// Shown after the test ends, until dismissed.
    result: Option<Verdict>,
    /// Picked an option on the question currently displayed.
    picked: Option<usize>,
    confirm_reset: bool,
}

impl ProfileState {
    /// Starts the placement test, as the button on this screen does.
    pub fn begin_test(&mut self, dict: &crate::dict::Dict) {
        self.test = Some(Placement::new(dict));
        self.picked = None;
    }

    /// Is a placement test running?
    pub fn testing(&self) -> bool {
        self.test.is_some()
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    if state.test.is_some() {
        return test_page(ui, ctx, state);
    }
    if let Some(verdict) = state.result {
        return result_page(ui, ctx, state, verdict);
    }
    home(ui, ctx, state);
}

/// Says where a reminder can actually be delivered on this platform.
fn reminder_note(ui: &mut egui::Ui, ctx: &mut Ctx) {
    if ctx.progress.reminders == Reminders::Off {
        return;
    }
    let note = if !ui::can_notify() {
        "Shown inside the app. Notifications outside it need a platform \
         integration this build does not have."
    } else if ui::notifications_allowed() {
        "Sent as a browser notification while WordTee is open, and shown \
         inside the app either way."
    } else {
        "Shown inside the app. Allow notifications to get them from the \
         browser too."
    };
    let muted = ui::muted(ui);
    ui.label(RichText::new(note).size(11.5).color(muted));
    if ui::can_notify()
        && !ui::notifications_allowed()
        && ui.button("Allow notifications").clicked()
    {
        ui::request_notifications();
    }
    ui.add_space(4.0);
}

fn home(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);

        // --- where the user stands ---
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Level").size(17.0).strong());
            ui.add_space(4.0);
            if ctx.progress.placement_done {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Vocabulary frontier:")
                            .size(13.0)
                            .color(ui::muted(ui)),
                    );
                    ui.label(
                        RichText::new(format!("#{}", ui::thousands(ctx.progress.assumed_below)))
                            .size(15.0)
                            .strong()
                            .color(ui::accent(ui)),
                    );
                });
                ui.label(
                    RichText::new(format!(
                        "Learning from #{} onwards.",
                        ui::thousands(ctx.progress.frontier)
                    ))
                    .size(12.5)
                    .color(ui::muted(ui)),
                );
            } else {
                ui.label(
                    RichText::new("Placement test not taken yet.")
                        .size(13.0)
                        .color(ui::muted(ui)),
                );
            }
            ui.add_space(8.0);
            let label = if ctx.progress.placement_done {
                "Retake the test"
            } else {
                "Take the placement test"
            };
            if ui::wide_button(ui, label, ui::accent(ui)).clicked() {
                state.test = Some(Placement::new(ctx.dict));
                state.picked = None;
            }
            ui.label(
                RichText::new(
                    "15–25 questions, with invented words mixed in to catch guessing. Retake it any time.",
                )
                .size(11.5)
                .color(ui::muted(ui)),
            );
        });

        // --- what has been learned ---
        ui.add_space(8.0);
        let counts = ctx
            .progress
            .tally(ctx.dict.learn_span(1..ctx.dict.learn_count() + 1));
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Progress").size(17.0).strong());
            ui.add_space(6.0);
            ui::progress_bar(ui, counts, 14.0);
            ui.add_space(6.0);
            egui::Grid::new("tally")
                .num_columns(2)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    for state_kind in [
                        State::Mastered,
                        State::Known,
                        State::AssumedKnown,
                        State::Review,
                        State::Learning,
                        State::Unexplored,
                    ] {
                        ui::state_chip(ui, state_kind);
                        ui.label(
                            RichText::new(ui::thousands(counts[state_kind as usize]))
                                .size(13.0)
                                .color(ui::muted(ui)),
                        );
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "{} day streak · best {}",
                    ctx.progress.streak, ctx.progress.best_streak
                ))
                .size(12.5)
                .color(ui::muted(ui)),
            );
        });

        // --- settings, grouped as in the reference design ---
        ui::settings_group(ui, "Learning", |ui| {
            let frontier = ctx.progress.assumed_below;
            let level = if ctx.progress.placement_done {
                format!("#{}", ui::thousands(frontier))
            } else {
                "Not taken".to_owned()
            };
            // Spec 2.2 is emphatic that self-assessment runs high, so there is
            // no "pick your level" here — the test is the way in.
            if ui::value_row(ui, ui::Icon::Letters, "Placement test", &level) {
                state.test = Some(Placement::new(ctx.dict));
                state.picked = None;
            }

            let accent = ctx.progress.accent;
            let labels: Vec<&str> = Accent::ALL.iter().map(|a| a.label()).collect();
            let at = Accent::ALL.iter().position(|a| *a == accent).unwrap_or(1);
            if let Some(i) = ui::choice_row(ui, ui::Icon::Speaker, "Accent", &labels, at, false) {
                ctx.progress.accent = Accent::ALL[i];
            }

            let goals = ["5", "10", "20"];
            let at = goals
                .iter()
                .position(|g| g.parse() == Ok(ctx.progress.daily_goal))
                .unwrap_or(1);
            if let Some(i) = ui::choice_row(ui, ui::Icon::Target, "New words a day", &goals, at, false)
            {
                ctx.progress.daily_goal = goals[i].parse().unwrap_or(10);
            }

            let rates: Vec<&str> = Reminders::ALL.iter().map(|r| r.label()).collect();
            let at = Reminders::ALL
                .iter()
                .position(|r| *r == ctx.progress.reminders)
                .unwrap_or(0);
            if let Some(i) = ui::choice_row(ui, ui::Icon::Bell, "Reminders", &rates, at, true) {
                ctx.progress.reminders = Reminders::ALL[i];
                let now = crate::progress::now_secs();
                ctx.progress.mark_reminded(now);
            }

            let mut alerts = ctx.progress.streak_alerts;
            if ui::switch_row(ui, ui::Icon::Star, "Streak alerts", &mut alerts) {
                ctx.progress.streak_alerts = alerts;
            }
            reminder_note(ui, ctx);
        });

        ui::settings_group(ui, "Review cards", |ui| {
            let mut examples = ctx.progress.show_examples;
            if ui::switch_row(ui, ui::Icon::Study, "Word examples", &mut examples) {
                ctx.progress.show_examples = examples;
            }
            let mut speak = ctx.progress.auto_pronounce;
            if ui::switch_row(ui, ui::Icon::Speaker, "Pronounce on show", &mut speak) {
                ctx.progress.auto_pronounce = speak;
            }
            let mut hard = ctx.progress.hard_word_alert;
            if ui::switch_row(ui, ui::Icon::Bell, "Hard word alert", &mut hard) {
                ctx.progress.hard_word_alert = hard;
            }

            let cases: Vec<&str> = Casing::ALL.iter().map(|c| c.label()).collect();
            let at = Casing::ALL
                .iter()
                .position(|c| *c == ctx.progress.casing)
                .unwrap_or(0);
            if let Some(i) = ui::choice_row(ui, ui::Icon::Letters, "Casing", &cases, at, false) {
                ctx.progress.casing = Casing::ALL[i];
            }

            ui.add_space(2.0);
            let muted = ui::muted(ui);
            ui.label(RichText::new("CHALLENGE TYPES").size(11.0).color(muted));
            ui.add_space(2.0);
            // Spec 3.3's three levels. The last one on cannot be cleared, or a
            // session would have nothing to ask.
            let mut challenges = ctx.progress.challenges;
            let only_one = challenges.count() == 1;
            for (label, flag) in [
                ("Recognise \u{2014} pick the meaning", &mut challenges.recognise),
                ("Recall \u{2014} fill the gap", &mut challenges.recall),
                ("Produce \u{2014} spell it out", &mut challenges.produce),
            ] {
                let locked = only_one && *flag;
                ui.add_enabled_ui(!locked, |ui| {
                    ui.checkbox(flag, RichText::new(label).size(13.0));
                });
            }
            if challenges.count() > 0 {
                ctx.progress.challenges = challenges;
            }
            if only_one {
                ui.label(
                    RichText::new("At least one has to stay on.")
                        .size(11.0)
                        .color(muted),
                );
            }

            ui.add_space(6.0);
            // Spec 3.2: desired retention, adjustable between 0,8 and 0,95.
            ui.label(RichText::new("Target retention").size(13.5).strong());
            ui.add(
                egui::Slider::new(&mut ctx.progress.retention, 0.8..=0.95)
                    .fixed_decimals(2)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            ui.label(
                RichText::new("Higher means firmer recall, but more reviews to sit through.")
                    .size(11.5)
                    .color(muted),
            );
        });

        ui::settings_group(ui, "General", |ui| {
            let dark = ctx.progress.theme == Theme::Dark;
            ui.horizontal(|ui| {
                let color = ui::accent(ui);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                let behind = ui.visuals().faint_bg_color;
                ui::paint_icon(
                    ui.painter(),
                    rect,
                    if dark { ui::Icon::Moon } else { ui::Icon::Sun },
                    color,
                    behind,
                );
                ui.add_space(4.0);
                ui.label(RichText::new("Theme").size(14.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(i) = ui::segmented(ui, &["Light", "Dark"], usize::from(dark), false)
                    {
                        ctx.progress.theme = if i == 1 { Theme::Dark } else { Theme::Light };
                    }
                });
            });
            ui.add_space(6.0);

            // Spec 2.3, rule 3: the Skip Band.
            if ctx.progress.placement_done {
                ui.label(RichText::new("Skip a rank band").size(13.5).strong());
                ui.horizontal_wrapped(|ui| {
                    for jump in [1_000u32, 3_000] {
                        if ui.button(format!("+{}", ui::thousands(jump))).clicked() {
                            ctx.progress.frontier =
                                (ctx.progress.frontier + jump).min(ctx.dict.learn_count());
                            ctx.say("Jumped to a higher band. The skipped range still comes up in Quick scan.");
                        }
                    }
                    if ui.button("Reset").clicked() {
                        ctx.progress.frontier = ctx.progress.assumed_below + 1;
                    }
                });
            }
        });

        // --- credits (spec 4.3) ---
        ui.add_space(8.0);
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Data sources").size(17.0).strong());
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Dictionary: {} headwords · {} senses · {} learning items",
                    ui::thousands(ctx.dict.word_count()),
                    ui::thousands(ctx.dict.sense_count()),
                    ui::thousands(ctx.dict.learn_count())
                ))
                .size(12.5)
                .color(ui::muted(ui)),
            );
            ui.add_space(4.0);
            for (what, source, license) in [
                (
                    "Headwords and senses",
                    "minhqnd/dictionary (Wiktionary, TVTD)",
                    "CC BY-SA",
                ),
                (
                    "Word frequency",
                    "hermitdave/FrequencyWords (OpenSubtitles)",
                    "CC BY-SA",
                ),
                ("Review scheduling", "FSRS-5", "MIT"),
                ("Typeface", "Noto Sans", "OFL 1.1"),
            ] {
                ui.label(
                    RichText::new(format!("• {what}: {source} — {license}"))
                        .size(11.5)
                        .color(ui::muted(ui)),
                );
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Everything lives on your device: lookup, study and review all work offline.",
                )
                .size(11.5)
                .color(ui::muted(ui)),
            );
        });

        // --- reset ---
        ui.add_space(8.0);
        if state.confirm_reset {
            ui::card(ui, Some(ui::bad(ui)), |ui| {
                ui.label(RichText::new("Erase all progress?").strong());
                ui.label(
                    RichText::new("This cannot be undone.")
                        .size(12.0)
                        .color(ui::muted(ui)),
                );
                ui.add_space(6.0);
                let danger = ui::bad(ui);
                ui.columns(2, |c| {
                    if c[0].button("Cancel").clicked() {
                        state.confirm_reset = false;
                    }
                    if c[1].button(RichText::new("Erase").color(danger)).clicked() {
                        *ctx.progress = Progress::default();
                        ctx.progress.roll_to(ctx.day);
                        state.confirm_reset = false;
                        ctx.say("Progress erased.");
                    }
                });
            });
        } else if ui
            .button(RichText::new("Erase progress").size(12.0).color(ui::muted(ui)))
            .clicked()
        {
            state.confirm_reset = true;
        }
        ui.add_space(16.0);
    });
}

// -------------------------------------------------------------------------
// the placement test (spec 2.2)
// -------------------------------------------------------------------------

fn test_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    // Everything the frame needs is copied out first: the widget closures
    // below would otherwise hold `state` borrowed while the answer handler
    // needs it back.
    let Some(test) = state.test.as_ref() else {
        return;
    };
    let (fraction, warned, asked) = (test.progress(), test.warned, test.asked());
    let question = test
        .question()
        .map(|q| (q.choice.prompt.clone(), q.choice.options.clone()));

    let mut quit = false;
    egui::Panel::top("test-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            quit = ui.small_button("×").clicked();
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_height(8.0)
                    .fill(ui::accent(ui)),
            );
        });
        ui.label(
            RichText::new(format!("question {} / ~{}", asked + 1, MAX_ITEMS))
                .size(11.5)
                .color(ui::muted(ui)),
        );
        ui.add_space(4.0);
    });
    if quit {
        state.test = None;
        return;
    }

    // Spec 2.2: warn a user who is guessing at the pseudo-words.
    if warned {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "Careful: you are picking meanings for words that do not exist (over \
                 {:.0}%). Press “I don't know” when you are not sure.",
                FALSE_ALARM_LIMIT * 100.0
            ))
            .size(12.0)
            .color(ui::warn(ui)),
        );
    }

    let Some((prompt, options)) = question else {
        // Out of questions: record the result (spec 2.2's Assumed_Known).
        let verdict = test.verdict();
        ctx.progress
            .apply_placement(verdict.frontier, verdict.theta);
        state.result = Some(verdict);
        state.test = None;
        return;
    };

    let mut answered: Option<Option<usize>> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(10.0);
        ui::card(ui, Some(ui::accent(ui)), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(&prompt).size(28.0).strong());
                ui.add_space(4.0);
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("What does this word mean?")
                .size(13.0)
                .color(ui::muted(ui)),
        );
        ui.add_space(6.0);

        for (i, option) in options.iter().enumerate() {
            if ui::wide_button(ui, option, ui.visuals().text_color()).clicked() {
                answered = Some(Some(i));
            }
            ui.add_space(3.0);
        }
        ui.add_space(8.0);
        // Spec 2.2 requires this: not knowing must not have to be a guess.
        if ui::wide_button(ui, "I don't know", ui::muted(ui)).clicked() {
            answered = Some(None);
        }
        ui.add_space(16.0);
    });

    if let Some(pick) = answered {
        if let Some(test) = state.test.as_mut() {
            test.answer(ctx.dict, pick);
        }
        state.picked = None;
    }
}

fn result_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState, verdict: Verdict) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Result").size(20.0).strong());
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("#{}", ui::thousands(verdict.frontier)))
                    .size(38.0)
                    .strong()
                    .color(ui::accent(ui)),
            );
            ui.label(
                RichText::new("vocabulary frontier")
                    .size(12.5)
                    .color(ui::muted(ui)),
            );
        });
        ui.add_space(12.0);

        ui::card(ui, None, |ui| {
            egui::Grid::new("verdict")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Questions").size(13.0).color(ui::muted(ui)));
                    ui.label(RichText::new(verdict.asked.to_string()).size(13.0));
                    ui.end_row();
                    ui.label(
                        RichText::new("Answered correctly")
                            .size(13.0)
                            .color(ui::muted(ui)),
                    );
                    ui.label(RichText::new(format!("{:.0}%", verdict.raw_rate * 100.0)).size(13.0));
                    ui.end_row();
                    ui.label(
                        RichText::new("After guess correction")
                            .size(13.0)
                            .color(ui::muted(ui)),
                    );
                    ui.label(
                        RichText::new(format!("{:.0}%", verdict.corrected_rate * 100.0)).size(13.0),
                    );
                    ui.end_row();
                    ui.label(
                        RichText::new("Claimed to know invented words")
                            .size(13.0)
                            .color(ui::muted(ui)),
                    );
                    ui.label(
                        RichText::new(format!("{:.0}%", verdict.false_alarm * 100.0))
                            .size(13.0)
                            .color(if verdict.false_alarm > FALSE_ALARM_LIMIT {
                                ui::warn(ui)
                            } else {
                                ui::muted(ui)
                            }),
                    );
                    ui.end_row();
                });
        });

        // Spec 2.2's output: the estimated share known, block by block.
        ui.add_space(8.0);
        ui::section(ui, "Estimate by block", |ui| {
            for block in 0..5u32 {
                let start = block * 1_000;
                let share = verdict.known_share(start);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} – {}",
                            ui::thousands(start + 1),
                            ui::thousands(start + 1_000)
                        ))
                        .size(12.0)
                        .color(ui::muted(ui)),
                    );
                    ui.add(
                        egui::ProgressBar::new(share)
                            .desired_height(8.0)
                            .fill(ui::c_assumed(ui))
                            .text(RichText::new(format!("{:.0}%", share * 100.0)).size(11.0)),
                    );
                });
            }
        });

        ui.add_space(14.0);
        if ui::wide_button(ui, "Start studying", ui::accent(ui)).clicked() {
            state.result = None;
            *ctx.goto = Some(crate::app::Tab::Study);
        }
        ui.label(
            RichText::new(
                "Words below this mark are assumed known but unverified — use \
                 Quick scan to find the gaps.",
            )
            .size(11.5)
            .color(ui::muted(ui)),
        );
        ui.add_space(16.0);
    });
}
