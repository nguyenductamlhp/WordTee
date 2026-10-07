//! You — where the user stands, the placement test (spec 2.2), the settings
//! spec 3.2 and 3.6 expose, sync, and the data credits spec 4.3 requires.
//!
//! The tab's own page is short: the level, the progress, and a list of
//! settings. Anything that takes more than a switch — reminder rates,
//! challenge types, retention, the credits — opens a page of its own.

use eframe::egui::{self, Align, Layout, RichText, Stroke};

use crate::app::Ctx;
use crate::google::Status;
use crate::placement::{FALSE_ALARM_LIMIT, MAX_ITEMS, Placement, Verdict};
use crate::progress::{self, Accent, Casing, LEVELS, Reminders, Stamp, State, Theme};
use crate::ui::{self, Icon, Kind, theme};

/// The pages behind the rows of the main one.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    #[default]
    Main,
    Sync,
    Levels,
    Reminders,
    Challenges,
    Retention,
    JumpAhead,
    About,
}

#[derive(Default)]
pub struct ProfileState {
    test: Option<Placement>,
    /// Shown after the test ends, until dismissed.
    result: Option<Verdict>,
    page: Page,
    confirm_reset: bool,
}

impl ProfileState {
    /// Starts the placement test, as the button on this screen does.
    pub fn begin_test(&mut self, dict: &crate::dict::Dict) {
        self.test = Some(Placement::new(dict));
    }

    /// Is a placement test running? The tab bar hides while one is.
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
    match state.page {
        Page::Main => main_page(ui, ctx, state),
        page => sub_page(ui, ctx, state, page),
    }
}

// -------------------------------------------------------------------------
// the main page
// -------------------------------------------------------------------------

fn main_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    ui::screen_header(ui, "You", |_| {});
    ui::page(ui, "you", |ui| {
        sync_summary(ui, ctx, state);
        level_card(ui, ctx, state);
        progress_card(ui, ctx);
        settings(ui, ctx, state);
        erase(ui, ctx, state);
    });
}

/// The sync status as one row, opening the Sync page.
fn sync_summary(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    let p = ui::palette(ui);
    let (title, detail, problem) = match ctx.google.status() {
        Status::Unavailable => (
            "Sync is not set up".to_owned(),
            "Google sign-in is not configured in this build".to_owned(),
            false,
        ),
        Status::SignedOut { error } => match error {
            Some(error) => ("Sign in to sync".to_owned(), error, true),
            None => (
                "Sign in to sync".to_owned(),
                "Keep your progress on all your devices".to_owned(),
                false,
            ),
        },
        Status::SigningIn => (
            "Signing in\u{2026}".to_owned(),
            "Finish in the sign-in window".to_owned(),
            false,
        ),
        Status::SignedIn {
            name,
            email,
            syncing,
            synced,
            error,
            ..
        } => {
            let title = if name.is_empty() { email } else { name };
            match (syncing, error) {
                (true, _) => (title, "Syncing\u{2026}".to_owned(), false),
                (false, Some(error)) => (title, error, true),
                (false, None) => (title, synced.map_or("Not synced yet".into(), ago), false),
            }
        }
    };
    let frame = ui::card_frame(ui).inner_margin(egui::Margin {
        left: 14,
        right: 10,
        top: 12,
        bottom: 12,
    });
    let (tapped, ()) = ui::tappable_card(ui, "sync", frame, |ui| {
        ui.horizontal(|ui| {
            let (avatar, _) = ui.allocate_exact_size(egui::Vec2::splat(44.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(avatar.center(), 22.0, p.primary_soft);
            ui::paint_icon(
                ui.painter(),
                egui::Rect::from_center_size(avatar.center(), egui::Vec2::splat(22.0)),
                Icon::Sync,
                p.primary_ink,
                p.primary_soft,
            );
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(theme::body_strong(&title));
                let color = if problem { p.wrong_ink } else { p.ink2 };
                ui.add(egui::Label::new(theme::caption(&detail).color(color)).truncate());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (chevron, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::hover());
                ui::paint_icon(ui.painter(), chevron, Icon::ChevronRight, p.ink3, p.surface);
            });
        });
    });
    if tapped {
        state.page = Page::Sync;
    }
}

/// Where the user's level sits, on a track through the five named levels.
fn level_card(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    let p = ui::palette(ui);
    let placed = ctx.progress.placement_done;
    let chosen = ctx.progress.level();
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let (over, big, note) = if placed {
            (
                "Vocabulary frontier",
                format!("#{}", ui::thousands(ctx.progress.assumed_below)),
                "from the placement test".to_owned(),
            )
        } else if let Some(level) = chosen {
            (
                "Starting level",
                level.label.to_owned(),
                format!("learning from #{}", ui::thousands(ctx.progress.frontier)),
            )
        } else {
            (
                "Your level",
                "Not set yet".to_owned(),
                "take the test, or pick one".to_owned(),
            )
        };
        ui.label(theme::label(over).color(p.ink2));
        ui.horizontal(|ui| {
            ui.label(RichText::new(big).size(28.0).family(theme::semibold()));
            ui.label(theme::caption(note).color(p.ink2));
        });
        ui.add_space(10.0);
        level_track(ui, ctx);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let width = ((ui.available_width() - 8.0) / 2.0).floor();
            let size = egui::vec2(width, ui::TOUCH);
            let (kind, label) = if placed {
                (Kind::Secondary, "Retake test")
            } else {
                (Kind::Primary, "Take the test")
            };
            if ui::button(ui, kind, None, label, size).clicked() {
                state.begin_test(ctx.dict);
            }
            if ui::button(ui, Kind::Secondary, None, "Choose a level", size).clicked() {
                state.page = Page::Levels;
            }
        });
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                theme::caption(
                    "15\u{2013}25 questions, with invented words mixed in to catch guessing.",
                )
                .color(p.ink3),
            )
            .wrap(),
        );
    });
}

/// The five levels as five stretches of a bar, with the frontier marked.
fn level_track(ui: &mut egui::Ui, ctx: &Ctx) {
    let p = ui::palette(ui);
    let end = ctx
        .dict
        .learn_count()
        .max(LEVELS[LEVELS.len() - 1].from + 1);
    let frontier = ctx.progress.frontier.max(1);
    let set = ctx.progress.has_level();
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
    let gap = 3.0;
    let each = (rect.width() - gap * (LEVELS.len() - 1) as f32) / LEVELS.len() as f32;
    let mut marker = None;
    let mut current = None;
    for (i, level) in LEVELS.iter().enumerate() {
        let to = LEVELS.get(i + 1).map_or(end, |next| next.from);
        let left = rect.left() + i as f32 * (each + gap);
        let segment =
            egui::Rect::from_min_size(egui::pos2(left, rect.top()), egui::vec2(each, 10.0));
        let reached = ((frontier.saturating_sub(level.from)) as f32 / (to - level.from) as f32)
            .clamp(0.0, 1.0);
        let painter = ui.painter();
        painter.rect_filled(segment, 3.0, p.unexplored);
        if set && reached > 0.0 {
            let done = egui::Rect::from_min_size(segment.min, egui::vec2(each * reached, 10.0));
            if ctx.progress.placement_done {
                painter.rect_filled(done, 3.0, p.assumed);
                ui::hatch(painter, done, p.assumed_hatch);
            } else {
                painter.rect_filled(done, 3.0, p.primary_soft);
            }
        }
        if frontier >= level.from && frontier < to {
            marker = Some(segment.left() + each * reached);
            current = Some(i);
        }
    }
    if set && let Some(x) = marker {
        let bar =
            egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(4.0, 20.0));
        ui.painter().rect_filled(bar.expand(2.0), 3.0, p.surface);
        ui.painter().rect_filled(bar, 2.0, p.primary_ink);
    }
    // Each stretch is labelled with the rank it starts at; the names are
    // too long to fit five abreast on a phone, so only the current one is
    // spelled out, under its stretch.
    let (labels, _) = ui.allocate_exact_size(egui::vec2(rect.width(), 18.0), egui::Sense::hover());
    // Where the last label ended, so a long name can take the next one's
    // place rather than run into it.
    let mut free_from = labels.left();
    for (i, level) in LEVELS.iter().enumerate() {
        let here = set && current == Some(i);
        let text = if here {
            level.label.to_owned()
        } else {
            format!("#{}", short_rank(level.from))
        };
        let x = labels.left() + i as f32 * (each + gap);
        if x < free_from {
            continue;
        }
        let drawn = ui.painter().text(
            egui::pos2(x, labels.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::new(12.0, theme::semibold()),
            if here { p.primary_ink } else { p.ink3 },
        );
        free_from = drawn.right() + 8.0;
    }
}

/// 1, 1k, 3k, 10k.
fn short_rank(rank: u32) -> String {
    if rank < 1_000 {
        rank.to_string()
    } else {
        format!("{}k", rank / 1_000)
    }
}

/// What has been learned: the bar, and a tile per state.
fn progress_card(ui: &mut egui::Ui, ctx: &mut Ctx) {
    let p = ui::palette(ui);
    let counts = ctx
        .progress
        .tally(ctx.dict.learn_span(1..ctx.dict.learn_count() + 1));
    ui::card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(theme::heading("Progress"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    theme::caption(format!("{} items", ui::thousands(ctx.dict.learn_count())))
                        .color(p.ink2),
                );
            });
        });
        ui.add_space(4.0);
        ui::stacked_bar(ui, counts, 10.0);
        ui.add_space(8.0);
        let width = ((ui.available_width() - 16.0) / 3.0).floor();
        let tiles = [
            (State::Mastered, "Mastered"),
            (State::Known, "Known"),
            (State::AssumedKnown, "Probably"),
            (State::Review, "In review"),
            (State::Learning, "Learning"),
            (State::Unexplored, "New"),
        ];
        for row in tiles.chunks(3) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for &(kind, label) in row {
                    tally_tile(ui, kind, label, counts[kind as usize], width);
                }
            });
        }
    });
}

/// One state's count, with its colour as a swatch.
fn tally_tile(ui: &mut egui::Ui, kind: State, label: &str, value: u32, width: f32) {
    let p = ui::palette(ui);
    egui::Frame::new()
        .fill(p.page)
        .corner_radius(12)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    let (swatch, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(9.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(swatch, 3.0, ui::state_color(ui, kind));
                    if kind == State::AssumedKnown {
                        ui::hatch(ui.painter(), swatch, p.assumed_hatch);
                    }
                    if kind == State::Unexplored {
                        ui.painter().rect_stroke(
                            swatch,
                            3.0,
                            Stroke::new(1.0, p.unexplored_dot),
                            egui::StrokeKind::Inside,
                        );
                    }
                    ui.label(
                        RichText::new(label)
                            .size(12.5)
                            .family(theme::semibold())
                            .color(p.ink2),
                    );
                });
                ui.label(
                    RichText::new(ui::thousands(value))
                        .size(19.0)
                        .family(theme::semibold()),
                );
            })
        });
}

fn settings(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    ui::settings_group(ui, "Learning", |ui| {
        let accent = ctx.progress.accent;
        let labels: Vec<&str> = Accent::ALL.iter().map(|a| a.label()).collect();
        let at = Accent::ALL.iter().position(|a| *a == accent).unwrap_or(1);
        if let Some(i) = ui::choice_row(ui, Icon::Speaker, "Accent", &labels, at) {
            ctx.progress.change_settings(|p| p.accent = Accent::ALL[i]);
        }

        let goals = ["5", "10", "20"];
        let at = goals
            .iter()
            .position(|g| g.parse() == Ok(ctx.progress.daily_goal))
            .unwrap_or(1);
        if let Some(i) = ui::choice_row(ui, Icon::Target, "New words a day", &goals, at) {
            ctx.progress
                .change_settings(|p| p.daily_goal = goals[i].parse().unwrap_or(10));
        }

        if ui::value_row(
            ui,
            Some(Icon::Bell),
            "Reminders",
            ctx.progress.reminders.label(),
        ) {
            state.page = Page::Reminders;
        }

        let mut alerts = ctx.progress.streak_alerts;
        if ui::switch_row(ui, Icon::Ball, "Streak alerts", &mut alerts) {
            ctx.progress.change_settings(|p| p.streak_alerts = alerts);
        }
    });

    ui::settings_group(ui, "Review cards", |ui| {
        let mut examples = ctx.progress.show_examples;
        if ui::switch_row(ui, Icon::Lines, "Word examples", &mut examples) {
            ctx.progress.change_settings(|p| p.show_examples = examples);
        }
        let mut speak = ctx.progress.auto_pronounce;
        if ui::switch_row(ui, Icon::Speaker, "Pronounce on show", &mut speak) {
            ctx.progress.change_settings(|p| p.auto_pronounce = speak);
        }
        let mut hard = ctx.progress.hard_word_alert;
        if ui::switch_row(ui, Icon::Warning, "Hard word alert", &mut hard) {
            ctx.progress.change_settings(|p| p.hard_word_alert = hard);
        }

        let cases: Vec<&str> = Casing::ALL.iter().map(|c| c.label()).collect();
        let at = Casing::ALL
            .iter()
            .position(|c| *c == ctx.progress.casing)
            .unwrap_or(0);
        if let Some(i) = ui::choice_row(ui, Icon::Letters, "Casing", &cases, at) {
            ctx.progress.change_settings(|p| p.casing = Casing::ALL[i]);
        }

        let challenges = ctx.progress.challenges;
        let summary = if challenges.count() == 3 {
            "All 3".to_owned()
        } else {
            [
                (challenges.recognise, "Recognise"),
                (challenges.recall, "Recall"),
                (challenges.produce, "Produce"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, name)| *name)
            .collect::<Vec<_>>()
            .join(", ")
        };
        if ui::value_row(ui, Some(Icon::Check), "Challenge types", &summary) {
            state.page = Page::Challenges;
        }
        let retention = format!("{:.0}%", ctx.progress.retention * 100.0);
        if ui::value_row(ui, Some(Icon::Trend), "Target retention", &retention) {
            state.page = Page::Retention;
        }
    });

    ui::settings_group(ui, "General", |ui| {
        let dark = ctx.progress.theme == Theme::Dark;
        let icon = if dark { Icon::Moon } else { Icon::Sun };
        if let Some(i) = ui::choice_row(ui, icon, "Theme", &["Light", "Dark"], usize::from(dark)) {
            let theme = if i == 1 { Theme::Dark } else { Theme::Light };
            ctx.progress.change_settings(|p| p.theme = theme);
        }
        // Spec 2.3, rule 3: the Skip Band.
        if ctx.progress.has_level() && ui::value_row(ui, Some(Icon::Arrow), "Jump ahead", "") {
            state.page = Page::JumpAhead;
        }
        if ui::value_row(ui, Some(Icon::Info), "About & data sources", "") {
            state.page = Page::About;
        }
    });
}

fn erase(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState) {
    let p = ui::palette(ui);
    ui.add_space(4.0);
    if !state.confirm_reset {
        ui::list(ui, |ui| {
            if ui::danger_row(ui, "Erase progress") {
                state.confirm_reset = true;
            }
        });
        return;
    }
    ui::card_frame(ui)
        .stroke(Stroke::new(1.5, p.wrong_line))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(theme::heading("Erase all progress?"));
            ui.add(
                egui::Label::new(
                    theme::caption(if ctx.google.signed_in() {
                        "This cannot be undone, and sync erases it on your other devices too."
                    } else {
                        "This cannot be undone."
                    })
                    .color(p.ink2),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let width = ((ui.available_width() - 8.0) / 2.0).floor();
                let size = egui::vec2(width, ui::TOUCH);
                if ui::button(ui, Kind::Secondary, None, "Cancel", size).clicked() {
                    state.confirm_reset = false;
                }
                if ui::button(ui, Kind::Danger, None, "Erase", size).clicked() {
                    ctx.progress.erase(ctx.day);
                    state.confirm_reset = false;
                    ctx.say("Progress erased.");
                }
            });
        });
}

// -------------------------------------------------------------------------
// the pages behind the rows
// -------------------------------------------------------------------------

fn sub_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState, page: Page) {
    let title = match page {
        Page::Main => "You",
        Page::Sync => "Sync",
        Page::Levels => "Starting level",
        Page::Reminders => "Reminders",
        Page::Challenges => "Challenge types",
        Page::Retention => "Target retention",
        Page::JumpAhead => "Jump ahead",
        Page::About => "About & data sources",
    };
    if ui::back_header(ui, title) {
        state.page = Page::Main;
        return;
    }
    let p = ui::palette(ui);
    let note = |ui: &mut egui::Ui, text: &str| {
        ui.add(egui::Label::new(theme::caption(text).color(p.ink2)).wrap());
    };
    ui::page(ui, "you-page", |ui| match page {
        Page::Main => {}
        Page::Sync => sync_page(ui, ctx),
        Page::Levels => {
            note(
                ui,
                "Pick where new words should start. Nothing below it is counted as \
                 known \u{2014} the test is the way to find out what is.",
            );
            let current = ctx.progress.level();
            ui::list(ui, |ui| {
                for level in LEVELS {
                    let from = format!("from #{}", ui::thousands(level.from));
                    if ui::check_row(ui, level.label, &from, current == Some(level))
                        && current != Some(level)
                    {
                        ctx.progress.choose_level(level);
                        ctx.say(format!("New words now start at {}.", level.label));
                    }
                }
            });
            note(
                ui,
                &format!(
                    "Words below your level still come up now and then \u{2014} about 1 in {} new words.",
                    100 / crate::study::LOWER_LEVEL_PERCENT
                ),
            );
        }
        Page::Reminders => {
            ui::list(ui, |ui| {
                for rate in Reminders::ALL {
                    if ui::check_row(ui, rate.label(), "", ctx.progress.reminders == rate)
                        && ctx.progress.reminders != rate
                    {
                        ctx.progress.change_settings(|p| p.reminders = rate);
                        ctx.progress.mark_reminded(progress::now_secs());
                    }
                }
            });
            reminder_note(ui, ctx);
            note(
                ui,
                "Nothing is sent when nothing is waiting, and finishing a session \
                 buys a full interval of quiet.",
            );
        }
        Page::Challenges => {
            note(
                ui,
                "Cards climb these three as they stick. Switch one off and a card that \
                 has earned it is asked the level below instead.",
            );
            // Spec 3.3's three levels. The last one on cannot be cleared, or a
            // session would have nothing to ask.
            let mut challenges = ctx.progress.challenges;
            let only_one = challenges.count() == 1;
            ui::list(ui, |ui| {
                for (icon, label, flag) in [
                    (
                        Icon::Check,
                        "Recognise \u{2014} pick the meaning",
                        &mut challenges.recognise,
                    ),
                    (
                        Icon::Lines,
                        "Recall \u{2014} fill the gap",
                        &mut challenges.recall,
                    ),
                    (
                        Icon::Letters,
                        "Produce \u{2014} spell it out",
                        &mut challenges.produce,
                    ),
                ] {
                    let locked = only_one && *flag;
                    ui.add_enabled_ui(!locked, |ui| {
                        ui::switch_row(ui, icon, label, flag);
                    });
                }
            });
            if challenges.count() > 0 && challenges != ctx.progress.challenges {
                ctx.progress.change_settings(|p| p.challenges = challenges);
            }
            if only_one {
                note(ui, "At least one has to stay on.");
            }
        }
        Page::Retention => {
            // Spec 3.2: desired retention, adjustable between 0,8 and 0,95.
            let mut retention = ctx.progress.retention;
            ui::card(ui, |ui| {
                ui.label(
                    RichText::new(format!("{:.0}%", retention * 100.0))
                        .size(34.0)
                        .family(theme::semibold())
                        .color(p.primary_ink),
                );
                ui.label(theme::caption("of reviewed words remembered").color(p.ink2));
                ui.add_space(8.0);
                ui.spacing_mut().slider_width = ui.available_width() - 16.0;
                let slider = egui::Slider::new(&mut retention, 0.8..=0.95)
                    .show_value(false)
                    .step_by(0.01);
                if ui.add(slider).changed() {
                    ctx.progress.change_settings(|p| p.retention = retention);
                }
                ui.horizontal(|ui| {
                    ui.label(theme::caption("80%").color(p.ink3));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(theme::caption("95%").color(p.ink3));
                    });
                });
            });
            note(
                ui,
                "Higher means firmer recall, but more reviews to sit through.",
            );
        }
        Page::JumpAhead => {
            note(
                ui,
                &format!(
                    "New words start at #{}. Skip to a harder band if these are too easy; \
                     the words you skip still come up in Quick scan.",
                    ui::thousands(ctx.progress.frontier)
                ),
            );
            for jump in [1_000u32, 3_000] {
                if ui::secondary_button(ui, &format!("Skip {} words", ui::thousands(jump)))
                    .clicked()
                {
                    ctx.progress
                        .set_frontier((ctx.progress.frontier + jump).min(ctx.dict.learn_count()));
                    ctx.say(
                        "Jumped to a higher band. The skipped range still comes up in Quick scan.",
                    );
                }
            }
            if ui::secondary_button(ui, "Back to my level").clicked() {
                ctx.progress.set_frontier(ctx.progress.assumed_below + 1);
                ctx.say(format!(
                    "New words start at #{} again.",
                    ui::thousands(ctx.progress.frontier)
                ));
            }
        }
        Page::About => about(ui, ctx),
    });
}

/// Says where a reminder can actually be delivered on this platform.
fn reminder_note(ui: &mut egui::Ui, ctx: &mut Ctx) {
    if ctx.progress.reminders == Reminders::Off {
        return;
    }
    let p = ui::palette(ui);
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
    ui.add(egui::Label::new(theme::caption(note).color(p.ink2)).wrap());
    if ui::can_notify()
        && !ui::notifications_allowed()
        && ui::secondary_button(ui, "Allow notifications").clicked()
    {
        ui::request_notifications();
    }
}

/// Spec 4.3's credits.
fn about(ui: &mut egui::Ui, ctx: &mut Ctx) {
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(theme::heading("Dictionary"));
        ui.label(
            theme::caption(format!(
                "{} headwords · {} senses · {} learning items",
                ui::thousands(ctx.dict.word_count()),
                ui::thousands(ctx.dict.sense_count()),
                ui::thousands(ctx.dict.learn_count())
            ))
            .color(p.ink2),
        );
    });
    ui::list(ui, |ui| {
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
            ui::list_row(ui, what, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(theme::body_strong(what));
                ui.add(egui::Label::new(theme::caption(source).color(p.ink2)).wrap());
                ui.label(theme::caption(license).color(p.ink3));
            });
        }
    });
    ui.add(
        egui::Label::new(
            theme::caption(
                "Everything lives on your device: lookup, study and review all work offline. \
                 Signing in to sync adds a copy in your own Google Drive.",
            )
            .color(p.ink2),
        )
        .wrap(),
    );
}

/// Signing in with Google, and how the copy in Drive stands.
fn sync_page(ui: &mut egui::Ui, ctx: &mut Ctx) {
    let now = ctx.now;
    let p = ui::palette(ui);
    let note = |ui: &mut egui::Ui, text: &str| {
        ui.add(egui::Label::new(theme::caption(text).color(p.ink2)).wrap());
    };
    let problem = |ui: &mut egui::Ui, text: &str| {
        ui.add(egui::Label::new(theme::caption(text).color(p.wrong_ink)).wrap());
    };
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        match ctx.google.status() {
            Status::Unavailable => {
                note(ui, "Google sign-in is not set up in this build.");
            }
            Status::SignedOut { error } => {
                note(
                    ui,
                    "Sign in with Google to keep a copy of your progress in your Google Drive, \
                     and pick up where you left off on another device.",
                );
                if let Some(error) = error {
                    problem(ui, &error);
                }
                ui.add_space(4.0);
                if ui::primary_button(ui, "Sign in with Google").clicked() {
                    ctx.google.sign_in(ui.ctx());
                }
            }
            Status::SigningIn => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    note(
                        ui,
                        if cfg!(target_arch = "wasm32") {
                            "Finish signing in in Google\u{2019}s window\u{2026}"
                        } else {
                            "Finish signing in in your browser, then come back here\u{2026}"
                        },
                    );
                });
                if ui::secondary_button(ui, "Cancel").clicked() {
                    ctx.google.cancel();
                }
            }
            Status::SignedIn {
                name,
                email,
                syncing,
                synced,
                error,
                waiting,
            } => {
                if !name.is_empty() {
                    ui.label(theme::heading(name));
                }
                note(ui, &email);
                if syncing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        note(ui, "Syncing\u{2026}");
                    });
                } else if let Some(error) = error {
                    problem(ui, &error);
                } else {
                    let mut line =
                        synced.map_or("Not synced yet.".into(), |s| format!("{}.", ago(s)));
                    if waiting {
                        line += " Tap Sync now to sync again.";
                    }
                    note(ui, &line);
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let width = ((ui.available_width() - 8.0) / 2.0).floor();
                    let size = egui::vec2(width, ui::TOUCH);
                    let sync = ui
                        .add_enabled_ui(!syncing, |ui| {
                            ui::button(ui, Kind::Primary, None, "Sync now", size)
                        })
                        .inner;
                    if sync.clicked() {
                        ctx.google.sync_now(ui.ctx(), ctx.progress, now);
                    }
                    if ui::button(ui, Kind::Secondary, None, "Sign out", size).clicked() {
                        ctx.google.sign_out();
                        ctx.say("Signed out. Your progress stays on this device.");
                    }
                });
            }
        }
    });
    note(
        ui,
        "The copy is one file in your Drive\u{2019}s app data folder, which only WordTee \
         can see. Signing out keeps the progress on this device.",
    );
}

/// "Synced 5 min ago".
fn ago(when: Stamp) -> String {
    let minutes = progress::now().saturating_sub(when) / 60_000;
    match minutes {
        0 => "Synced just now".into(),
        1..=59 => format!("Synced {minutes} min ago"),
        60..=1_439 => format!("Synced {} h ago", minutes / 60),
        _ => format!("Synced {} days ago", minutes / 1_440),
    }
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

    let count = format!("{} / ~{}", asked + 1, MAX_ITEMS);
    if ui::focus_header(ui, fraction, &count) {
        state.test = None;
        return;
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

    let p = ui::palette(ui);
    let mut answered: Option<Option<usize>> = None;
    ui::page(ui, "placement", |ui| {
        // Spec 2.2: warn a user who is guessing at the pseudo-words.
        if warned {
            ui::card_frame(ui)
                .fill(p.streak_soft)
                .stroke(Stroke::NONE)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            theme::caption(format!(
                                "Careful: you are picking meanings for words that do not exist \
                                 (over {:.0}%). Choose \u{201c}I don\u{2019}t know\u{201d} when you \
                                 are not sure.",
                                FALSE_ALARM_LIMIT * 100.0
                            ))
                            .color(p.streak_ink),
                        )
                        .wrap(),
                    );
                });
        }
        ui::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(theme::label("What does this word mean?").color(p.ink2));
                ui.label(theme::display(&prompt));
            });
        });
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for (i, option) in options.iter().enumerate() {
                if ui::answer_option(ui, i, option, ui::Mark::Plain).clicked() {
                    answered = Some(Some(i));
                }
            }
        });
        // Spec 2.2 requires this: not knowing must not have to be a guess.
        if ui::secondary_button(ui, "I don\u{2019}t know").clicked() {
            answered = Some(None);
        }
    });

    if let Some(pick) = answered
        && let Some(test) = state.test.as_mut()
    {
        test.answer(ctx.dict, pick);
    }
}

fn result_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut ProfileState, verdict: Verdict) {
    ui::screen_header(ui, "Your result", |_| {});
    let p = ui::palette(ui);
    ui::page(ui, "placement-result", |ui| {
        ui::card(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(theme::label("Vocabulary frontier").color(p.ink2));
                ui.label(
                    RichText::new(format!("#{}", ui::thousands(verdict.frontier)))
                        .size(40.0)
                        .family(theme::semibold())
                        .color(p.primary_ink),
                );
                ui.label(theme::caption("Words before this are assumed known").color(p.ink2));
            });
        });

        ui::list(ui, |ui| {
            let falsely = verdict.false_alarm > FALSE_ALARM_LIMIT;
            for (label, value, warn) in [
                ("Questions", verdict.asked.to_string(), false),
                (
                    "Answered correctly",
                    format!("{:.0}%", verdict.raw_rate * 100.0),
                    false,
                ),
                (
                    "After guess correction",
                    format!("{:.0}%", verdict.corrected_rate * 100.0),
                    false,
                ),
                (
                    "Knew invented words",
                    format!("{:.0}%", verdict.false_alarm * 100.0),
                    falsely,
                ),
            ] {
                ui::list_row(ui, label, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(theme::body(label));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let color = if warn { p.warn } else { p.ink };
                            ui.label(theme::body_strong(value).color(color));
                        });
                    });
                });
            }
        });

        // Spec 2.2's output: the estimated share known, block by block.
        ui::section_title(ui, "Estimate by block", "");
        ui::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            for block in 0..5u32 {
                let start = block * 1_000;
                let share = verdict.known_share(start);
                ui.horizontal(|ui| {
                    ui.label(
                        theme::caption(format!(
                            "{} \u{2013} {}",
                            ui::thousands(start + 1),
                            ui::thousands(start + 1_000)
                        ))
                        .color(p.ink2),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            theme::caption(format!("{:.0}%", share * 100.0))
                                .family(theme::semibold()),
                        );
                    });
                });
                ui::progress_track(ui, share, 8.0, p.assumed);
                ui.add_space(4.0);
            }
        });

        if ui::primary_button(ui, "Start studying").clicked() {
            state.result = None;
            *ctx.goto = Some(crate::app::Tab::Study);
        }
        ui.add(
            egui::Label::new(
                theme::caption(
                    "Words below this mark are assumed known but unverified \u{2014} use \
                     Quick scan to find the gaps.",
                )
                .color(p.ink3),
            )
            .wrap(),
        );
    });
}
