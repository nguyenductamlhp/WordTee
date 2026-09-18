//! The adaptive placement test of spec 2.2: find the user's Vocabulary
//! Frontier — the rank past which words stop being familiar.
//!
//! Spec 2.2 rejects v1's binary search, because one lucky guess out of four
//! options sends it off course with no way back. This is the replacement it
//! asks for: a computerised adaptive test over a one-parameter IRT (Rasch)
//! model.
//!
//! ```text
//! P(known | θ, b) = 1 / (1 + e^-(θ − b))      b = ln(sense_rank)
//! ```
//!
//! θ and b share one scale — natural log of rank — so θ reads directly as "the
//! rank where this user is a coin flip", and the frontier follows in closed
//! form. Every answer refits θ by Newton's method; the next item is whichever
//! is closest to the current θ, which is where an answer is most informative.

use crate::dict::{Dict, SenseId};
use crate::quiz::{self, Choice};
use crate::rng::Rng;
use crate::search;

/// Spec 2.2: stop at 25 questions, never before 15.
pub const MIN_ITEMS: usize = 15;
pub const MAX_ITEMS: usize = 25;
/// …or earlier, once θ is pinned down this tightly.
pub const SE_TARGET: f32 = 0.35;
/// Spec 2.2: one pseudo-word for every five real ones.
pub const TRAP_EVERY: usize = 5;
/// Spec 2.2: a false-alarm rate past this means the user is guessing, so the
/// test warns them and asks more.
pub const FALSE_ALARM_LIMIT: f32 = 0.5;
/// Extra questions granted when that happens.
const EXTRA_ITEMS: usize = 6;
/// Spec 2.2: the frontier is the last rank the user still knows 80% of the
/// time. In the Rasch model P = 0.8 exactly when θ − b = ln 4.
const KNOWN_ODDS: f32 = 1.386_294_4;
/// Spec 2.2: the warm-up ranks.
const SEED_RANKS: [u32; 3] = [1_000, 3_000, 8_000];
const OPTIONS: usize = 4;

/// One asked-and-answered real item.
struct Answer {
    /// Difficulty, ln(rank).
    b: f32,
    known: bool,
}

/// The question on screen.
pub struct Asked {
    /// `None` for a pseudo-word: there is no right answer.
    pub sense: Option<SenseId>,
    pub choice: Choice,
}

/// What the test concluded (spec 2.2, "Kết quả đầu ra").
#[derive(Clone, Copy, Debug)]
pub struct Verdict {
    /// Largest rank the user knows with probability >= 0.8.
    pub frontier: u32,
    pub theta: f32,
    /// Raw share of real items answered correctly.
    pub raw_rate: f32,
    /// …after the guessing correction below.
    pub corrected_rate: f32,
    /// Share of pseudo-words the user claimed to know.
    pub false_alarm: f32,
    pub asked: usize,
}

impl Verdict {
    /// Estimated share of a 1.000-item block that is already known, for the
    /// map's opening picture.
    pub fn known_share(&self, block_start: u32) -> f32 {
        let b = ((block_start + 500) as f32).ln();
        1.0 / (1.0 + (-(self.theta - b)).exp())
    }
}

/// A placement test in progress.
pub struct Placement {
    rng: Rng,
    answers: Vec<Answer>,
    /// One entry per pseudo-word shown: did the user claim to know it?
    traps: Vec<bool>,
    asked_senses: Vec<SenseId>,
    theta: f32,
    current: Option<Asked>,
    limit: usize,
    /// Set once the false-alarm rate crosses the limit, so the UI can say so.
    pub warned: bool,
}

impl Placement {
    pub fn new(dict: &Dict) -> Self {
        let mut test = Self {
            rng: Rng::new(),
            answers: Vec::new(),
            traps: Vec::new(),
            asked_senses: Vec::new(),
            // ln(3000): start in the middle of the list.
            theta: (3_000f32).ln(),
            current: None,
            limit: MAX_ITEMS,
            warned: false,
        };
        test.advance(dict);
        test
    }

    pub fn question(&self) -> Option<&Asked> {
        self.current.as_ref()
    }

    /// Questions answered so far, pseudo-words included.
    pub fn asked(&self) -> usize {
        self.answers.len() + self.traps.len()
    }

    /// A 0..1 estimate of how far along the test is, for a progress bar.
    pub fn progress(&self) -> f32 {
        (self.asked() as f32 / self.limit as f32).min(1.0)
    }

    pub fn finished(&self) -> bool {
        self.current.is_none()
    }

    /// Records an answer. `pick` is `None` when the user pressed "I don't
    /// know", which spec 2.2 requires as an option so that not-knowing does
    /// not have to be expressed as a guess.
    pub fn answer(&mut self, dict: &Dict, pick: Option<usize>) {
        let Some(asked) = self.current.take() else {
            return;
        };
        match asked.sense {
            Some(sense) => {
                let rank = dict.sense(sense).rank.max(1);
                self.answers.push(Answer {
                    b: (rank as f32).ln(),
                    known: pick == Some(asked.choice.answer),
                });
                self.refit();
            }
            // A pseudo-word: any answer at all is a false alarm, because there
            // was nothing there to know.
            None => self.traps.push(pick.is_some()),
        }
        if self.false_alarm() > FALSE_ALARM_LIMIT && !self.warned {
            self.warned = true;
            self.limit = (self.limit + EXTRA_ITEMS).min(MAX_ITEMS + EXTRA_ITEMS);
        }
        self.advance(dict);
    }

    /// The result so far. Usable before the test ends, for a live preview.
    pub fn verdict(&self) -> Verdict {
        let raw = if self.answers.is_empty() {
            0.0
        } else {
            self.answers.iter().filter(|a| a.known).count() as f32 / self.answers.len() as f32
        };
        let fa = self.false_alarm();
        // Spec 2.2: true rate = (H − FA) / (1 − FA).
        let corrected = if fa >= 1.0 {
            0.0
        } else {
            ((raw - fa) / (1.0 - fa)).clamp(0.0, 1.0)
        };
        // The same correction is applied to the frontier: if a third of the
        // "known" answers were luck, the frontier they imply is too generous.
        let shrink = if raw > 0.0 { corrected / raw } else { 1.0 };
        let frontier = ((self.theta - KNOWN_ODDS).exp() * shrink).round();
        Verdict {
            frontier: (frontier.max(0.0) as u32).min(25_000),
            theta: self.theta,
            raw_rate: raw,
            corrected_rate: corrected,
            false_alarm: fa,
            asked: self.asked(),
        }
    }

    fn false_alarm(&self) -> f32 {
        if self.traps.is_empty() {
            return 0.0;
        }
        self.traps.iter().filter(|t| **t).count() as f32 / self.traps.len() as f32
    }

    /// One Newton step towards the maximum-likelihood θ.
    ///
    /// All-correct or all-wrong runs have no finite maximum, so the step is
    /// capped and θ is kept inside the range the learning list covers.
    fn refit(&mut self) {
        for _ in 0..12 {
            let (mut score, mut info) = (0.0f32, 0.0f32);
            for a in &self.answers {
                let p = 1.0 / (1.0 + (-(self.theta - a.b)).exp());
                score += f32::from(a.known) - p;
                info += p * (1.0 - p);
            }
            if info < 1e-4 {
                // No information yet: nudge in the direction of the evidence.
                self.theta += score.clamp(-1.0, 1.0);
                break;
            }
            let step = (score / info).clamp(-1.5, 1.5);
            self.theta += step;
            if step.abs() < 1e-3 {
                break;
            }
        }
        // ln(1) .. ln(25000), the span the learning list actually covers.
        self.theta = self.theta.clamp(0.0, 10.13);
    }

    fn standard_error(&self) -> f32 {
        let info: f32 = self
            .answers
            .iter()
            .map(|a| {
                let p = 1.0 / (1.0 + (-(self.theta - a.b)).exp());
                p * (1.0 - p)
            })
            .sum();
        if info <= 0.0 {
            f32::INFINITY
        } else {
            1.0 / info.sqrt()
        }
    }

    /// Chooses the next question, or ends the test.
    fn advance(&mut self, dict: &Dict) {
        let asked = self.asked();
        let done = asked >= self.limit
            || (self.answers.len() >= MIN_ITEMS && self.standard_error() < SE_TARGET);
        if done {
            self.current = None;
            return;
        }
        // Spec 2.2: a pseudo-word after every five real items.
        if !self.answers.is_empty()
            && self.answers.len().is_multiple_of(TRAP_EVERY)
            && self.traps.len() < self.answers.len() / TRAP_EVERY
            && let Some(trap) = self.build_trap(dict)
        {
            self.current = Some(trap);
            return;
        }
        self.current = self.build_real(dict);
    }

    /// The next real item: warm-up ranks first, then whichever rank sits
    /// closest to the current θ, which is where the answer says most.
    fn build_real(&mut self, dict: &Dict) -> Option<Asked> {
        let target = match SEED_RANKS.get(self.answers.len()) {
            Some(&rank) => rank,
            None => (self.theta.exp().round().max(1.0) as u32).min(dict.learn_count()),
        };
        // Walk outwards from the target rank until an unasked, askable item
        // turns up — some ranks have too few same-part-of-speech neighbours to
        // build four options from.
        for step in 0..600u32 {
            for rank in [target.saturating_add(step), target.saturating_sub(step)] {
                let Some(sense) = dict.at_rank(rank.max(1)) else {
                    continue;
                };
                if self.asked_senses.contains(&sense.id) || sense.offensive {
                    continue;
                }
                if let Some(choice) = quiz::meaning_choice(dict, &mut self.rng, &sense, OPTIONS) {
                    self.asked_senses.push(sense.id);
                    return Some(Asked {
                        sense: Some(sense.id),
                        choice,
                    });
                }
            }
        }
        None
    }

    /// Builds a pseudo-word question (spec 2.2's trap).
    fn build_trap(&mut self, dict: &Dict) -> Option<Asked> {
        let fake = self.invent_word(dict)?;
        // Four real definitions from around the middle of the test's range —
        // all of them wrong, since the word does not exist.
        let anchor =
            dict.at_rank((self.theta.exp().round().max(1.0) as u32).min(dict.learn_count()))?;
        let mut options = Vec::with_capacity(OPTIONS);
        for _ in 0..200 {
            if options.len() == OPTIONS {
                break;
            }
            let rank = 1 + self.rng.below(dict.learn_count() as usize) as u32;
            if let Some(s) = dict.at_rank(rank)
                && s.pos == anchor.pos
                && !s.offensive
                && !options.contains(&s.def.to_owned())
            {
                options.push(s.def.to_owned());
            }
        }
        (options.len() == OPTIONS).then_some(Asked {
            sense: None,
            // `answer` is never right for a trap; it just has to be in range.
            choice: Choice {
                prompt: fake,
                options,
                answer: usize::MAX,
            },
        })
    }

    /// Invents a word that looks English but is not one.
    ///
    /// Spec 2.2's filter: it must not be a real word, and must differ from
    /// every real word by at least two characters.
    fn invent_word(&mut self, dict: &Dict) -> Option<String> {
        const ONSET: [&str; 24] = [
            "b", "c", "d", "f", "g", "h", "j", "k", "l", "m", "n", "p", "r", "s", "t", "v", "w",
            "br", "cl", "dr", "fl", "gr", "st", "tr",
        ];
        const VOWEL: [&str; 10] = ["a", "e", "i", "o", "u", "ai", "ea", "oo", "ou", "au"];
        const CODA: [&str; 14] = [
            "b", "ck", "d", "g", "l", "m", "n", "p", "r", "sh", "st", "t", "nt", "rn",
        ];
        const TAIL: [&str; 6] = ["", "", "le", "en", "er", "ish"];

        for _ in 0..40 {
            let mut word = String::new();
            for syllable in 0..2 {
                word.push_str(self.rng.choice(&ONSET)?);
                word.push_str(self.rng.choice(&VOWEL)?);
                if syllable == 1 {
                    word.push_str(self.rng.choice(&CODA)?);
                }
            }
            word.push_str(self.rng.choice(&TAIL)?);
            // The expensive check runs last, and only on a shape that survived.
            if dict.exact(&word).is_none() && !search::near_a_real_word(dict, &word) {
                return Some(word);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        Dict::load()
    }

    /// Answers as someone who knows everything up to `known_to` and nothing
    /// past it, and never guesses on a pseudo-word.
    fn simulate(dict: &Dict, known_to: u32) -> Verdict {
        let mut test = Placement::new(dict);
        let mut guard = 0;
        while let Some(asked) = test.question() {
            guard += 1;
            assert!(guard < 200, "placement did not terminate");
            let pick = match asked.sense {
                None => None, // honest: does not claim to know a fake word
                Some(sense) => (dict.sense(sense).rank <= known_to).then_some(asked.choice.answer),
            };
            test.answer(dict, pick);
        }
        test.verdict()
    }

    #[test]
    fn a_question_is_always_answerable() {
        let d = dict();
        let mut test = Placement::new(&d);
        while let Some(asked) = test.question() {
            assert_eq!(asked.choice.options.len(), OPTIONS);
            if let Some(sense) = asked.sense {
                assert!(asked.choice.answer < OPTIONS);
                assert_eq!(
                    asked.choice.options[asked.choice.answer],
                    d.sense(sense).def
                );
            }
            let pick = asked.choice.answer.checked_rem(OPTIONS);
            test.answer(&d, pick);
        }
        assert!(test.finished());
    }

    #[test]
    fn it_stops_inside_the_specs_bounds() {
        let d = dict();
        for known_to in [500, 5_000, 20_000] {
            let v = simulate(&d, known_to);
            assert!(v.asked >= MIN_ITEMS, "{known_to}: only {} asked", v.asked);
            assert!(
                v.asked <= MAX_ITEMS + EXTRA_ITEMS,
                "{known_to}: {} asked",
                v.asked
            );
        }
    }

    #[test]
    fn the_frontier_tracks_the_simulated_vocabulary() {
        let d = dict();
        let mut last = 0;
        for known_to in [1_000u32, 3_000, 8_000, 15_000] {
            let v = simulate(&d, known_to);
            // The frontier is deliberately conservative, and by a known
            // factor: θ lands on the 50% rank, and the frontier is the 80%
            // rank, which in the Rasch model is a quarter of it. Spec 2.2
            // wants exactly that — the 0,8–0,95 band above it is what Quick
            // Scan sweeps up, rather than being assumed known outright.
            let ratio = v.frontier as f32 / known_to as f32;
            assert!(
                (0.1..1.2).contains(&ratio),
                "knew {known_to}, guessed {}",
                v.frontier
            );
            assert!(v.frontier > last, "frontier did not grow at {known_to}");
            last = v.frontier;
        }
    }

    #[test]
    fn traps_are_shown_and_cost_nothing_when_answered_honestly() {
        let d = dict();
        let mut test = Placement::new(&d);
        let mut traps = 0;
        while let Some(asked) = test.question() {
            let pick = match asked.sense {
                None => {
                    traps += 1;
                    None
                }
                Some(_) => Some(asked.choice.answer),
            };
            test.answer(&d, pick);
        }
        assert!(traps >= 2, "only {traps} pseudo-words in a full test");
        let v = test.verdict();
        assert_eq!(v.false_alarm, 0.0);
        assert!((v.corrected_rate - v.raw_rate).abs() < 1e-5);
    }

    #[test]
    fn guessing_is_caught_and_corrected_for() {
        // Someone who clicks an option every single time, pseudo-words too.
        let d = dict();
        let mut test = Placement::new(&d);
        while let Some(asked) = test.question() {
            let pick = Some(asked.choice.answer.min(OPTIONS - 1));
            test.answer(&d, pick);
        }
        let v = test.verdict();
        assert_eq!(v.false_alarm, 1.0, "every pseudo-word was claimed as known");
        assert_eq!(v.corrected_rate, 0.0, "corrected rate should collapse");
        assert_eq!(v.frontier, 0, "a pure guesser gets no free frontier");
        assert!(test.warned);
    }

    #[test]
    fn pseudo_words_are_not_real_and_not_near_real() {
        let d = dict();
        let mut test = Placement::new(&d);
        for _ in 0..4 {
            let fake = test.invent_word(&d).expect("invented a word");
            assert!(d.exact(&fake).is_none(), "{fake} is a real headword");
            assert!(
                !search::near_a_real_word(&d, &fake),
                "{fake} is one edit from a real word"
            );
            assert!(fake.chars().all(|c| c.is_ascii_lowercase()), "{fake}");
        }
    }

    #[test]
    fn knowing_nothing_gives_the_lowest_frontier() {
        let d = dict();
        let v = simulate(&d, 0);
        assert!(v.frontier < 200, "frontier {} for a beginner", v.frontier);
        assert_eq!(v.raw_rate, 0.0);
    }

    #[test]
    fn the_known_share_curve_falls_with_rank() {
        let d = dict();
        let v = simulate(&d, 5_000);
        let mut last = 1.0;
        for block in (0..25_000).step_by(1_000) {
            let share = v.known_share(block);
            assert!((0.0..=1.0).contains(&share));
            assert!(share <= last, "share rose at block {block}");
            last = share;
        }
    }
}
