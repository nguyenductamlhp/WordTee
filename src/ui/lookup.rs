//! Look up — the search box, the results list and the word page.
//!
//! This is spec 1's "Acquisition Funnel": looking a word up is where learning
//! starts, so every sense on the page carries its own state and its own three
//! buttons (spec 1.4).

use eframe::egui::{self, RichText};

use crate::app::Ctx;
use crate::dict::{Kind, Relation, Sense, SenseId, WordId};
use crate::progress::{Source, State};
use crate::quiz::{self, Choice, Cloze};
use crate::search::{self, Results, Tier};
use crate::study::{self, Exercise};
use crate::ui;

/// How many headwords the results list shows.
const RESULT_LIMIT: usize = 40;

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
    /// Still on track for "Known" — set false by the first wrong answer.
    perfect: bool,
}

#[derive(Default)]
pub struct LookupState {
    query: String,
    /// The query the current `results` were computed for.
    searched: String,
    results: Results,
    /// The word page, when one is open.
    open: Option<WordId>,
    /// Which sense the action bar acts on (spec 1.4: "nghĩa đang xem").
    focus: usize,
    /// Spec 1.3: opening a word from a link pushes onto this, so Back returns
    /// to where you were.
    back: Vec<WordId>,
    test: Option<QuickTest>,
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
    }

    fn close(&mut self) {
        self.open = self.back.pop();
        self.focus = 0;
        self.test = None;
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
    ui.add_space(6.0);
    let field = ui.add_sized(
        [ui.available_width(), 38.0],
        egui::TextEdit::singleline(&mut state.query)
            .hint_text("Search in English, or type Vietnamese with tone marks…")
            .font(egui::TextStyle::Heading),
    );
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
    if state.results.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Nothing found.").color(ui::muted(ui)));
            ui.label(
                RichText::new("Try it without tone marks, or check the spelling.")
                    .size(12.0)
                    .color(ui::muted(ui)),
            );
        });
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut open = None;
        for hit in &state.results.words {
            let word = ctx.dict.word(hit.word);
            let response = ui::card(ui, None, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(word.text).size(17.0).strong());
                    if !word.ipa.is_empty() {
                        ui.label(RichText::new(word.ipa).size(13.0).color(ui::muted(ui)));
                    }
                    if word.kind == Kind::Phrase {
                        ui::chip(ui, "phrase", ui::accent(ui));
                    }
                    if hit.tier != Tier::Exact {
                        ui::chip(ui, hit.tier.label(), ui::muted(ui));
                    }
                });
                // Spec 1.1: a lemma reached through an inflected form says so.
                if let Some(form) = &hit.form {
                    ui.label(
                        RichText::new(format!(
                            "“{}” is the {} of this word",
                            form.surface, form.tag
                        ))
                        .size(12.0)
                        .color(ui::accent(ui)),
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
                    ui.label(
                        RichText::new(gloss.join(" · "))
                            .size(13.5)
                            .color(ui::muted(ui)),
                    );
                }
            });
            if ui::card_clicked(ui, &response) {
                open = Some(hit.word);
            }
            ui.add_space(4.0);
        }

        // Spec 5.2: the Vietnamese → English direction.
        if !state.results.reverse.is_empty() {
            ui::section(ui, "Vietnamese to English", |ui| {
                for &id in &state.results.reverse {
                    let sense = ctx.dict.sense(id);
                    let word = ctx.dict.word(sense.word);
                    let response = ui::card(ui, None, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(word.text).size(16.0).strong());
                            ui::pos_chip(ui, sense.pos);
                            ui::band_chip(ui, sense.band(), sense.rank);
                        });
                        ui.label(RichText::new(sense.def).size(13.5).color(ui::muted(ui)));
                    });
                    if ui::card_clicked(ui, &response) {
                        open = Some(sense.word);
                    }
                    ui.add_space(4.0);
                }
            });
        }
        if let Some(word) = open {
            state.open(word);
        }
    });
}

/// What the search tab shows before anything is typed.
fn idle_hint(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState) {
    ui.add_space(14.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(crate::app::APP_NAME).size(26.0).strong());
        ui.label(
            RichText::new(format!(
                "{} headwords, {} senses — works with no connection",
                ui::thousands(ctx.dict.word_count()),
                ui::thousands(ctx.dict.sense_count())
            ))
            .size(13.0)
            .color(ui::muted(ui)),
        );
    });
    ui.add_space(10.0);
    ui::section(ui, "Try one", |ui| {
        ui.horizontal_wrapped(|ui| {
            for word in ["decision", "swimming", "teh", "run", "quyết định"] {
                if ui.button(word).clicked() {
                    state.query = word.to_owned();
                }
            }
        });
        ui.label(
            RichText::new(
                "Typos still find the word (teh finds the), so do inflected \
                 forms (swimming finds swim), and Vietnamese with tone marks \
                 searches the definitions instead.",
            )
            .size(12.0)
            .color(ui::muted(ui)),
        );
    });
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

    // --- header, fixed (spec 1.3) ---
    egui::Panel::top("word-header").show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("‹").clicked() {
                state.close();
            }
            ui.label(RichText::new(word.text).size(26.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui::speak_buttons(ui, word.text);
            });
        });
        ui.horizontal_wrapped(|ui| {
            if !word.ipa.is_empty() {
                ui.label(RichText::new(word.ipa).size(15.0).color(ui::accent(ui)));
            }
            if let Some(sense) = meanings.get(state.focus) {
                ui::band_chip(ui, sense.band(), sense.rank);
            }
            if word.kind == Kind::Phrase {
                ui::chip(ui, "phrase", ui::accent(ui));
            }
            if word.offensive {
                ui::chip(ui, "coarse — lookup only", ui::bad(ui));
            }
        });
        // Spec 1.1: "cũng là dạng của …".
        let forms = search::forms_of(ctx.dict, word.norm);
        if !forms.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("also").size(12.0).color(ui::muted(ui)));
                for (lemma, tag) in forms {
                    if ui
                        .link(RichText::new(format!("{} of {}", tag, lemma.text)).size(12.0))
                        .clicked()
                    {
                        state.open(lemma.id);
                    }
                }
            });
        }
        ui.add_space(4.0);
    });

    // --- action bar, fixed (spec 1.4) ---
    let focused = meanings.get(state.focus).copied();
    if let Some(sense) = focused {
        action_bar(ui, ctx, state, &sense);
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        if meanings.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("This entry is only an inflected form of another word.")
                    .color(ui::muted(ui)),
            );
        }
        // --- sense cards (spec 1.2: one sense, one learning item) ---
        for (i, sense) in meanings.iter().enumerate() {
            let selected = i == state.focus;
            let state_now = ctx.progress.state(sense);
            let response = ui::card(ui, selected.then_some(ui::accent(ui)), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("{}.", i + 1))
                            .strong()
                            .color(ui::muted(ui)),
                    );
                    ui::pos_chip(ui, sense.pos);
                    ui::state_chip(ui, state_now);
                    if sense.rank > 0 {
                        ui::band_chip(ui, sense.band(), sense.rank);
                    }
                });
                ui.label(RichText::new(sense.def).size(15.5));
                if !sense.example.is_empty() {
                    ui.add_space(2.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(sense.example)
                                .size(13.5)
                                .italics()
                                .color(ui::muted(ui)),
                        );
                        ui::speak_buttons(ui, sense.example);
                    });
                    // Spec 1.4: remember we showed it, so no test re-uses it.
                    ctx.shown.mark(sense.id);
                }
            });
            if ui::card_clicked(ui, &response) {
                state.focus = i;
            }
            ui.add_space(4.0);
        }

        usage_block(ui, ctx, state, id);
        ui.add_space(60.0);
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
    if relations.is_empty() && phrases.is_empty() {
        return;
    }

    ui::section(ui, "Usage", |ui| {
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
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("{}:", kind.label()))
                        .size(12.5)
                        .color(ui::muted(ui)),
                );
                for text in words {
                    // Only link the ones that are actually in the dictionary.
                    match ctx.dict.exact(&search::normalize(text)) {
                        Some(target) => {
                            if ui.link(RichText::new(text).size(13.0)).clicked() {
                                state.open(target.id);
                            }
                        }
                        None => {
                            ui.label(RichText::new(text).size(13.0).color(ui::muted(ui)));
                        }
                    }
                }
            });
        }
        if !phrases.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Phrases:").size(12.5).color(ui::muted(ui)));
                for phrase in phrases {
                    if ui.link(RichText::new(phrase.text).size(13.0)).clicked() {
                        state.open(phrase.id);
                    }
                }
            });
        }
    });
}

/// The three buttons of spec 1.4, pinned to the bottom of the word page.
fn action_bar(ui: &mut egui::Ui, ctx: &mut Ctx, state: &mut LookupState, sense: &Sense) {
    egui::Panel::bottom("actions").show(ui, |ui| {
        ui.add_space(5.0);
        let current = ctx.progress.state(sense);
        let accent = ui::accent(ui);
        ui.columns(3, |c| {
            if c[0]
                .add_sized(
                    [c[0].available_width(), 40.0],
                    egui::Button::new(RichText::new("I know this").size(13.5)),
                )
                .clicked()
            {
                let undo = ctx
                    .progress
                    .set_state(sense.id, State::Known, Source::Manual, ctx.day);
                ctx.say_undoable("Marked as known.", undo);
            }
            if c[1]
                .add_sized(
                    [c[1].available_width(), 40.0],
                    egui::Button::new(RichText::new("Quick test").size(13.5)),
                )
                .clicked()
            {
                state.test = Some(build_quick_test(ctx, sense));
            }
            let learn = egui::Button::new(
                RichText::new("Learn this")
                    .size(13.5)
                    .color(accent)
                    .strong(),
            );
            if c[2]
                .add_sized([c[2].available_width(), 40.0], learn)
                .clicked()
            {
                let undo = ctx
                    .progress
                    .start_learning(sense.id, Source::Manual, ctx.day);
                ctx.say_undoable("Added to your learning list.", undo);
            }
        });
        if current != State::Unexplored {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Selected sense:")
                        .size(11.5)
                        .color(ui::muted(ui)),
                );
                ui::state_chip(ui, current);
            });
        }
        ui.add_space(5.0);
    });
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
        perfect: true,
    }
}

/// Draws the Quick Test. Returns true when it is over and should be dropped.
fn quick_test_page(ui: &mut egui::Ui, ctx: &mut Ctx, test: &mut QuickTest) -> bool {
    let sense = ctx.dict.sense(test.sense);
    let word = ctx.dict.word(sense.word);
    let mut finish = false;

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Quick test").size(18.0).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Exit").clicked() {
                finish = true;
            }
            ui.label(RichText::new(format!("{}/2", (test.step + 1).min(2))).color(ui::muted(ui)));
        });
    });
    ui.add_space(6.0);

    match test.step {
        0 => {
            let answered = match &test.first {
                Exercise::Fill(gap) => fill_question(ui, gap, &mut test.typed),
                Exercise::PickWord(choice) => {
                    pick_question(ui, choice, &mut test.picked, "Which word means this?")
                }
                // No usable question: skip straight to the meaning check.
                _ => Some(true),
            };
            if let Some(correct) = answered {
                test.perfect &= correct;
                test.step = 1;
                test.picked = None;
            }
        }
        1 => {
            if test.second.options.is_empty() {
                test.step = 2;
            } else if let Some(correct) = pick_question(
                ui,
                &test.second,
                &mut test.picked,
                &format!("What does “{}” mean?", word.text),
            ) {
                test.perfect &= correct;
                test.step = 2;
            }
        }
        _ => {}
    }

    if test.step == 2 {
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
        finish = true;
    }
    finish
}

/// A gap-fill. Returns `Some(correct)` once the user submits.
fn fill_question(ui: &mut egui::Ui, gap: &Cloze, typed: &mut String) -> Option<bool> {
    ui::card(ui, None, |ui| {
        ui.label(
            RichText::new("Fill in the missing word:")
                .size(13.0)
                .color(ui::muted(ui)),
        );
        ui.add_space(4.0);
        ui.label(RichText::new(format!("{}{}{}", gap.before, gap.blank(), gap.after)).size(16.0));
    });
    ui.add_space(8.0);
    let field = ui.add_sized(
        [ui.available_width(), 38.0],
        egui::TextEdit::singleline(typed).hint_text(format!("starts with “{}”", gap.hint)),
    );
    let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let clicked = ui::wide_button(ui, "Answer", ui::accent(ui)).clicked();
    (submitted || clicked).then(|| gap.accepts(typed))
}

/// A four-option question. Returns `Some(correct)` once an option is picked.
fn pick_question(
    ui: &mut egui::Ui,
    choice: &Choice,
    picked: &mut Option<usize>,
    prompt: &str,
) -> Option<bool> {
    ui::card(ui, None, |ui| {
        ui.label(RichText::new(prompt).size(17.0).strong());
        if !choice.prompt.is_empty() && !prompt.contains(&choice.prompt) {
            ui.label(
                RichText::new(&choice.prompt)
                    .size(14.0)
                    .color(ui::muted(ui)),
            );
        }
    });
    ui.add_space(8.0);
    for (i, option) in choice.options.iter().enumerate() {
        let color = match *picked {
            Some(_) if i == choice.answer => ui::good(ui),
            Some(p) if p == i => ui::bad(ui),
            _ => ui.visuals().text_color(),
        };
        if ui::wide_button(ui, option, color).clicked() && picked.is_none() {
            *picked = Some(i);
        }
        ui.add_space(3.0);
    }
    let p = (*picked)?;
    ui.add_space(6.0);
    if ui::wide_button(ui, "Continue", ui::accent(ui)).clicked() {
        return Some(p == choice.answer);
    }
    None
}
