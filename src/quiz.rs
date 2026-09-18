//! Question building, shared by the placement test (spec 2.2) and study
//! sessions (spec 3.3).
//!
//! Both specs ask for the same thing of a distractor: "cùng từ loại, cùng dải
//! rank" — same part of speech, same frequency band. A distractor from a
//! different band gives the answer away, because the odd one out is simply the
//! word the learner has never seen.

use std::collections::HashSet;

use crate::dict::{Dict, Sense, SenseId};
use crate::rng::Rng;

/// How far either side of the target a distractor may be drawn from.
const BAND: u32 = 2_000;
/// Give up looking for a well-matched distractor after this many draws.
const DRAWS: usize = 400;

/// A multiple-choice question.
#[derive(Clone, Debug)]
pub struct Choice {
    /// What is being asked about — a word, or a definition.
    pub prompt: String,
    pub options: Vec<String>,
    /// Index into `options`.
    pub answer: usize,
}

/// A fill-in-the-blank built from an example sentence (spec 3.3, level 2).
#[derive(Clone, Debug)]
pub struct Cloze {
    pub before: String,
    pub after: String,
    /// The word that belongs in the gap, as the sentence spells it.
    pub answer: String,
    /// First letter, the hint spec 3.3 asks for.
    pub hint: char,
}

impl Cloze {
    /// The gap as it is drawn, sized to the answer.
    pub fn blank(&self) -> String {
        format!(
            "{}{}",
            self.hint,
            "_".repeat(self.answer.chars().count().saturating_sub(1))
        )
    }

    /// Accepts the answer regardless of case or surrounding punctuation.
    pub fn accepts(&self, typed: &str) -> bool {
        let tidy = |s: &str| {
            s.trim()
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        };
        !tidy(typed).is_empty() && tidy(typed) == tidy(&self.answer)
    }
}

/// Senses that would give the game away if used as a distractor for `target`:
/// another sense of the same word, or a definition that reads the same.
fn conflicts(target: &Sense, candidate: &Sense) -> bool {
    candidate.id == target.id
        || candidate.word == target.word
        || candidate.def == target.def
        || candidate.is_inflection
        || candidate.offensive
}

/// Draws `n` distinct distractor senses near `target`.
///
/// Two passes: same part of speech first, then any, so a question is always
/// produced even for a part of speech with few members.
fn distractors(dict: &Dict, rng: &mut Rng, target: &Sense, n: usize) -> Vec<Sense> {
    let lo = target.rank.saturating_sub(BAND).max(1);
    let hi = (target.rank + BAND).min(dict.learn_count());
    let span = (hi - lo + 1) as usize;

    let mut out: Vec<Sense> = Vec::with_capacity(n);
    let mut seen: HashSet<&str> = HashSet::from([target.def]);
    for pass in 0..2 {
        for _ in 0..DRAWS {
            if out.len() == n {
                return out;
            }
            let Some(s) = dict.at_rank(lo + rng.below(span) as u32) else {
                continue;
            };
            if conflicts(target, &s) || !seen.insert(s.def) {
                continue;
            }
            if pass == 0 && s.pos != target.pos {
                seen.remove(s.def);
                continue;
            }
            out.push(s);
        }
    }
    out
}

/// Shuffles the right answer in among the wrong ones.
fn assemble(rng: &mut Rng, prompt: String, correct: String, wrong: Vec<String>) -> Choice {
    let mut options = wrong;
    options.push(correct.clone());
    rng.shuffle(&mut options);
    let answer = options.iter().position(|o| *o == correct).unwrap_or(0);
    Choice {
        prompt,
        options,
        answer,
    }
}

/// "What does *word* mean?" — spec 3.3 level 1, and the placement test's only
/// question shape.
pub fn meaning_choice(
    dict: &Dict,
    rng: &mut Rng,
    target: &Sense,
    options: usize,
) -> Option<Choice> {
    let wrong = distractors(dict, rng, target, options - 1);
    if wrong.len() < options - 1 {
        return None;
    }
    Some(assemble(
        rng,
        dict.word(target.word).text.to_owned(),
        target.def.to_owned(),
        wrong.iter().map(|s| s.def.to_owned()).collect(),
    ))
}

/// The other direction: given the meaning, pick the word. Used when the sense
/// has no example sentence to build a gap-fill from.
pub fn word_choice(dict: &Dict, rng: &mut Rng, target: &Sense, options: usize) -> Option<Choice> {
    let wrong = distractors(dict, rng, target, options - 1);
    if wrong.len() < options - 1 {
        return None;
    }
    Some(assemble(
        rng,
        target.def.to_owned(),
        dict.word(target.word).text.to_owned(),
        wrong
            .iter()
            .map(|s| dict.word(s.word).text.to_owned())
            .collect(),
    ))
}

/// Blanks the target word out of its example sentence.
///
/// Returns `None` when the sense has no example, or when the example does not
/// actually contain the headword — which happens often enough in the source
/// data that the caller must have a fallback.
pub fn cloze(dict: &Dict, target: &Sense) -> Option<Cloze> {
    let sentence = target.example;
    if sentence.is_empty() {
        return None;
    }
    let word = dict.word(target.word).text;
    let (hay, needle) = (sentence.to_lowercase(), word.to_lowercase());
    // Only on a word boundary: blanking the "cat" inside "catalogue" would be
    // unanswerable. The two ends are checked separately — the character *at*
    // the start of a match is the word's own first letter.
    let free_before = |at: usize| {
        hay[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    };
    let free_after = |end: usize| {
        hay[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
    };
    let at = hay
        .match_indices(&needle)
        .find(|(i, m)| free_before(*i) && free_after(i + m.len()))
        .map(|(i, _)| i)?;
    // `to_lowercase` can change byte lengths, so slice the original by the
    // needle's length in the *original* string rather than trusting the index.
    let end = at + needle.len();
    if !sentence.is_char_boundary(at) || !sentence.is_char_boundary(end) {
        return None;
    }
    let answer = sentence[at..end].to_owned();
    Some(Cloze {
        before: sentence[..at].to_owned(),
        after: sentence[end..].to_owned(),
        hint: answer.chars().next()?,
        answer,
    })
}

/// Senses whose example sentence the user has already been shown.
///
/// Spec 1.4 is firm about this: a Quick Test that re-uses the sentence just
/// read on screen is one the user cannot get wrong.
#[derive(Default, Debug)]
pub struct Shown(HashSet<SenseId>);

impl Shown {
    pub fn mark(&mut self, sense: SenseId) {
        self.0.insert(sense);
    }

    pub fn contains(&self, sense: SenseId) -> bool {
        self.0.contains(&sense)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        Dict::load()
    }

    #[test]
    fn a_meaning_question_has_one_right_answer() {
        let d = dict();
        let mut rng = Rng::seeded(1);
        for rank in [1, 250, 3_000, 17_000] {
            let target = d.at_rank(rank).unwrap();
            let q = meaning_choice(&d, &mut rng, &target, 4).expect("built");
            assert_eq!(q.options.len(), 4);
            assert_eq!(q.options[q.answer], target.def);
            let unique: HashSet<&String> = q.options.iter().collect();
            assert_eq!(unique.len(), 4, "duplicate options: {:?}", q.options);
        }
    }

    #[test]
    fn distractors_share_the_part_of_speech_and_the_band() {
        let d = dict();
        let mut rng = Rng::seeded(2);
        let target = d.at_rank(1_500).unwrap();
        let wrong = distractors(&d, &mut rng, &target, 3);
        assert_eq!(wrong.len(), 3);
        for s in wrong {
            assert_eq!(s.pos, target.pos);
            assert!(
                s.rank.abs_diff(target.rank) <= BAND,
                "rank {} vs {}",
                s.rank,
                target.rank
            );
            assert_ne!(s.word, target.word);
        }
    }

    #[test]
    fn a_word_question_offers_distinct_words() {
        let d = dict();
        let mut rng = Rng::seeded(3);
        let target = d.at_rank(900).unwrap();
        let q = word_choice(&d, &mut rng, &target, 4).expect("built");
        assert_eq!(q.options[q.answer], d.word(target.word).text);
        assert_eq!(q.options.iter().collect::<HashSet<_>>().len(), 4);
    }

    #[test]
    fn the_answer_is_not_always_in_the_same_slot() {
        let d = dict();
        let mut rng = Rng::seeded(4);
        let target = d.at_rank(600).unwrap();
        let slots: HashSet<usize> = (0..40)
            .filter_map(|_| meaning_choice(&d, &mut rng, &target, 4))
            .map(|q| q.answer)
            .collect();
        assert!(slots.len() >= 3, "answer landed in {slots:?} only");
    }

    #[test]
    fn cloze_blanks_the_word_out_of_its_example() {
        let d = dict();
        // Find a learning item that has a usable example.
        let target = (1..4_000)
            .filter_map(|r| d.at_rank(r))
            .find_map(|s| cloze(&d, &s).map(|c| (s, c)));
        let (sense, gap) = target.expect("some item has a usable example");
        assert_eq!(
            format!("{}{}{}", gap.before, gap.answer, gap.after),
            sense.example
        );
        assert!(gap.accepts(&gap.answer));
        assert!(gap.accepts(&gap.answer.to_uppercase()));
        assert!(gap.accepts(&format!(" {}.", gap.answer)));
        assert!(!gap.accepts("definitelynotit"));
        assert!(!gap.accepts(""));
        assert_eq!(gap.blank().chars().count(), gap.answer.chars().count());
    }

    #[test]
    fn cloze_declines_when_the_example_lacks_the_word() {
        let d = dict();
        // "leaf" is not a standalone word in "a leaflet", so no gap-fill.
        let mut checked = 0;
        for rank in 1..6_000 {
            let Some(s) = d.at_rank(rank) else { continue };
            if s.example.is_empty() {
                assert!(cloze(&d, &s).is_none());
                checked += 1;
            }
        }
        assert!(checked > 0, "expected some senses with no example");
    }

    #[test]
    fn shown_examples_are_remembered() {
        let mut shown = Shown::default();
        assert!(!shown.contains(7));
        shown.mark(7);
        assert!(shown.contains(7));
    }
}
