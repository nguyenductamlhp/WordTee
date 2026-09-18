//! Học — the daily session (spec 3.6) and Quick Scan (spec 2.2).
//!
//! The session order is the spec's: overdue reviews hardest-first, then new
//! items from Smart Feeding, then at most two spot-checks on things marked
//! known. Grades are never asked for — spec 3.2 derives them from how the
//! exercise went, which is what [`Outcome`] carries.

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::dict::SenseId;
use crate::progress::{BACKLOG_FACTOR, QUICK_SCAN_DAILY, Source, State};
use crate::srs::{Grade, Outcome};
use crate::study::{self, Exercise, Session, Task};
use crate::ui;

/// Answering slower than this counts as hesitation, which spec 3.2 grades as
/// Hard rather than Good.
const SLOW_SECONDS: f64 = 12.0;

/// The question on screen and how it is going.
struct Active {
    task: Task,
    exercise: Exercise,
    typed: String,
    picked: Option<usize>,
    /// The user asked to see the answer, or took their time.
    hesitated: bool,
    /// `Context::input(|i| i.time)` when the question appeared.
    started: f64,
    /// Set once graded, so the card can show the verdict before moving on.
    verdict: Option<bool>,
}

/// `(due, new, checks)`, the three numbers the session badge shows.
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
    /// Counters for the end-of-session summary.
    answered: u32,
    right: u32,
    /// The frontier check below runs once per session, not once per frame.
    frontier_checked: bool,
    /// Cached answer for [`Self::pending`], and the revision it was computed
    /// at. The top bar asks for this on every frame of every tab, and building
    /// a session is not free — Smart Feeding scores 500 candidates, each with
    /// a word-family lookup.
    pending: std::cell::Cell<Option<(u64, Pending)>>,
}

impl StudyState {
    /// Dropped when the day turns over.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Starts today's session, as the button on this screen does.
    pub fn begin_session(
        &mut self,
        dict: &crate::dict::Dict,
        progress: &crate::progress::Progress,
        day: crate::progress::Day,
    ) {
        self.session = Some(Session::build(dict, progress, day));
        self.mode = Mode::Session;
        self.active = None;
    }

    /// Starts a Quick Scan run over `items`.
    pub fn begin_scan(&mut self, items: Vec<SenseId>) {
        self.scan = items;
        self.mode = Mode::Scan;
    }

    /// `(due, new, checks)` waiting right now — the badge in the top bar.
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
        let counts = Session::build(dict, progress, day).counts();
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
// the home card
// -------------------------------------------------------------------------

fn home(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    // Cached against the progress revision: building a session scores 500
    // Smart Feeding candidates, which is not something to redo every frame.
    let (due, new, checks) = state.pending(ctx.dict, ctx.progress, ctx.day);

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!("🔥 {}", ctx.progress.streak))
                    .size(30.0)
                    .strong(),
            );
            ui.label(
                RichText::new(if ctx.progress.streak == 0 {
                    "Học hôm nay để bắt đầu chuỗi ngày".to_owned()
                } else {
                    format!("ngày liên tiếp · kỷ lục {}", ctx.progress.best_streak)
                })
                .size(12.5)
                .color(ui::MUTED),
            );
        });
        ui.add_space(10.0);

        if !ctx.progress.placement_done {
            ui::card(ui, Some(ui::ACCENT), |ui| {
                ui.label(RichText::new("Chưa biết bạn đang ở đâu").strong());
                ui.label(
                    RichText::new(
                        "Làm bài test đầu vào (khoảng 15–25 câu) để hệ thống tìm \
                         “đường chân trời từ vựng” và bắt đầu đẩy từ đúng mức.",
                    )
                    .size(13.0)
                    .color(ui::MUTED),
                );
                ui.add_space(6.0);
                if ui::wide_button(ui, "Làm bài test đầu vào", ui::ACCENT).clicked() {
                    *ctx.goto = Some(crate::app::Tab::Profile);
                }
            });
            ui.add_space(8.0);
        }

        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Phiên hôm nay").size(17.0).strong());
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui::chip(ui, &format!("{due} thẻ ôn"), ui::C_LEARNING);
                ui::chip(ui, &format!("{new} từ mới"), ui::ACCENT);
                if checks > 0 {
                    ui::chip(ui, &format!("{checks} kiểm tra lại"), ui::MUTED);
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "Mục tiêu {} từ mới/ngày · đã học {} · đã ôn {}",
                    ctx.progress.daily_goal, ctx.progress.new_today, ctx.progress.reviews_today
                ))
                .size(12.0)
                .color(ui::MUTED),
            );

            // Spec 3.6: the debt notice when reviews have piled up.
            if ctx.progress.backlog_is_heavy(ctx.day) {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "⚠ Thẻ ôn đang dồn quá {BACKLOG_FACTOR} lần mục tiêu ngày. \
                         Tạm ngừng nạp từ mới — hãy trả nợ thẻ ôn trong vài ngày tới."
                    ))
                    .size(12.5)
                    .color(ui::WARN),
                );
            }
            ui.add_space(8.0);
            let enabled = due + new + checks > 0;
            ui.add_enabled_ui(enabled, |ui| {
                if ui::wide_button(ui, "Bắt đầu học", ui::ACCENT).clicked() {
                    state.session = Some(Session::build(ctx.dict, ctx.progress, ctx.day));
                    state.mode = Mode::Session;
                    state.active = None;
                    state.answered = 0;
                    state.right = 0;
                    state.frontier_checked = false;
                }
            });
            if !enabled {
                ui.label(
                    RichText::new("Xong hết rồi. Quay lại vào ngày mai, hoặc quét nhanh bên dưới.")
                        .size(12.5)
                        .color(ui::MUTED),
                );
            }
        });

        // Spec 2.2's Gap Filling.
        ui.add_space(8.0);
        let scanned = ctx.progress.scanned_today;
        let scan_left = QUICK_SCAN_DAILY.saturating_sub(scanned);
        ui::card(ui, None, |ui| {
            ui.label(RichText::new("Quét nhanh").size(16.0).strong());
            ui.label(
                RichText::new(
                    "Những từ hệ thống đoán là bạn đã biết. Xác nhận nhanh để \
                     tìm ra các lỗ hổng từ đời thường.",
                )
                .size(12.5)
                .color(ui::MUTED),
            );
            ui.add_space(6.0);
            ui.add_enabled_ui(scan_left > 0 && ctx.progress.frontier > 1, |ui| {
                if ui::wide_button(ui, &format!("Quét nhanh ({scan_left} thẻ)"), ui::WARN).clicked()
                {
                    state.scan =
                        study::quick_scan(ctx.dict, ctx.progress, ctx.rng, scan_left as usize);
                    state.mode = Mode::Scan;
                }
            });
            if ctx.progress.frontier <= 1 {
                ui.label(
                    RichText::new("Cần làm bài test đầu vào trước.")
                        .size(12.0)
                        .color(ui::MUTED),
                );
            }
        });
        ui.add_space(12.0);
    });
}

// -------------------------------------------------------------------------
// the session
// -------------------------------------------------------------------------

fn session_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    let Some(session) = &mut state.session else {
        state.mode = Mode::Idle;
        return;
    };
    let Some(task) = session.current() else {
        return summary(ui, ctx, state);
    };
    let (done, total) = (session.done(), session.total());

    egui::Panel::top("session-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.small_button("✕").clicked() {
                state.mode = Mode::Idle;
                state.session = None;
                state.active = None;
            }
            ui.add(
                egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                    .desired_height(8.0)
                    .fill(ui::ACCENT),
            );
        });
        ui.label(
            RichText::new(format!("{done}/{total}"))
                .size(11.5)
                .color(ui::MUTED),
        );
        ui.add_space(4.0);
    });

    // Build the question for this task the first time we see it.
    if state.active.as_ref().is_none_or(|a| a.task != task) {
        let sense = ctx.dict.sense(task.sense());
        let level = match task {
            // Spec 3.3: a brand-new item is read, not tested.
            Task::New(_) => 0,
            // Spec 3.5: checks are level-2 exercises.
            Task::Verify(_) => 2,
            Task::Review(id) => ctx.progress.card(id).map_or(1, |c| c.level),
        };
        state.active = Some(Active {
            task,
            exercise: study::exercise(ctx.dict, ctx.rng, &sense, level, ctx.shown),
            typed: String::new(),
            picked: None,
            hesitated: false,
            started: ctx.now,
            verdict: None,
        });
    }
    let sense = ctx.dict.sense(task.sense());
    let word = ctx.dict.word(sense.word);

    // The question is drawn with only `state.active` borrowed; grading needs
    // `state` as a whole, so it happens after that borrow has ended.
    let mut answer: Option<bool> = None;
    let Some(active) = &mut state.active else {
        return;
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(6.0);
        if let Task::Verify(_) = task {
            ui::chip(ui, "Kiểm tra lại từ bạn đánh dấu đã biết", ui::MUTED);
            ui.add_space(4.0);
        }

        answer = active.verdict;
        match &active.exercise {
            // Spec 3.3 level 1, first sight: show the whole card.
            Exercise::Study => {
                ui::card(ui, Some(ui::ACCENT), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(word.text).size(26.0).strong());
                        ui::speak_buttons(ui, word.text);
                    });
                    if !word.ipa.is_empty() {
                        ui.label(RichText::new(word.ipa).size(14.0).color(ui::ACCENT));
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui::pos_chip(ui, sense.pos);
                        ui::band_chip(ui, sense.band(), sense.rank);
                    });
                    ui.add_space(4.0);
                    ui.label(RichText::new(sense.def).size(16.0));
                    if !sense.example.is_empty() {
                        ui.label(
                            RichText::new(sense.example)
                                .size(13.5)
                                .italics()
                                .color(ui::MUTED),
                        );
                        ctx.shown.mark(sense.id);
                    }
                });
                ui.add_space(10.0);
                ui.columns(2, |c| {
                    if c[0]
                        .add_sized(
                            [c[0].available_width(), 42.0],
                            egui::Button::new("Đã biết rồi"),
                        )
                        .clicked()
                    {
                        ctx.progress
                            .set_state(sense.id, State::Known, Source::Manual, ctx.day);
                        answer = Some(true);
                    }
                    let learn =
                        egui::Button::new(RichText::new("Học từ này").color(ui::ACCENT).strong());
                    if c[1]
                        .add_sized([c[1].available_width(), 42.0], learn)
                        .clicked()
                    {
                        ctx.progress
                            .start_learning(sense.id, Source::Study, ctx.day);
                        answer = Some(true);
                    }
                });
            }
            Exercise::Meaning(choice) => {
                answer = question(
                    ui,
                    ctx,
                    active,
                    &format!("“{}” nghĩa là gì?", word.text),
                    Some(choice.clone()),
                    None,
                );
            }
            Exercise::PickWord(choice) => {
                answer = question(
                    ui,
                    ctx,
                    active,
                    "Từ nào mang nghĩa này?",
                    Some(choice.clone()),
                    None,
                );
            }
            Exercise::Fill(gap) => {
                let gap = gap.clone();
                answer = question(ui, ctx, active, "Điền từ còn thiếu:", None, Some(*gap));
            }
            Exercise::Spell => {
                answer = spell_question(ui, ctx, active, &sense);
            }
        }

        if let Some(correct) = answer {
            active.verdict = Some(correct);
        }
        ui.add_space(20.0);
    });

    if let Some(correct) = answer {
        grade_and_advance(ctx, state, correct);
    }
}

/// Draws a multiple-choice or gap-fill question and reports the result.
fn question(
    ui: &mut egui::Ui,
    ctx: &mut Ctx,
    active: &mut Active,
    prompt: &str,
    choice: Option<crate::quiz::Choice>,
    gap: Option<crate::quiz::Cloze>,
) -> Option<bool> {
    if let Some(gap) = gap {
        ui::card(ui, None, |ui| {
            ui.label(RichText::new(prompt).size(13.0).color(ui::MUTED));
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("{}{}{}", gap.before, gap.blank(), gap.after)).size(17.0),
            );
        });
        ui.add_space(8.0);
        ui.add_sized(
            [ui.available_width(), 38.0],
            egui::TextEdit::singleline(&mut active.typed)
                .hint_text(format!("bắt đầu bằng “{}”", gap.hint)),
        );
        ui.add_space(6.0);
        if ui::wide_button(ui, "Trả lời", ui::ACCENT).clicked() {
            active.hesitated |= ctx.now - active.started > SLOW_SECONDS;
            return Some(gap.accepts(&active.typed));
        }
        return None;
    }

    let choice = choice?;
    ui::card(ui, None, |ui| {
        ui.label(RichText::new(prompt).size(17.0).strong());
        if !choice.prompt.is_empty() && !prompt.contains(&choice.prompt) {
            ui.add_space(3.0);
            ui.label(RichText::new(&choice.prompt).size(14.5).color(ui::MUTED));
        }
    });
    ui.add_space(8.0);
    for (i, option) in choice.options.iter().enumerate() {
        let color = match active.picked {
            Some(_) if i == choice.answer => ui::GOOD,
            Some(p) if p == i => ui::BAD,
            Some(_) => ui::MUTED,
            None => ui.visuals().text_color(),
        };
        if ui::wide_button(ui, option, color).clicked() && active.picked.is_none() {
            active.picked = Some(i);
            active.hesitated |= ctx.now - active.started > SLOW_SECONDS;
        }
        ui.add_space(3.0);
    }
    let picked = active.picked?;
    ui.add_space(6.0);
    if ui::wide_button(ui, "Tiếp tục", ui::ACCENT).clicked() {
        return Some(picked == choice.answer);
    }
    None
}

/// Spec 3.3 level 3: produce the word, spelled correctly.
fn spell_question(
    ui: &mut egui::Ui,
    ctx: &mut Ctx,
    active: &mut Active,
    sense: &crate::dict::Sense,
) -> Option<bool> {
    ui::card(ui, None, |ui| {
        ui.label(
            RichText::new("Viết từ mang nghĩa này:")
                .size(13.0)
                .color(ui::MUTED),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(sense.def).size(17.0));
        ui::pos_chip(ui, sense.pos);
    });
    ui.add_space(8.0);
    ui.add_sized(
        [ui.available_width(), 38.0],
        egui::TextEdit::singleline(&mut active.typed).hint_text("gõ từ tiếng Anh…"),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.small_button("Gợi ý chữ đầu").clicked() {
            active.hesitated = true;
        }
        if active.hesitated {
            let word = ctx.dict.word(sense.word).text;
            ui.label(
                RichText::new(format!(
                    "bắt đầu bằng “{}”",
                    word.chars().next().unwrap_or('?')
                ))
                .size(12.5)
                .color(ui::WARN),
            );
        }
    });
    ui.add_space(6.0);
    if ui::wide_button(ui, "Trả lời", ui::ACCENT).clicked() {
        active.hesitated |= ctx.now - active.started > SLOW_SECONDS;
        return Some(study::spelling_accepts(ctx.dict, sense, &active.typed));
    }
    None
}

/// Applies the answer to the card and moves the session on.
fn grade_and_advance(ctx: &mut Ctx, state: &mut StudyState, correct: bool) {
    let Some(active) = &state.active else { return };
    let (task, level, hesitated) = (active.task, active.exercise.level(), active.hesitated);
    state.answered += 1;
    state.right += u32::from(correct);

    match task {
        // The "read the card" step is not graded; the button already set the
        // state.
        Task::New(_) => {}
        Task::Verify(id) => ctx.progress.verify(id, correct, ctx.day),
        Task::Review(id) => {
            let outcome = Outcome {
                correct,
                hesitated,
                level,
            };
            let (grade, _) = ctx.progress.answer(id, outcome, ctx.day);
            // Spec 3.3: a missed item comes round again this session — unless
            // spec 3.6 has just flagged it a leech, which is set aside for
            // three days rather than drilled until it sticks.
            let leech = ctx.progress.card(id).is_some_and(|c| c.is_leech());
            if grade == Grade::Again && !leech {
                if let Some(session) = &mut state.session {
                    session.requeue();
                    state.active = None;
                }
                return;
            }
            if leech {
                ctx.say("Từ này đang khó nhớ — tạm hoãn 3 ngày rồi xem lại thẻ đầy đủ.");
            }
        }
    }
    if let Some(session) = &mut state.session {
        session.advance();
    }
    state.active = None;
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
                    "Đã mở dải từ mới: bắt đầu từ #{}.",
                    ui::thousands(ctx.progress.frontier)
                ));
            }
        }
    }
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new("✅").size(44.0));
        ui.label(RichText::new("Xong phiên hôm nay").size(20.0).strong());
        ui.add_space(6.0);
        let rate = (state.right * 100).checked_div(state.answered).unwrap_or(0);
        ui.label(
            RichText::new(format!("{} câu · đúng {rate}%", state.answered))
                .size(13.0)
                .color(ui::MUTED),
        );
        ui.label(RichText::new(format!("🔥 chuỗi {} ngày", ctx.progress.streak)).size(14.0));
        ui.add_space(14.0);
        if ui.button("Về trang Học").clicked() {
            state.mode = Mode::Idle;
            state.session = None;
            state.active = None;
        }
    });
}

// -------------------------------------------------------------------------
// Quick Scan (spec 2.2)
// -------------------------------------------------------------------------

fn scan_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut StudyState) {
    egui::Panel::top("scan-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.small_button("✕").clicked() {
                state.mode = Mode::Idle;
                state.scan.clear();
            }
            ui.label(RichText::new("Quét nhanh").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("còn {}", state.scan.len()))
                        .size(12.5)
                        .color(ui::MUTED),
                );
            });
        });
        ui.add_space(4.0);
    });

    let Some(&sense) = state.scan.first() else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Đã quét xong.").size(17.0).strong());
            ui.add_space(8.0);
            if ui.button("Về trang Học").clicked() {
                state.mode = Mode::Idle;
            }
        });
        ctx.progress.mark_active(ctx.day);
        return;
    };

    ui.add_space(10.0);
    ui.label(
        RichText::new("Bạn có biết từ này không?")
            .size(14.0)
            .color(ui::MUTED),
    );
    ui.add_space(6.0);
    if super::map::scan_card(ui, ctx, sense) {
        state.scan.remove(0);
    }
}
