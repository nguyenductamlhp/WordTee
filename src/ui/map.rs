//! Map — spec 2.3's knowledge map, in the shape of the reference design: a
//! square per learning item coloured by what you know of it, a range picker,
//! and the word you are pointed at.
//!
//! Two views share the screen. The grid is the default and the one the
//! reference shows — a hundred items at a time, close enough to touch. Behind
//! the toggle is the overview spec 2.3 actually specifies: 25 blocks of 1.000,
//! each with a four-colour bar. The grid answers "what is in front of me"; the
//! overview answers "how far along am I", and neither answers the other.

use eframe::egui::{self, Align, Layout, RichText, Stroke, vec2};

use crate::app::Ctx;
use crate::dict::Band;
use crate::progress::{Source, State};
use crate::ui::{self, Icon, Tone, theme};

/// Spec 2.3: 25 blocks of 1.000.
const BLOCK: u32 = 1_000;
const BLOCKS: u32 = 25;
/// Items per grid page, as in the reference's "Range 801 – 900".
const RANGE: u32 = 100;
/// Squares per row: ten on a phone, where each is about 32 points and easy
/// to hit, twenty once the window is wide enough to keep them that size.
const NARROW_COLUMNS: u32 = 10;
const WIDE_COLUMNS: u32 = 20;
const WIDE_FROM: f32 = 560.0;

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

    let mut view = state.view;
    ui::screen_header(ui, "Knowledge map", |ui| {
        let at = usize::from(view == View::Blocks);
        if let Some(i) = ui::segmented(ui, &["Grid", "Blocks"], at) {
            view = if i == 1 { View::Blocks } else { View::Grid };
        }
    });
    state.view = view;

    match state.view {
        View::Grid => grid_view(ui, ctx, state),
        View::Blocks => blocks_view(ui, ctx, state),
    }
}

// -------------------------------------------------------------------------
// the grid (the reference's screen)
// -------------------------------------------------------------------------

fn grid_view(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    if let Some(sense) = ctx.dict.at_rank(state.selected) {
        selected_sheet(ui, ctx, sense);
    }

    let last = ctx.dict.learn_count();
    let end = (state.start + RANGE).min(last + 1);
    let counts = ctx.progress.tally(ctx.dict.learn_span(state.start..end));
    ui::page(ui, "map-grid", |ui| {
        range_picker(ui, ctx, state);
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            ui::stacked_bar(ui, counts, 8.0);
            legend(ui, Some(counts));
        });
        squares(ui, ctx, state);
    });
}

/// The word the grid is pointed at, in a sheet above the two actions.
fn selected_sheet(ui: &mut egui::Ui, ctx: &mut Ctx, sense: crate::dict::Sense) {
    let word = ctx.dict.word(sense.word);
    let voice = ctx.progress.accent;
    let headword = ctx.progress.casing.apply(word.text);
    let item_state = ctx.progress.state(&sense);
    let p = ui::palette(ui);
    let mut open = false;
    let (knew, learn) = ui::sheet(ui, "map-actions", p.surface, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(&headword)
                    .size(22.0)
                    .family(theme::semibold()),
            );
            if !word.ipa.is_empty() {
                ui.label(RichText::new(word.ipa).size(15.0).color(p.ink2));
            }
            if ui::can_speak()
                && ui::icon_button(ui, Icon::Speaker, "Play", ui::IconStyle::Soft).clicked()
            {
                ui::speak(word.text, 1.0, voice);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                open = ui::compact_button(ui, ui::Kind::Ghost, None, "Open").clicked();
            });
        });
        ui.add(egui::Label::new(RichText::new(sense.def).size(15.0)).wrap());
        let mut note = vec![
            sense.pos.label().to_owned(),
            sense.band().label().to_owned(),
        ];
        note.push(format!("#{}", ui::thousands(sense.rank)));
        note.push(item_state.label().to_owned());
        note.retain(|s| !s.is_empty());
        ui.label(theme::caption(note.join(" · ")).color(p.ink3));
        ui.add_space(8.0);
        ui::decision_buttons(ui)
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
    if open {
        *ctx.open_word = Some(sense.word);
    }
}

/// "‹  #1,201 – 1,300 ▾  ›".
fn range_picker(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let last = ctx.dict.learn_count();
    let ranges = last.div_ceil(RANGE);
    let index = (state.start - 1) / RANGE;

    ui.horizontal(|ui| {
        let previous = ui
            .add_enabled_ui(index > 0, |ui| {
                ui::icon_button(
                    ui,
                    Icon::ChevronLeft,
                    "Previous hundred",
                    ui::IconStyle::Soft,
                )
            })
            .inner;
        if previous.clicked() {
            state.show_rank(state.start - RANGE);
        }

        let label = format!(
            "#{} \u{2013} {}",
            ui::thousands(state.start),
            ui::thousands((state.start + RANGE - 1).min(last))
        );
        let width = ui.available_width() - ui::TOUCH - ui.spacing().item_spacing.x;
        egui::ComboBox::from_id_salt("range")
            .selected_text(RichText::new(label).size(17.0).family(theme::semibold()))
            .width(width)
            .height(360.0)
            .show_ui(ui, |ui| {
                // 250 ranges is a lot to scroll, so the list opens where the
                // user is rather than at rank 1.
                for i in 0..ranges {
                    let first = i * RANGE + 1;
                    let text = format!(
                        "{} \u{2013} {}",
                        ui::thousands(first),
                        ui::thousands((first + RANGE - 1).min(last))
                    );
                    let response = ui.selectable_label(i == index, RichText::new(text).size(15.0));
                    if i == index {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                    if response.clicked() {
                        state.show_rank(first);
                    }
                }
            });

        let next = ui
            .add_enabled_ui(index + 1 < ranges, |ui| {
                ui::icon_button(ui, Icon::ChevronRight, "Next hundred", ui::IconStyle::Soft)
            })
            .inner;
        if next.clicked() {
            state.show_rank(state.start + RANGE);
        }
    });
}

/// One square per item in the range, coloured by what is known of it.
fn squares(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let last = ctx.dict.learn_count();
    let width = ui.available_width();
    let gap = 4.0;
    let columns = if width >= WIDE_FROM {
        WIDE_COLUMNS
    } else {
        NARROW_COLUMNS
    };
    let side = ((width - gap * (columns - 1) as f32) / columns as f32).floor();
    let rows = RANGE.div_ceil(columns);
    let (area, _) = ui.allocate_exact_size(
        vec2(width, rows as f32 * (side + gap) - gap),
        egui::Sense::hover(),
    );
    let p = ui::palette(ui);
    let radius = (side * 0.18).min(6.0);
    let mut picked = None;
    for offset in 0..RANGE {
        let rank = state.start + offset;
        if rank > last {
            break;
        }
        let Some((sense, _)) = ctx.dict.learn_span(rank..rank + 1).next() else {
            continue;
        };
        let (row, col) = (offset / columns, offset % columns);
        let cell = egui::Rect::from_min_size(
            area.min + vec2(col as f32 * (side + gap), row as f32 * (side + gap)),
            egui::Vec2::splat(side),
        );
        let item_state = ctx.progress.state_at(sense, rank);
        let response = ui
            .interact(cell, ui.id().with(("square", rank)), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let painter = ui.painter();
        painter.rect_filled(cell, radius, ui::state_fill(&p, item_state));
        if item_state == State::AssumedKnown {
            ui::hatch(painter, cell.shrink(1.0), p.assumed_hatch);
        }
        if rank == state.selected {
            // The one the sheet is showing, ringed rather than recoloured so
            // its own state still reads.
            painter.rect_stroke(
                cell.expand(2.0),
                radius + 2.0,
                Stroke::new(2.5, p.primary_ink),
                egui::StrokeKind::Outside,
            );
        } else if response.hovered() {
            painter.rect_stroke(
                cell,
                radius,
                Stroke::new(1.5, p.ink3),
                egui::StrokeKind::Inside,
            );
        }
        if response.clicked() {
            picked = Some(rank);
        }
    }
    if let Some(rank) = picked {
        state.selected = rank;
    }
}

/// The four colours, with how many of each when `counts` is given.
fn legend(ui: &mut egui::Ui, counts: Option<[u32; 6]>) {
    let p = ui::palette(ui);
    let n = |states: &[State]| counts.map(|c| states.iter().map(|s| c[*s as usize]).sum::<u32>());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for (color, hatched, label, count) in [
            (p.known, false, "known", n(&[State::Known, State::Mastered])),
            (p.assumed, true, "probably known", n(&[State::AssumedKnown])),
            (
                p.learning,
                false,
                "learning",
                n(&[State::Learning, State::Review]),
            ),
            (p.unexplored, false, "new", n(&[State::Unexplored])),
        ] {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let (swatch, _) =
                    ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                ui.painter().rect_filled(swatch, 3.0, color);
                if hatched {
                    ui::hatch(ui.painter(), swatch, p.assumed_hatch);
                }
                if let Some(count) = count {
                    ui.label(
                        RichText::new(ui::thousands(count))
                            .size(theme::size::CAPTION)
                            .family(theme::semibold()),
                    );
                }
                ui.label(theme::caption(label).color(p.ink2));
            });
        }
    });
}

// -------------------------------------------------------------------------
// the overview (spec 2.3's own picture)
// -------------------------------------------------------------------------

fn blocks_view(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut MapState) {
    let p = ui::palette(ui);
    let here = ctx.progress.frontier.saturating_sub(1) / BLOCK;
    let mut open = None;
    ui::page(ui, "map-blocks", |ui| {
        ui.label(
            theme::caption(format!(
                "{} learning items, in {} blocks of 1,000",
                ui::thousands(ctx.dict.learn_count()),
                BLOCKS
            ))
            .color(p.ink2),
        );
        legend(ui, None);
        ui::list(ui, |ui| {
            for block in 0..BLOCKS {
                let counts = state.tallies[block as usize];
                let start = block * BLOCK + 1;
                let total: u32 = counts.iter().sum();
                let known = counts[State::Known as usize]
                    + counts[State::Mastered as usize]
                    + counts[State::AssumedKnown as usize];
                let tapped = ui::list_row(ui, ("block", block), |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(theme::body_strong(format!(
                            "{} \u{2013} {}",
                            ui::thousands(start),
                            ui::thousands(start + BLOCK - 1)
                        )));
                        ui::chip(ui, Band::of(start).label(), Tone::Neutral);
                        if block == here {
                            ui::chip(ui, "you are here", Tone::Primary);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let percent = (known * 100).checked_div(total).unwrap_or(0);
                            ui.label(theme::label(format!("{percent}%")).color(p.ink2));
                        });
                    });
                    ui::stacked_bar(ui, counts, 10.0);
                });
                if tapped {
                    open = Some(block);
                }
            }
        });
    });
    if let Some(block) = open {
        state.open_block(block);
    }
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
