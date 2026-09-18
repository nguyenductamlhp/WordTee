//! Tôi — the placement test (spec 2.2), the settings spec 3.2 and 3.6 expose,
//! and the data credits spec 4.3 requires.

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::placement::{FALSE_ALARM_LIMIT, MAX_ITEMS, Placement, Verdict};
use crate::progress::{Progress, State};
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

fn home(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);

        // --- where the user stands ---
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Trình độ").size(17.0).strong());
            ui.add_space(4.0);
            if ctx.progress.placement_done {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Đường chân trời từ vựng:")
                            .size(13.0)
                            .color(ui::MUTED),
                    );
                    ui.label(
                        RichText::new(format!("#{}", ui::thousands(ctx.progress.assumed_below)))
                            .size(15.0)
                            .strong()
                            .color(ui::ACCENT),
                    );
                });
                ui.label(
                    RichText::new(format!(
                        "Đang học từ #{} trở đi.",
                        ui::thousands(ctx.progress.frontier)
                    ))
                    .size(12.5)
                    .color(ui::MUTED),
                );
            } else {
                ui.label(
                    RichText::new("Chưa làm bài test đầu vào.")
                        .size(13.0)
                        .color(ui::MUTED),
                );
            }
            ui.add_space(8.0);
            let label = if ctx.progress.placement_done {
                "Làm lại bài test"
            } else {
                "Làm bài test đầu vào"
            };
            if ui::wide_button(ui, label, ui::ACCENT).clicked() {
                state.test = Some(Placement::new(ctx.dict));
                state.picked = None;
            }
            ui.label(
                RichText::new(
                    "15–25 câu, có cả từ giả để phát hiện đoán mò. Làm lại lúc nào cũng được.",
                )
                .size(11.5)
                .color(ui::MUTED),
            );
        });

        // --- what has been learned ---
        ui.add_space(8.0);
        let counts = ctx
            .progress
            .tally(ctx.dict.learn_span(1..ctx.dict.learn_count() + 1));
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Tiến độ").size(17.0).strong());
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
                                .color(ui::MUTED),
                        );
                        ui.end_row();
                    }
                });
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "🔥 chuỗi {} ngày · kỷ lục {}",
                    ctx.progress.streak, ctx.progress.best_streak
                ))
                .size(12.5)
                .color(ui::MUTED),
            );
        });

        // --- settings (spec 3.2, 3.6) ---
        ui.add_space(8.0);
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Cài đặt").size(17.0).strong());
            ui.add_space(6.0);

            ui.label(RichText::new("Từ mới mỗi ngày").size(13.0));
            ui.horizontal(|ui| {
                for goal in [5u32, 10, 20] {
                    let on = ctx.progress.daily_goal == goal;
                    if ui.selectable_label(on, format!("{goal}")).clicked() {
                        ctx.progress.daily_goal = goal;
                    }
                }
            });

            ui.add_space(8.0);
            // Spec 3.2: desired retention, adjustable between 0,8 and 0,95.
            ui.label(RichText::new("Tỷ lệ nhớ mục tiêu").size(13.0));
            ui.add(
                egui::Slider::new(&mut ctx.progress.retention, 0.8..=0.95)
                    .fixed_decimals(2)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
            );
            ui.label(
                RichText::new("Cao hơn = nhớ chắc hơn nhưng phải ôn dày hơn.")
                    .size(11.5)
                    .color(ui::MUTED),
            );

            // Spec 2.3, rule 3: the Skip Band.
            if ctx.progress.placement_done {
                ui.add_space(8.0);
                ui.label(RichText::new("Bỏ qua một dải rank").size(13.0));
                ui.horizontal_wrapped(|ui| {
                    for jump in [1_000u32, 3_000] {
                        if ui.button(format!("+{}", ui::thousands(jump))).clicked() {
                            ctx.progress.frontier =
                                (ctx.progress.frontier + jump).min(ctx.dict.learn_count());
                            ctx.say("Đã nhảy lên dải cao hơn. Dải bỏ qua vẫn được quét nhanh.");
                        }
                    }
                    if ui.button("Đặt lại").clicked() {
                        ctx.progress.frontier = ctx.progress.assumed_below + 1;
                    }
                });
            }
        });

        // --- credits (spec 4.3) ---
        ui.add_space(8.0);
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Nguồn dữ liệu").size(17.0).strong());
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Từ điển: {} mục từ · {} nghĩa · {} đơn vị học",
                    ui::thousands(ctx.dict.word_count()),
                    ui::thousands(ctx.dict.sense_count()),
                    ui::thousands(ctx.dict.learn_count())
                ))
                .size(12.5)
                .color(ui::MUTED),
            );
            ui.add_space(4.0);
            for (what, source, license) in [
                (
                    "Mục từ và nghĩa",
                    "minhqnd/dictionary (Wiktionary, TVTD)",
                    "CC BY-SA",
                ),
                (
                    "Tần suất từ",
                    "hermitdave/FrequencyWords (OpenSubtitles)",
                    "CC BY-SA",
                ),
                ("Lịch ôn tập", "FSRS-5", "MIT"),
            ] {
                ui.label(
                    RichText::new(format!("• {what}: {source} — {license}"))
                        .size(11.5)
                        .color(ui::MUTED),
                );
            }
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Toàn bộ dữ liệu nằm trong máy: tra từ, học và ôn đều chạy khi không có mạng.",
                )
                .size(11.5)
                .color(ui::MUTED),
            );
        });

        // --- reset ---
        ui.add_space(8.0);
        if state.confirm_reset {
            ui::card(ui, Some(ui::BAD), |ui| {
                ui.label(RichText::new("Xoá toàn bộ tiến độ?").strong());
                ui.label(
                    RichText::new("Không thể hoàn tác.")
                        .size(12.0)
                        .color(ui::MUTED),
                );
                ui.add_space(6.0);
                ui.columns(2, |c| {
                    if c[0].button("Huỷ").clicked() {
                        state.confirm_reset = false;
                    }
                    if c[1].button(RichText::new("Xoá").color(ui::BAD)).clicked() {
                        *ctx.progress = Progress::default();
                        ctx.progress.roll_to(ctx.day);
                        state.confirm_reset = false;
                        ctx.say("Đã xoá tiến độ.");
                    }
                });
            });
        } else if ui
            .button(RichText::new("Xoá tiến độ").size(12.0).color(ui::MUTED))
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
            quit = ui.small_button("✕").clicked();
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_height(8.0)
                    .fill(ui::ACCENT),
            );
        });
        ui.label(
            RichText::new(format!("câu {} / ~{}", asked + 1, MAX_ITEMS))
                .size(11.5)
                .color(ui::MUTED),
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
                "⚠ Bạn đang chọn nghĩa cho cả những từ không có thật (trên {:.0}%). \
                 Hãy bấm “Không biết” nếu không chắc.",
                FALSE_ALARM_LIMIT * 100.0
            ))
            .size(12.0)
            .color(ui::WARN),
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
        ui::card(ui, Some(ui::ACCENT), |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(&prompt).size(28.0).strong());
                ui.add_space(4.0);
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Từ này nghĩa là gì?")
                .size(13.0)
                .color(ui::MUTED),
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
        if ui::wide_button(ui, "Không biết", ui::MUTED).clicked() {
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
            ui.label(RichText::new("Kết quả").size(20.0).strong());
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("#{}", ui::thousands(verdict.frontier)))
                    .size(38.0)
                    .strong()
                    .color(ui::ACCENT),
            );
            ui.label(
                RichText::new("đường chân trời từ vựng")
                    .size(12.5)
                    .color(ui::MUTED),
            );
        });
        ui.add_space(12.0);

        ui::card(ui, None, |ui| {
            egui::Grid::new("verdict")
                .num_columns(2)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Số câu").size(13.0).color(ui::MUTED));
                    ui.label(RichText::new(verdict.asked.to_string()).size(13.0));
                    ui.end_row();
                    ui.label(RichText::new("Trả lời đúng").size(13.0).color(ui::MUTED));
                    ui.label(RichText::new(format!("{:.0}%", verdict.raw_rate * 100.0)).size(13.0));
                    ui.end_row();
                    ui.label(
                        RichText::new("Sau hiệu chỉnh đoán mò")
                            .size(13.0)
                            .color(ui::MUTED),
                    );
                    ui.label(
                        RichText::new(format!("{:.0}%", verdict.corrected_rate * 100.0)).size(13.0),
                    );
                    ui.end_row();
                    ui.label(
                        RichText::new("Chọn nghĩa cho từ giả")
                            .size(13.0)
                            .color(ui::MUTED),
                    );
                    ui.label(
                        RichText::new(format!("{:.0}%", verdict.false_alarm * 100.0))
                            .size(13.0)
                            .color(if verdict.false_alarm > FALSE_ALARM_LIMIT {
                                ui::WARN
                            } else {
                                ui::MUTED
                            }),
                    );
                    ui.end_row();
                });
        });

        // Spec 2.2's output: the estimated share known, block by block.
        ui.add_space(8.0);
        ui::section(ui, "Ước lượng theo khối", |ui| {
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
                        .color(ui::MUTED),
                    );
                    ui.add(
                        egui::ProgressBar::new(share)
                            .desired_height(8.0)
                            .fill(ui::C_ASSUMED)
                            .text(RichText::new(format!("{:.0}%", share * 100.0)).size(11.0)),
                    );
                });
            }
        });

        ui.add_space(14.0);
        if ui::wide_button(ui, "Bắt đầu học", ui::ACCENT).clicked() {
            state.result = None;
            *ctx.goto = Some(crate::app::Tab::Study);
        }
        ui.label(
            RichText::new(
                "Những từ dưới mốc này được coi là đã biết nhưng chưa kiểm chứng — \
                 dùng Quét nhanh để dò lỗ hổng.",
            )
            .size(11.5)
            .color(ui::MUTED),
        );
        ui.add_space(16.0);
    });
}
