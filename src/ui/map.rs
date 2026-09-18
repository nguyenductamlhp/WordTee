//! Bản đồ — spec 2.3's knowledge map: 25 blocks of 1.000 learning items, each
//! with a four-colour bar, and a drill-down into any one of them.

use eframe::egui::{self, RichText};

use crate::app::{Ctx, Tab};
use crate::dict::{Band, SenseId};
use crate::progress::{Source, State};

use crate::ui;

/// Spec 2.3: 25 blocks of 1.000.
const BLOCK: u32 = 1_000;
const BLOCKS: u32 = 25;
/// How many items the drill-down lists at once.
const PAGE: usize = 60;

#[derive(Default)]
pub struct MapState {
    /// Which block is open, 0-based.
    open: Option<u32>,
    filter: Option<State>,
    /// Cached per-block tallies, and the revision they were computed at.
    tallies: Vec<[u32; 6]>,
    stamp: Option<u64>,
}

impl MapState {
    /// Opens one block's drill-down, as tapping it does.
    pub fn open_block(&mut self, block: u32) {
        self.open = Some(block.min(BLOCKS - 1));
        self.filter = None;
    }

    /// Recomputes the block tallies when anything in the progress changed.
    ///
    /// Counting 25.000 items is cheap but not free, and the map redraws every
    /// frame.
    fn refresh(&mut self, ctx: &Ctx) {
        let stamp = Some(ctx.progress.revision());
        if stamp == self.stamp && !self.tallies.is_empty() {
            return;
        }
        self.stamp = stamp;
        self.tallies = (0..BLOCKS)
            .map(|b| {
                ctx.progress
                    .tally(ctx.dict.learn_span(b * BLOCK + 1..(b + 1) * BLOCK + 1))
            })
            .collect();
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    state.refresh(ctx);
    match state.open {
        Some(block) => block_page(ui, ctx, state, block),
        None => overview(ui, ctx, state),
    }
}

fn overview(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    ui.add_space(6.0);
    ui.label(RichText::new("Bản đồ tri thức").size(20.0).strong());
    ui.label(
        RichText::new(format!(
            "{} đơn vị học, chia thành {} khối 1.000",
            ui::thousands(ctx.dict.learn_count()),
            BLOCKS
        ))
        .size(12.5)
        .color(ui::MUTED),
    );
    ui.add_space(6.0);
    legend(ui);
    ui.horizontal_wrapped(|ui| {
        for band in [Band::Core, Band::Advanced, Band::Academic] {
            ui.label(
                RichText::new(format!("{} {}", band.label(), band.range()))
                    .size(11.0)
                    .color(ui::MUTED),
            );
            ui.add_space(4.0);
        }
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        let frontier_block = ctx.progress.frontier.saturating_sub(1) / BLOCK;
        for block in 0..BLOCKS {
            let counts = state.tallies[block as usize];
            let start = block * BLOCK + 1;
            let band = Band::of(start);
            let known = counts[State::Known as usize]
                + counts[State::Mastered as usize]
                + counts[State::AssumedKnown as usize];
            let total: u32 = counts.iter().sum();

            let response = ui::card(ui, (block == frontier_block).then_some(ui::ACCENT), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} – {}",
                            ui::thousands(start),
                            ui::thousands(start + BLOCK - 1)
                        ))
                        .strong(),
                    );
                    ui::chip(ui, band.label(), ui::MUTED);
                    if block == frontier_block {
                        ui::chip(ui, "đang ở đây", ui::ACCENT);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let percent = (known * 100).checked_div(total).unwrap_or(0);
                        ui.label(
                            RichText::new(format!("{percent}%"))
                                .size(13.0)
                                .color(ui::MUTED),
                        );
                    });
                });
                ui.add_space(3.0);
                ui::progress_bar(ui, counts, 12.0);
            });
            if ui::card_clicked(ui, &response) {
                state.open = Some(block);
                state.filter = None;
            }
            ui.add_space(4.0);
        }
        ui.add_space(12.0);
    });
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for (color, text) in [
            (ui::C_KNOWN, "Đã biết / Nhớ vững"),
            (ui::C_ASSUMED, "Suy ra (kẻ sọc)"),
            (ui::C_LEARNING, "Đang học / ôn"),
            (ui::C_UNEXPLORED, "Chưa gặp"),
        ] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color);
            ui.label(RichText::new(text).size(11.5).color(ui::MUTED));
            ui.add_space(4.0);
        }
    });
}

fn block_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState, block: u32) {
    let start = block * BLOCK + 1;
    let counts = state.tallies[block as usize];

    egui::Panel::top("block-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("←").clicked() {
                state.open = None;
            }
            ui.label(
                RichText::new(format!(
                    "{} – {}",
                    ui::thousands(start),
                    ui::thousands(start + BLOCK - 1)
                ))
                .size(18.0)
                .strong(),
            );
        });
        ui::progress_bar(ui, counts, 10.0);
        ui.add_space(4.0);
        // Spec 2.3: filter the list, and scan this block.
        ui.horizontal_wrapped(|ui| {
            for (label, want) in [
                ("Tất cả", None),
                ("Chưa gặp", Some(State::Unexplored)),
                ("Suy ra", Some(State::AssumedKnown)),
                ("Đang học", Some(State::Learning)),
                ("Đã biết", Some(State::Known)),
            ] {
                if ui
                    .selectable_label(state.filter == want, RichText::new(label).size(12.5))
                    .clicked()
                {
                    state.filter = want;
                }
            }
        });
        if ui.button("⚡ Quét nhanh khối này").clicked() {
            *ctx.goto = Some(Tab::Study);
            ctx.say("Mở tab Học để quét nhanh.");
        }
        ui.add_space(4.0);
    });

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut shown = 0;
        let mut open_word = None;
        for rank in start..start + BLOCK {
            if shown >= PAGE {
                break;
            }
            let Some(sense) = ctx.dict.at_rank(rank) else {
                continue;
            };
            let current = ctx.progress.state(&sense);
            // Learning and Review are one bucket in the filter, as on the bar.
            let matches = match state.filter {
                None => true,
                Some(State::Learning) => matches!(current, State::Learning | State::Review),
                Some(State::Known) => matches!(current, State::Known | State::Mastered),
                Some(want) => current == want,
            };
            if !matches {
                continue;
            }
            shown += 1;
            let word = ctx.dict.word(sense.word);
            let response = ui::card(ui, None, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("#{}", ui::thousands(rank)))
                            .size(11.5)
                            .color(ui::MUTED),
                    );
                    ui.label(RichText::new(word.text).size(15.5).strong());
                    ui::pos_chip(ui, sense.pos);
                    ui::state_chip(ui, current);
                });
                ui.label(RichText::new(sense.def).size(13.0).color(ui::MUTED));
            });
            if ui::card_clicked(ui, &response) {
                open_word = Some(sense.word);
            }
            ui.add_space(3.0);
        }
        if shown == 0 {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Không có mục nào ở trạng thái này.").color(ui::MUTED));
            });
        } else if shown >= PAGE {
            ui.label(
                RichText::new(format!("Hiển thị {PAGE} mục đầu tiên."))
                    .size(12.0)
                    .color(ui::MUTED),
            );
        }
        if let Some(word) = open_word {
            *ctx.open_word = Some(word);
        }
        ui.add_space(12.0);
    });
}

/// Spec 2.2's Quick Scan, used by the study tab: one item, two answers.
pub fn scan_card(ui: &mut egui::Ui, ctx: &mut Ctx, sense: SenseId) -> bool {
    let sense = ctx.dict.sense(sense);
    let word = ctx.dict.word(sense.word);
    let mut answered = false;

    ui::card(ui, Some(ui::ACCENT), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(word.text).size(30.0).strong());
            if !word.ipa.is_empty() {
                ui.label(RichText::new(word.ipa).size(14.0).color(ui::ACCENT));
            }
            ui.horizontal(|ui| {
                ui::pos_chip(ui, sense.pos);
                ui::band_chip(ui, sense.band(), sense.rank);
            });
            ui.add_space(6.0);
        });
    });
    ui.add_space(10.0);
    ui.columns(2, |c| {
        if c[0]
            .add_sized(
                [c[0].available_width(), 46.0],
                egui::Button::new(RichText::new("Chưa biết").color(ui::WARN)),
            )
            .clicked()
        {
            // Spec 3.1: a "don't know" here goes straight into Learning.
            ctx.progress
                .start_learning(sense.id, Source::Manual, ctx.day);
            answered = true;
        }
        if c[1]
            .add_sized(
                [c[1].available_width(), 46.0],
                egui::Button::new(RichText::new("Đã biết").color(ui::GOOD)),
            )
            .clicked()
        {
            ctx.progress
                .set_state(sense.id, State::Known, Source::Manual, ctx.day);
            answered = true;
        }
    });
    if answered {
        ctx.progress.scanned_today += 1;
    }
    ui.add_space(8.0);
    ui.collapsing("Xem nghĩa", |ui| {
        ui.label(RichText::new(sense.def).size(14.0));
        if !sense.example.is_empty() {
            ui.label(
                RichText::new(sense.example)
                    .size(13.0)
                    .italics()
                    .color(ui::MUTED),
            );
        }
    });
    answered
}
