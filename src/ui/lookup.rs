//! Look up — the search box, the results list and the word page.
//!
//! This is spec 1's "Acquisition Funnel": looking a word up is where learning
//! starts, so every sense on the page carries its own state, and the two
//! actions at the bottom act on the one being read (spec 1.4).

use eframe::egui::{self, Align, Layout, Margin, RichText};

use crate::app::Ctx;
use crate::dict::{Kind, Relation, Sense, SenseId, WordId};
use crate::progress::{Source, State};
use crate::quiz::{self, Choice, Cloze};
use crate::search::{self, Results, Tier};
use crate::study::{self, Exercise};
use crate::ui::{self, FieldState, Icon, Tone, theme};

/// How many headwords the results list shows.
const RESULT_LIMIT: usize = 40;

/// How many of the other meanings show before "Show more".
const OTHER_MEANINGS: usize = 3;

/// Spec 1.4's Quick Test: two questions, because one four-option question is
/// 25% guessable.
struct QuickTest {
    sense: SenseId,
    first: Exercise,
    second: Choice,
    /// 0 = first question, 1 = second, 2 = verdict.
    step: u8,
    typed: String,
    picked: Option<usize>,
    /// The current question has been answered, and how.
    verdict: Option<bool>,
    /// Still on track for "Known" — set false by the first wrong answer.
    perfect: bool,
    /// The answer field has been given focus once.
    focused: bool,
}

#[derive(Default)]
pub struct LookupState {
    query: String,
    /// The query the current `results` were computed for.
    searched: String,
    results: Results,
    /// The word page, when one is open.
    open: Option<WordId>,
    /// Which sense the page leads with and the actions act on (spec 1.4:
    /// "nghĩa đang xem").
    focus: usize,
    /// Spec 1.3: opening a word from a link pushes onto this, so Back returns
    /// to where you were.
    back: Vec<WordId>,
    test: Option<QuickTest>,
    /// Whether the word family and phrases are unfolded. Collapsed by
    /// default: the card answers "what does this mean", and the word family
    /// is a second question.
    show_usage: bool,
    /// Every other meaning is listed, not just the first few.
    show_all: bool,
}

impl LookupState {
    /// The headwords the last query produced.
    pub fn hits(&self) -> &[crate::search::Hit] {
        &self.results.words
    }

    /// The word page currently open, if any.
    pub fn open_word(&self) -> Option<WordId> {
        self.open
    }

    /// Is a Quick Test running? The tab bar hides while one is.
    pub fn testing(&self) -> bool {
        self.open.is_some() && self.test.is_some()
    }

    /// Types into the search box, as tapping a suggestion does.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
    }

    /// Starts a Quick Test on the focused sense (spec 1.4).
    pub fn begin_quick_test(&mut self, ctx: &mut Ctx, sense: &Sense) {
        self.test = Some(build_quick_test(ctx, sense));
    }

    /// Opens a word page, remembering where we came from.
    pub fn open(&mut self, word: WordId) {
        if let Some(previous) = self.open
            && previous != word
        {
            self.back.push(previous);
        }
        self.open = Some(word);
        self.focus = 0;
        self.test = None;
        self.show_usage = false;
        self.show_all = false;
    }

    fn close(&mut self) {
        self.open = self.back.pop();
        self.focus = 0;
        self.test = None;
        self.show_all = false;
    }
}

pub fn show(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState) {
    match state.open {
        Some(word) => word_page(ui, ctx, state, word),
        None => search_page(ui, ctx, state),
    }
}

// -------------------------------------------------------------------------
// the search page
// -------------------------------------------------------------------------

fn search_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState) {
    ui::screen_header(ui, "Look up", |_| {});
    let page = ui.visuals().panel_fill;
    let field = egui::Panel::top("search-box")
        .frame(egui::Frame::new().fill(page).inner_margin(Margin {
            left: ui::GUTTER,
            right: ui::GUTTER,
            top: 4,
            bottom: 8,
        }))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui::search_field(ui, &mut state.query, "Search English or Vietnamese")
        })
        .inner;
    if state.query.is_empty() && state.searched.is_empty() {
        field.request_focus();
    }

    // Re-run only when the text actually changed: spec 1.1 budgets 50 ms per
    // keystroke, and there is no reason to spend it twice on the same query.
    if state.query != state.searched {
        state.searched = state.query.clone();
        let looked_up = ctx.progress.lookups.clone();
        state.results = search::lookup(
            ctx.dict,
            &state.query,
            &|word| looked_up.contains(&word),
            RESULT_LIMIT,
        );
    }

    if state.query.trim().is_empty() {
        idle_hint(ui, ctx, state);
        return;
    }
    let p = ui::palette(ui);
    if state.results.is_empty() {
        ui::page(ui, "no-results", |ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(theme::heading("Nothing found"));
                ui.label(
                    theme::caption("Try it without tone marks, or check the spelling.")
                        .color(p.ink2),
                );
            });
        });
        return;
    }

    let mut open = None;
    ui::page(ui, "results", |ui| {
        ui::list(ui, |ui| {
            for hit in &state.results.words {
                let word = ctx.dict.word(hit.word);
                let tapped = ui::list_row(ui, hit.word, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(word.text)
                                .size(18.0)
                                .family(theme::semibold()),
                        );
                        if !word.ipa.is_empty() {
                            ui.label(theme::caption(word.ipa).color(p.ink3));
                        }
                        if word.kind == Kind::Phrase {
                            ui::chip(ui, "phrase", Tone::Primary);
                        }
                        if hit.tier != Tier::Exact {
                            ui::chip(ui, hit.tier.label(), Tone::Neutral);
                        }
                    });
                    // Spec 1.1: a lemma reached through an inflected form says so.
                    if let Some(form) = &hit.form {
                        ui.label(
                            theme::caption(format!(
                                "\u{201c}{}\u{201d} is the {} of this word",
                                form.surface, form.tag
                            ))
                            .color(p.primary_ink),
                        );
                    }
                    let gloss: Vec<&str> = word
                        .senses()
                        .map(|id| ctx.dict.sense(id))
                        .filter(|s| !s.is_inflection)
                        .map(|s| s.def)
                        .take(2)
                        .collect();
                    if !gloss.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(gloss.join(" · ")).size(15.0).color(p.ink2),
                            )
                            .wrap(),
                        );
                    }
                });
                if tapped {
                    open = Some(hit.word);
                }
            }
        });

        // Spec 5.2: the Vietnamese → English direction.
        if !state.results.reverse.is_empty() {
            ui::section_title(ui, "Vietnamese to English", "");
            ui::list(ui, |ui| {
                for &id in &state.results.reverse {
                    let sense = ctx.dict.sense(id);
                    let word = ctx.dict.word(sense.word);
                    let tapped = ui::list_row(ui, ("reverse", id), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            RichText::new(word.text)
                                .size(18.0)
                                .family(theme::semibold()),
                        );
                        ui.add(
                            egui::Label::new(RichText::new(sense.def).size(15.0).color(p.ink2))
                                .wrap(),
                        );
                        ui.label(
                            theme::caption(sense_note(&sense, State::Unexplored)).color(p.ink3),
                        );
                    });
                    if tapped {
                        open = Some(sense.word);
                    }
                }
            });
        }
    });
    if let Some(word) = open {
        state.open(word);
    }
}

/// What the search tab shows before anything is typed.
fn idle_hint(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState) {
    let p = ui::palette(ui);
    ui::page(ui, "lookup-idle", |ui| {
        ui.add_space(4.0);
        ui.label(theme::label("Try one").color(p.ink2));
        ui.horizontal_wrapped(|ui| {
            for word in ["decision", "swimming", "teh", "run", "quyết định"] {
                if ui::compact_button(ui, ui::Kind::Outline, None, word).clicked() {
                    state.query = word.to_owned();
                }
            }
        });
        ui.add(
            egui::Label::new(
                theme::caption(
                    "Typos still find the word (teh finds the), so do inflected forms \
                     (swimming finds swim), and Vietnamese with tone marks searches \
                     the meanings instead.",
                )
                .color(p.ink2),
            )
            .wrap(),
        );
        ui.label(
            theme::caption(format!(
                "{} headwords, {} senses \u{2014} all on this device, no connection needed.",
                ui::thousands(ctx.dict.word_count()),
                ui::thousands(ctx.dict.sense_count())
            ))
            .color(p.ink3),
        );
    });
}

/// "noun · Academic · #10,289 · Learning".
fn sense_note(sense: &Sense, state: State) -> String {
    let mut parts = Vec::new();
    if !sense.pos.label().is_empty() {
        parts.push(sense.pos.label().to_owned());
    }
    parts.push(sense.band().label().to_owned());
    if sense.rank > 0 {
        parts.push(format!("#{}", ui::thousands(sense.rank)));
    }
    if state != State::Unexplored {
        parts.push(state.label().to_owned());
    }
    parts.join(" · ")
}

// -------------------------------------------------------------------------
// the word page (spec 1.3)
// -------------------------------------------------------------------------

fn word_page(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState, id: WordId) {
    let word = ctx.dict.word(id);
    // Spec 2.3's Lookup signal, and spec 3.1's "looking an assumed-known item
    // up again means it was not really known".
    let senses: Vec<Sense> = word.senses().map(|s| ctx.dict.sense(s)).collect();
    ctx.progress.note_lookup(id, &senses);

    let meanings: Vec<Sense> = senses
        .iter()
        .copied()
        .filter(|s| !s.is_inflection)
        .collect();
    state.focus = state.focus.min(meanings.len().saturating_sub(1));
    let focused = meanings.get(state.focus).copied();

    if state.test.is_some() {
        let mut finished = false;
        if let Some(test) = &mut state.test {
            finished = quick_test_page(ui, ctx, test);
        }
        if finished {
            state.test = None;
        }
        return;
    }

    // --- the header: back, and the lighter third action ---
    let mut back = false;
    let mut test = false;
    ui::bar_header(ui, |ui| {
        back = ui::icon_button(ui, Icon::ChevronLeft, "Back", ui::IconStyle::Plain).clicked();
        if focused.is_some() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                test = ui::compact_button(ui, ui::Kind::Outline, Some(Icon::Check), "Quick test")
                    .clicked();
            });
        }
    });
    if back {
        state.close();
        return;
    }
    if test && let Some(sense) = focused {
        state.test = Some(build_quick_test(ctx, &sense));
        return;
    }

    // --- the two actions, pinned to the bottom (spec 1.4) ---
    if let Some(sense) = focused {
        let (knew, learn) = ui::action_bar(ui, "actions", ui::decision_buttons);
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
    }

    let voice = ctx.progress.accent;
    let headword = ctx.progress.casing.apply(word.text);
    let p = ui::palette(ui);
    ui::page(ui, "word", |ui| {
        // --- the card: word, sound, the meaning being read ---
        ui::card(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add(egui::Label::new(theme::display(&headword)).wrap());
            if !word.ipa.is_empty() || ui::can_speak() {
                ui.horizontal(|ui| {
                    if !word.ipa.is_empty() {
                        ui.label(theme::body(word.ipa).color(p.ink2));
                    }
                    ui::speak_buttons(ui, word.text, voice);
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(sense) = focused {
                    ui::pos_chip(ui, sense.pos);
                    ui::band_chip(ui, sense.band(), sense.rank);
                    ui::state_chip(ui, ctx.progress.state(&sense));
                }
                if word.kind == Kind::Phrase {
                    ui::chip(ui, "phrase", Tone::Primary);
                }
                if word.offensive {
                    ui::chip(ui, "coarse \u{2014} lookup only", Tone::Wrong);
                }
            });
            if let Some(sense) = focused {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(sense.def)
                            .size(20.0)
                            .family(theme::semibold()),
                    )
                    .wrap(),
                );
                if !sense.example.is_empty() && ctx.progress.show_examples {
                    ui.horizontal(|ui| {
                        let width = ui.available_width() - if ui::can_speak() { 52.0 } else { 0.0 };
                        ui.allocate_ui_with_layout(
                            egui::vec2(width, 0.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(sense.example)
                                            .size(15.0)
                                            .italics()
                                            .color(p.ink2),
                                    )
                                    .wrap(),
                                )
                            },
                        );
                        ui::speak_button(ui, sense.example, voice);
                    });
                    // Spec 1.4: remember we showed it, so no test re-uses it.
                    ctx.shown.mark(sense.id);
                }
            }
        });

        // Spec 1.1: "cũng là dạng của …".
        let forms = search::forms_of(ctx.dict, word.norm);
        if !forms.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(4.0);
                ui.label(theme::caption("also").color(p.ink2));
                for (lemma, tag) in forms {
                    let link = RichText::new(format!("{tag} of {}", lemma.text))
                        .size(theme::size::CAPTION)
                        .color(p.primary_ink);
                    if ui.link(link).clicked() {
                        state.open(lemma.id);
                    }
                }
            });
        }

        if meanings.is_empty() {
            ui.label(
                theme::caption("This entry is only an inflected form of another word.")
                    .color(p.ink2),
            );
        }

        // --- the other meanings (spec 1.2: one sense, one learning item) ---
        if meanings.len() > 1 {
            let others: Vec<(usize, Sense)> = meanings
                .iter()
                .copied()
                .enumerate()
                .filter(|(i, _)| *i != state.focus)
                .collect();
            let shown = if state.show_all {
                others.len()
            } else {
                others.len().min(OTHER_MEANINGS)
            };
            ui::section_title(ui, "Other meanings", &format!("{} more", others.len()));
            ui::list(ui, |ui| {
                for &(i, sense) in &others[..shown] {
                    let item_state = ctx.progress.state(&sense);
                    let tapped = ui::list_row(ui, ("meaning", i), |ui| {
                        ui.horizontal(|ui| {
                            let (badge, _) = ui
                                .allocate_exact_size(egui::Vec2::splat(24.0), egui::Sense::hover());
                            ui.painter().circle_filled(badge.center(), 12.0, p.sunken);
                            ui.painter().text(
                                badge.center(),
                                egui::Align2::CENTER_CENTER,
                                (i + 1).to_string(),
                                egui::FontId::new(12.5, theme::semibold()),
                                p.ink2,
                            );
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add(egui::Label::new(theme::body(sense.def)).wrap());
                                ui.label(
                                    theme::caption(sense_note(&sense, item_state)).color(p.ink3),
                                );
                            });
                        });
                    });
                    if tapped {
                        state.focus = i;
                    }
                }
                if shown < others.len() {
                    let more = others.len() - shown;
                    let tapped = ui::list_row(ui, "more", |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                theme::label(format!("Show {more} more")).color(p.primary_ink),
                            );
                        });
                    });
                    if tapped {
                        state.show_all = true;
                    }
                }
            });
        }

        // Spec 1.3's "Cách dùng": the word family and the phrases built on it.
        let has_usage = !ctx.dict.relations(id).is_empty()
            || ctx
                .dict
                .with_prefix(&format!("{} ", word.norm))
                .next()
                .is_some();
        if has_usage {
            ui::list(ui, |ui| {
                if ui::disclosure_row(ui, "Word family & phrases", state.show_usage) {
                    state.show_usage = !state.show_usage;
                }
                if state.show_usage {
                    usage_block(ui, ctx, state, id);
                }
            });
        }
    });
}

/// Spec 1.3's "Cách dùng" block.
///
/// The spec also wants collocations, grammar patterns, register labels, UK/US
/// differences and the mistakes Vietnamese learners make. Those are authored
/// content (spec 4.1) and no free dictionary carries them, so what is shown
/// here is what the data actually supports: the word family, synonyms and
/// antonyms, and the phrases built on this word.
fn usage_block(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState, id: WordId) {
    let word = ctx.dict.word(id);
    let relations = ctx.dict.relations(id);
    // Phrases and idioms that start with this word — "give up" under "give".
    let phrases: Vec<_> = ctx
        .dict
        .with_prefix(&format!("{} ", word.norm))
        .take(12)
        .collect();
    let p = ui::palette(ui);
    let link = |text: &str| RichText::new(text).size(15.0).color(p.primary_ink);

    ui.spacing_mut().item_spacing.y = 10.0;
    for kind in [
        Relation::Derived,
        Relation::Synonym,
        Relation::Antonym,
        Relation::Related,
    ] {
        let words: Vec<&str> = relations
            .iter()
            .filter(|(k, _)| *k == kind)
            .map(|(_, text)| *text)
            .take(10)
            .collect();
        if words.is_empty() {
            continue;
        }
        ui.label(
            theme::caption(kind.label())
                .color(p.ink2)
                .family(theme::semibold()),
        );
        ui.horizontal_wrapped(|ui| {
            for text in words {
                // Only link the ones that are actually in the dictionary.
                match ctx.dict.exact(&search::normalize(text)) {
                    Some(target) => {
                        if ui.link(link(text)).clicked() {
                            state.open(target.id);
                        }
                    }
                    None => {
                        ui.label(RichText::new(text).size(15.0).color(p.ink2));
                    }
                }
            }
        });
    }
    if !phrases.is_empty() {
        ui.label(
            theme::caption("Phrases")
                .color(p.ink2)
                .family(theme::semibold()),
        );
        ui.horizontal_wrapped(|ui| {
            for phrase in phrases {
                if ui.link(link(phrase.text)).clicked() {
                    state.open(phrase.id);
                }
            }
        });
    }
    ui.add_space(4.0);
}

// -------------------------------------------------------------------------
// Quick Test (spec 1.4)
// -------------------------------------------------------------------------

fn build_quick_test(ctx: &mut Ctx, sense: &Sense) -> QuickTest {
    // Question 1 is a gap-fill on a sentence the user has not been shown; when
    // there is none, it falls back to picking the word for a meaning, which
    // also cannot be answered from what is on screen.
    let first = study::exercise(ctx.dict, ctx.rng, sense, 2, ctx.shown);
    let second = quiz::meaning_choice(ctx.dict, ctx.rng, sense, 4).unwrap_or(Choice {
        prompt: String::new(),
        options: Vec::new(),
        answer: 0,
    });
    QuickTest {
        sense: sense.id,
        first,
        second,
        step: 0,
        typed: String::new(),
        picked: None,
        verdict: None,
        perfect: true,
        focused: false,
    }
}

/// Draws the Quick Test. Returns true when it is over and should be dropped.
fn quick_test_page(ui: &mut egui::Ui, ctx: &mut Ctx, test: &mut QuickTest) -> bool {
    let sense = ctx.dict.sense(test.sense);
    let word = ctx.dict.word(sense.word);

    // Questions that cannot be asked are skipped rather than shown blank.
    if test.step == 0 && !matches!(test.first, Exercise::Fill(_) | Exercise::PickWord(_)) {
        test.step = 1;
    }
    if test.step == 1 && test.second.options.is_empty() {
        test.step = 2;
    }
    if test.step >= 2 {
        // Spec 1.4: both right -> Known (source = test); anything wrong ->
        // Learning. Either way with an Undo.
        let (state, source, message) = if test.perfect {
            (State::Known, Source::Test, "Both right — marked as known.")
        } else {
            (
                State::Learning,
                Source::Study,
                "Not quite — added to your learning list.",
            )
        };
        let undo = ctx.progress.set_state(test.sense, state, source, ctx.day);
        ctx.say_undoable(message, undo);
        return true;
    }

    let count = format!("{} / 2", test.step + 1);
    if ui::focus_header(ui, test.step as f32 / 2.0, &count) {
        return true;
    }

    // --- the bottom: Check for a typed answer, then the verdict ---
    let mut checked: Option<bool> = None;
    match test.verdict {
        Some(right) => {
            let answer = match (test.step, &test.first) {
                (0, Exercise::Fill(gap)) => gap.answer.clone(),
                (0, Exercise::PickWord(choice)) => choice
                    .options
                    .get(choice.answer)
                    .cloned()
                    .unwrap_or_default(),
                _ => test
                    .second
                    .options
                    .get(test.second.answer)
                    .cloned()
                    .unwrap_or_default(),
            };
            let detail = if right {
                String::new()
            } else {
                format!("Answer: {answer}")
            };
            let title = if right { "Correct" } else { "Not quite" };
            if ui::feedback_sheet(ui, right, title, &detail, "") {
                test.perfect &= right;
                test.step += 1;
                test.verdict = None;
                test.picked = None;
                test.typed.clear();
                test.focused = false;
            }
        }
        None => {
            if test.step == 0
                && let Exercise::Fill(gap) = &test.first
            {
                let filled = !test.typed.trim().is_empty();
                ui::action_bar(ui, "actions", |ui| {
                    ui.add_enabled_ui(filled, |ui| {
                        if ui::primary_button(ui, "Check").clicked() {
                            checked = Some(gap.accepts(&test.typed));
                        }
                    });
                });
            }
        }
    }

    let p = ui::palette(ui);
    ui::page(ui, "quick-test", |ui| {
        ui::chip(ui, "Quick test", Tone::Primary);
        match (test.step, test.first.clone()) {
            (0, Exercise::Fill(gap)) => {
                fill_question(ui, &gap, test.verdict);
                let state = match test.verdict {
                    None => FieldState::Typing,
                    Some(true) => FieldState::Right,
                    Some(false) => FieldState::Wrong,
                };
                let hint = format!("starts with \u{201c}{}\u{201d}\u{2026}", gap.hint);
                let field = ui::answer_field(ui, "quick-answer", &mut test.typed, &hint, state);
                if test.verdict.is_none() && !test.focused {
                    field.request_focus();
                    test.focused = true;
                }
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if test.verdict.is_none() && submitted && !test.typed.trim().is_empty() {
                    checked = Some(gap.accepts(&test.typed));
                }
            }
            (0, Exercise::PickWord(choice)) => {
                ui::card(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(theme::label("Which word means this?").color(p.ink2));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&choice.prompt)
                                .size(20.0)
                                .family(theme::semibold()),
                        )
                        .wrap(),
                    );
                });
                if let Some(i) = pick(ui, &choice, test.picked) {
                    test.picked = Some(i);
                    checked = Some(i == choice.answer);
                }
            }
            _ => {
                let second = test.second.clone();
                ui::card(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(theme::label("What does it mean?").color(p.ink2));
                        ui.label(theme::display(word.text));
                    });
                });
                if let Some(i) = pick(ui, &second, test.picked) {
                    test.picked = Some(i);
                    checked = Some(i == second.answer);
                }
            }
        }
    });
    if let Some(right) = checked
        && test.verdict.is_none()
    {
        test.verdict = Some(right);
    }
    false
}

/// A gap-fill's sentence, the gap filled in once answered.
fn fill_question(ui: &mut egui::Ui, gap: &Cloze, verdict: Option<bool>) {
    let p = ui::palette(ui);
    ui::card(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(theme::label("Fill in the missing word").color(p.ink2));
        let middle = match verdict {
            None => gap.blank(),
            Some(_) => gap.answer.clone(),
        };
        ui.add(
            egui::Label::new(
                RichText::new(format!("{}{}{}", gap.before, middle, gap.after))
                    .size(20.0)
                    .family(theme::semibold()),
            )
            .wrap(),
        );
    });
}

/// Four options. Returns the one picked this frame, if any.
fn pick(ui: &mut egui::Ui, choice: &Choice, picked: Option<usize>) -> Option<usize> {
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
    chose
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::Dict;

    #[test]
    fn the_usage_block_starts_folded_away_on_every_word() {
        // "Learn more…" is the affordance that opens it, so a freshly opened
        // word must never arrive already unfolded.
        let mut state = LookupState::default();
        let dict = Dict::load();
        state.show_usage = true;
        state.open(dict.exact("run").unwrap().id);
        assert!(!state.show_usage);

        state.show_usage = true;
        state.open(dict.exact("decision").unwrap().id);
        assert!(!state.show_usage, "carried over from the previous word");
    }
}
