//! Map — spec 2.3's knowledge map, in the shape of the reference design: the
//! word you are pointed at, a range picker, and a square per learning item
//! coloured by what you know of it.
//!
//! Two views share the screen. The grid is the default and the one the
//! reference shows — a hundred items at a time, close enough to touch. Behind
//! the toggle is the overview spec 2.3 actually specifies: 25 blocks of 1.000,
//! each with a four-colour bar. The grid answers "what is in front of me"; the
//! overview answers "how far along am I", and neither answers the other.

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::dict::{Band, SenseId};
use crate::progress::{Source, State};
use crate::ui;

/// Spec 2.3: 25 blocks of 1.000.
const BLOCK: u32 = 1_000;
const BLOCKS: u32 = 25;
/// Items per grid page, as in the reference's "Range 801 – 900".
const RANGE: u32 = 100;
/// Squares per row, when the window is wide enough for them.
const WIDE_COLUMNS: u32 = 20;
const NARROW_COLUMNS: u32 = 10;
/// Below this square size the grid stops being touchable.
const MIN_SQUARE: f32 = 11.0;

#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
enum View {
    #[default]
    Grid,
    Blocks,
}

#[derive(Default)]
pub struct MapState {
    view: View,
    /// First rank of the range on screen. Always a multiple of [`RANGE`] plus 1.
    start: u32,
    /// The rank whose card is shown, 0 until the user has picked one.
    selected: u32,
    /// Cached block tallies for the overview, and the revision they are for.
    tallies: Vec<[u32; 6]>,
    stamp: Option<u64>,
}

impl MapState {
    /// Opens the grid at the range holding `rank`.
    pub fn show_rank(&mut self, rank: u32) {
        self.start = (rank.saturating_sub(1) / RANGE) * RANGE + 1;
        self.selected = rank;
        self.view = View::Grid;
    }

    /// First rank of the range on screen.
    pub fn range_start(&self) -> u32 {
        self.start
    }

    /// Opens one of spec 2.3's blocks, as tapping it in the overview does.
    pub fn open_block(&mut self, block: u32) {
        self.show_rank(block.min(BLOCKS - 1) * BLOCK + 1);
    }

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
    // First visit: point the grid at where the user is actually working.
    if state.start == 0 {
        state.show_rank(ctx.progress.frontier.max(1));
    }

    egui::Panel::top("map-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Knowledge Map").size(17.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (label, next) = match state.view {
                    View::Grid => ("Blocks", View::Blocks),
                    View::Blocks => ("Grid", View::Grid),
                };
                if ui.button(RichText::new(label).size(12.5)).clicked() {
                    state.view = next;
                }
            });
        });
        ui.add_space(4.0);
    });

    match state.view {
        View::Grid => grid_view(ui, ctx, state),
        View::Blocks => blocks_view(ui, ctx, state),
    }
}

// -------------------------------------------------------------------------
// the grid (the reference's screen)
// -------------------------------------------------------------------------

fn grid_view(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let selected = ctx.dict.at_rank(state.selected);

    // The actions sit against the bottom edge, where a thumb is.
    if let Some(sense) = selected {
        egui::Panel::bottom("map-actions").show(ui, |ui| {
            ui.add_space(5.0);
            let (purple, teal) = (ui::BRAND_PURPLE, ui::BRAND_TEAL);
            let (mut learn, mut knew) = (false, false);
            ui.columns(2, |c| {
                learn = ui::action_button(&mut c[0], "Should Learn", purple).clicked();
                knew = ui::action_button(&mut c[1], "Already Knew", teal).clicked();
            });
            if learn {
                let undo = ctx
                    .progress
                    .start_learning(sense.id, Source::Manual, ctx.day);
                ctx.say_undoable("Added to your learning list.", undo);
            }
            if knew {
                let undo = ctx
                    .progress
                    .set_state(sense.id, State::Known, Source::Manual, ctx.day);
                ctx.say_undoable("Marked as known.", undo);
            }
            ui.add_space(6.0);
        });
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        if let Some(sense) = selected {
            word_card(ui, ctx, sense);
        }
        ui.add_space(8.0);
        range_picker(ui, ctx, state);
        ui.add_space(8.0);
        squares(ui, ctx, state);
        ui.add_space(10.0);
        legend(ui);
        ui.add_space(14.0);
    });
}

/// The selected word, in the same shape as the word page's card.
///
/// Without the illustration slot: the grid is the point of this screen, and an
/// empty picture frame would push it off the bottom.
fn word_card(ui: &mut egui::Ui, ctx: &mut Ctx, sense: crate::dict::Sense) {
    let word = ctx.dict.word(sense.word);
    let mut open = false;
    let response = ui::card(ui, None, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(word.text).size(26.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui::speak_buttons(ui, word.text);
            });
        });
        if !word.ipa.is_empty() {
            let muted = ui::muted(ui);
            ui.label(RichText::new(word.ipa).size(14.0).color(muted));
        }
        ui.add_space(4.0);
        let accent = ui::accent(ui);
        ui.label(RichText::new(sense.def).size(16.5).color(accent));
        if !sense.example.is_empty() {
            ui.label(RichText::new(sense.example).size(13.5));
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui::pos_chip(ui, sense.pos);
            ui::state_chip(ui, ctx.progress.state(&sense));
            ui::band_chip(ui, sense.band(), sense.rank);
        });
    });
    if ui::card_clicked(ui, &response) {
        open = true;
    }
    if open {
        *ctx.open_word = Some(sense.word);
    }
}

/// "‹  Range 801 – 900 ▾  ›".
fn range_picker(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let last = ctx.dict.learn_count();
    let ranges = last.div_ceil(RANGE);
    let index = (state.start - 1) / RANGE;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(index > 0, egui::Button::new("\u{2039}"))
            .clicked()
        {
            state.show_rank(state.start - RANGE);
        }

        let label = format!(
            "Range {} – {}",
            ui::thousands(state.start),
            ui::thousands((state.start + RANGE - 1).min(last))
        );
        egui::ComboBox::from_id_salt("range")
            .selected_text(RichText::new(label).size(14.0))
            .width(ui.available_width() - 44.0)
            .show_ui(ui, |ui| {
                // 250 ranges is a lot to scroll, so the list opens where the
                // user is rather than at rank 1.
                for i in 0..ranges {
                    let first = i * RANGE + 1;
                    let text = format!(
                        "{} – {}",
                        ui::thousands(first),
                        ui::thousands((first + RANGE - 1).min(last))
                    );
                    let response = ui.selectable_label(i == index, text);
                    if i == index {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        state.show_rank(first);
                    }
                }
            });

        if ui
            .add_enabled(index + 1 < ranges, egui::Button::new("\u{203A}"))
            .clicked()
        {
            state.show_rank(state.start + RANGE);
        }
    });
}

/// One square per item in the range, coloured by what is known of it.
fn squares(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let last = ctx.dict.learn_count();
    let width = ui.available_width();
    let gap = 3.0;
    let columns = if (width - gap * (WIDE_COLUMNS - 1) as f32) / WIDE_COLUMNS as f32 >= MIN_SQUARE {
        WIDE_COLUMNS
    } else {
        NARROW_COLUMNS
    };
    let side = ((width - gap * (columns - 1) as f32) / columns as f32).floor();

    let accent = ui::accent(ui);
    let mut picked = None;
    for row in 0..RANGE.div_ceil(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for col in 0..columns {
                let rank = state.start + row * columns + col;
                if rank > last || rank >= state.start + RANGE {
                    break;
                }
                let Some((sense, _)) = ctx.dict.learn_span(rank..rank + 1).next() else {
                    continue;
                };
                let item_state = ctx.progress.state_at(sense, rank);
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
                let painter = ui.painter();
                painter.rect_filled(rect, 2.5, ui::state_color(ui, item_state));
                if rank == state.selected {
                    // The one the card is showing, ringed rather than recoloured
                    // so its own state still reads.
                    ui.painter().rect_stroke(
                        rect.expand(1.0),
                        3.5,
                        egui::Stroke::new(2.0, accent),
                        egui::StrokeKind::Outside,
                    );
                }
                if response.clicked() {
                    picked = Some(rank);
                }
            }
        });
        ui.add_space(gap);
    }
    if let Some(rank) = picked {
        state.selected = rank;
    }
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal_wrapped(|ui| {
        for (color, text) in [
            (ui::c_known(ui), "Known"),
            (ui::c_assumed(ui), "Probably known"),
            (ui::c_learning(ui), "Learning"),
            (ui::c_unexplored(ui), "New"),
        ] {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(11.0, 11.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 2.0, color);
            let muted = ui::muted(ui);
            ui.label(RichText::new(text).size(11.5).color(muted));
            ui.add_space(6.0);
        }
    });
}

// -------------------------------------------------------------------------
// the overview (spec 2.3's own picture)
// -------------------------------------------------------------------------

fn blocks_view(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);
        let muted = ui::muted(ui);
        ui.label(
            RichText::new(format!(
                "{} learning items, in {} blocks of 1,000",
                ui::thousands(ctx.dict.learn_count()),
                BLOCKS
            ))
            .size(12.5)
            .color(muted),
        );
        ui.add_space(6.0);
        legend(ui);
        ui.add_space(6.0);

        let here = ctx.progress.frontier.saturating_sub(1) / BLOCK;
        let mut open = None;
        for block in 0..BLOCKS {
            let counts = state.tallies[block as usize];
            let start = block * BLOCK + 1;
            let total: u32 = counts.iter().sum();
            let known = counts[State::Known as usize]
                + counts[State::Mastered as usize]
                + counts[State::AssumedKnown as usize];

            let accent = ui::accent(ui);
            let response = ui::card(ui, (block == here).then_some(accent), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} – {}",
                            ui::thousands(start),
                            ui::thousands(start + BLOCK - 1)
                        ))
                        .strong(),
                    );
                    let muted = ui::muted(ui);
                    ui::chip(ui, Band::of(start).label(), muted);
                    if block == here {
                        let accent = ui::accent(ui);
                        ui::chip(ui, "you are here", accent);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let percent = (known * 100).checked_div(total).unwrap_or(0);
                        let muted = ui::muted(ui);
                        ui.label(RichText::new(format!("{percent}%")).size(13.0).color(muted));
                    });
                });
                ui.add_space(3.0);
                ui::progress_bar(ui, counts, 12.0);
            });
            if ui::card_clicked(ui, &response) {
                open = Some(block);
            }
            ui.add_space(4.0);
        }
        if let Some(block) = open {
            state.open_block(block);
        }
        ui.add_space(12.0);
    });
}

/// Spec 2.2's Quick Scan card, used by the study tab: one item, two answers.
pub fn scan_card(ui: &mut egui::Ui, ctx: &mut Ctx, sense: SenseId) -> bool {
    let sense = ctx.dict.sense(sense);
    let word = ctx.dict.word(sense.word);
    let mut answered = false;

    let accent = ui::accent(ui);
    ui::card(ui, Some(accent), |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(word.text).size(30.0).strong());
            if !word.ipa.is_empty() {
                let muted = ui::muted(ui);
                ui.label(RichText::new(word.ipa).size(14.0).color(muted));
            }
            ui.horizontal(|ui| {
                ui::pos_chip(ui, sense.pos);
                ui::band_chip(ui, sense.band(), sense.rank);
            });
            ui.add_space(6.0);
        });
    });
    ui.add_space(10.0);
    let (purple, teal) = (ui::BRAND_PURPLE, ui::BRAND_TEAL);
    let (mut unknown, mut known) = (false, false);
    ui.columns(2, |c| {
        unknown = ui::action_button(&mut c[0], "Don't know", purple).clicked();
        known = ui::action_button(&mut c[1], "Known", teal).clicked();
    });
    if unknown {
        // Spec 3.1: a "don't know" here goes straight into Learning.
        ctx.progress
            .start_learning(sense.id, Source::Manual, ctx.day);
        answered = true;
    }
    if known {
        ctx.progress
            .set_state(sense.id, State::Known, Source::Manual, ctx.day);
        answered = true;
    }
    if answered {
        ctx.progress.scanned_today += 1;
    }
    ui.add_space(8.0);
    ui.collapsing("Show meaning", |ui| {
        ui.label(RichText::new(sense.def).size(14.0));
        if !sense.example.is_empty() {
            let muted = ui::muted(ui);
            ui.label(
                RichText::new(sense.example)
                    .size(13.0)
                    .italics()
                    .color(muted),
            );
        }
    });
    answered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_starts_on_a_hundred_boundary() {
        let mut state = MapState::default();
        for (rank, start) in [(1, 1), (100, 1), (101, 101), (850, 801), (25_000, 24_901)] {
            state.show_rank(rank);
            assert_eq!(state.start, start, "rank {rank}");
            assert_eq!(state.selected, rank);
        }
    }

    #[test]
    fn opening_a_block_lands_on_its_first_range() {
        let mut state = MapState::default();
        state.open_block(0);
        assert_eq!(state.start, 1);
        state.open_block(8);
        assert_eq!(state.start, 8_001);
        // And the last block is still inside the list.
        state.open_block(BLOCKS - 1);
        assert_eq!(state.start, 24_001);
        state.open_block(99);
        assert_eq!(state.start, 24_001, "clamped to the last block");
    }

    #[test]
    fn the_grid_is_the_view_a_block_opens_into() {
        let mut state = MapState {
            view: View::Blocks,
            ..MapState::default()
        };
        state.open_block(3);
        assert_eq!(state.view, View::Grid);
    }
}
