//! Choosing what to study, and what to ask about it.
//!
//! Three separate jobs the spec keeps apart:
//!
//! * **Smart Feeding** (spec 2.3) — which unexplored items to offer next.
//! * **The session** (spec 3.6) — overdue reviews, then new items, then a
//!   couple of spot-checks on things marked known.
//! * **The exercise** (spec 3.3) — what to actually ask, by level.

use crate::dict::{Dict, Relation, Sense, SenseId};
use crate::progress::{Day, FEED_WINDOW, Progress, State};
use crate::quiz::{self, Choice, Cloze, Shown};
use crate::rng::Rng;
use crate::search;

/// Relevance weights from spec 2.3. The spec marks these as hypotheses to be
/// settled by A/B test; these are the starting values.
///
/// `Goal` and `Topic` are zero on purpose. They need the goal packs of spec 5.1
/// (IELTS, TOEIC, Công sở) and topic labels per item, and neither exists in the
/// data this app ships — see the README. The two signals we can compute
/// honestly are whether the user looked the word up and whether it shares a
/// word family with something they already know.
const W_GOAL: f32 = 0.0;
const W_TOPIC: f32 = 0.0;
const W_LOOKUP: f32 = 0.6;
const W_FAMILY: f32 = 0.4;

/// One thing to do in a session.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Task {
    /// An item that is due (spec 3.6, first in the queue).
    Review(SenseId),
    /// A new item from Smart Feeding.
    New(SenseId),
    /// A spot-check on something marked known (spec 3.5).
    Verify(SenseId),
}

impl Task {
    pub fn sense(self) -> SenseId {
        match self {
            Self::Review(s) | Self::New(s) | Self::Verify(s) => s,
        }
    }
}

/// What to put in front of the user for one task (spec 3.3).
#[derive(Clone, Debug)]
pub enum Exercise {
    /// Level 1, first sight: the whole card, nothing to answer yet.
    Study,
    /// Level 1: pick the meaning out of four.
    Meaning(Choice),
    /// Level 2: fill the word into a sentence, first letter given.
    Fill(Box<Cloze>),
    /// Level 2 fallback, when no example sentence is usable: pick the word
    /// that matches a meaning.
    PickWord(Choice),
    /// Level 3: produce the word from its meaning, spelled out.
    Spell,
}

impl Exercise {
    /// The level this exercise counts as, for FSRS grading (spec 3.2).
    pub fn level(&self) -> u8 {
        match self {
            Self::Study | Self::Meaning(_) => 1,
            Self::Fill(_) | Self::PickWord(_) => 2,
            Self::Spell => 3,
        }
    }
}

/// Builds the exercise for one item at one level.
///
/// `shown` is what the user has already seen this session — spec 1.4 and 3.3
/// both insist an exercise must not re-use a sentence just read on screen.
pub fn exercise(dict: &Dict, rng: &mut Rng, sense: &Sense, level: u8, shown: &Shown) -> Exercise {
    match level {
        0 => Exercise::Study,
        1 => quiz::meaning_choice(dict, rng, sense, 4).map_or(Exercise::Study, Exercise::Meaning),
        2 => match quiz::cloze(dict, sense) {
            Some(gap) if !shown.contains(sense.id) => Exercise::Fill(Box::new(gap)),
            _ => quiz::word_choice(dict, rng, sense, 4).map_or(Exercise::Study, Exercise::PickWord),
        },
        _ => Exercise::Spell,
    }
}

/// Accepts a typed answer for [`Exercise::Spell`], ignoring case and padding.
pub fn spelling_accepts(dict: &Dict, sense: &Sense, typed: &str) -> bool {
    let want = search::normalize(dict.word(sense.word).text);
    !typed.trim().is_empty() && search::normalize(typed) == want
}

// -------------------------------------------------------------------------
// Smart Feeding (spec 2.3)
// -------------------------------------------------------------------------

/// Score for one candidate: `Priority = RankScore × Relevance`.
fn priority(dict: &Dict, progress: &Progress, sense: &Sense) -> f32 {
    // 1 at the near edge of the window, 0 at the far edge.
    let past = sense.rank.saturating_sub(progress.frontier) as f32;
    let rank_score = (1.0 - past / FEED_WINDOW as f32).clamp(0.0, 1.0);

    let looked_up = f32::from(progress.lookups.contains(&sense.word));
    let family = f32::from(shares_a_family(dict, progress, sense));
    let relevance = 1.0 + W_GOAL * 0.0 + W_TOPIC * 0.0 + W_LOOKUP * looked_up + W_FAMILY * family;
    rank_score * relevance
}

/// Is this item in the same word family as something already known?
fn shares_a_family(dict: &Dict, progress: &Progress, sense: &Sense) -> bool {
    dict.relations(sense.word)
        .into_iter()
        .filter(|(kind, _)| *kind == Relation::Derived)
        .filter_map(|(_, text)| dict.exact(&search::normalize(text)))
        .any(|word| {
            word.senses().any(|id| {
                matches!(
                    progress.state(&dict.sense(id)),
                    State::Known | State::Mastered
                )
            })
        })
}

/// Would putting `candidate` next to the already-chosen items cause memory
/// interference? Spec 2.3, step 3.
fn interferes(dict: &Dict, chosen: &[Sense], candidate: &Sense) -> bool {
    // Never two senses of the same headword in one session.
    if chosen.iter().any(|s| s.word == candidate.word) {
        return true;
    }
    // Never a synonym or antonym of something already picked: Tinkham and
    // Waring found that learning near-synonyms together makes both harder.
    let related: Vec<&str> = dict
        .relations(candidate.word)
        .into_iter()
        .filter(|(kind, _)| matches!(kind, Relation::Synonym | Relation::Antonym))
        .map(|(_, text)| text)
        .collect();
    chosen.iter().any(|s| {
        let word = dict.word(s.word).text;
        related
            .iter()
            .any(|r| search::normalize(r) == search::normalize(word))
    })
}

/// Picks up to `count` new items to teach (spec 2.3).
pub fn suggest(dict: &Dict, progress: &Progress, count: usize) -> Vec<SenseId> {
    if count == 0 {
        return Vec::new();
    }
    // Step 1: unexplored items inside the window above the frontier.
    let last = (progress.frontier + FEED_WINDOW).min(dict.learn_count());
    let mut scored: Vec<(f32, Sense)> = (progress.frontier..=last)
        .filter_map(|rank| dict.at_rank(rank))
        .filter(|s| s.teachable() && progress.state(s) == State::Unexplored)
        .map(|s| (priority(dict, progress, &s), s))
        .collect();
    // Step 2: highest priority first, ties broken by rank so it is stable.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.rank.cmp(&b.1.rank)));

    // Step 3: fill the session, skipping anything that would interfere.
    let mut chosen: Vec<Sense> = Vec::with_capacity(count);
    for (_, sense) in &scored {
        if chosen.len() == count {
            break;
        }
        if !interferes(dict, &chosen, sense) {
            chosen.push(*sense);
        }
    }
    chosen.iter().map(|s| s.id).collect()
}

/// Random items from below the frontier, for Gap Filling (spec 2.2).
///
/// These are the words a textbook learner is most likely to be missing:
/// everyday vocabulary the placement test assumed they had.
pub fn quick_scan(dict: &Dict, progress: &Progress, rng: &mut Rng, count: usize) -> Vec<SenseId> {
    let ceiling = progress.frontier.saturating_sub(1).min(dict.learn_count());
    if ceiling == 0 || count == 0 {
        return Vec::new();
    }
    let mut out: Vec<SenseId> = Vec::with_capacity(count);
    for _ in 0..count * 60 {
        if out.len() == count {
            break;
        }
        let Some(sense) = dict.at_rank(1 + rng.below(ceiling as usize) as u32) else {
            continue;
        };
        // Only things never confirmed either way: assumed known, or inside a
        // band the user chose to skip.
        let state = progress.state(&sense);
        let untested = matches!(state, State::AssumedKnown | State::Unexplored);
        if sense.teachable() && untested && !out.contains(&sense.id) {
            out.push(sense.id);
        }
    }
    out
}

// -------------------------------------------------------------------------
// the session (spec 3.6)
// -------------------------------------------------------------------------

/// A day's queue: due reviews, then new items, then spot-checks.
#[derive(Clone, Debug, Default)]
pub struct Session {
    tasks: Vec<Task>,
    at: usize,
}

impl Session {
    /// Builds the queue in the order spec 3.6 lays down: "thẻ ôn quá hạn (xếp
    /// theo khả năng nhớ thấp nhất trước) → LI mới → kiểm tra xác minh".
    pub fn build(dict: &Dict, progress: &Progress, day: Day) -> Self {
        let mut tasks: Vec<Task> = progress
            .due_cards(day)
            .into_iter()
            .map(Task::Review)
            .collect();
        let allowance = progress.new_allowance(day) as usize;
        tasks.extend(
            suggest(dict, progress, allowance)
                .into_iter()
                .map(Task::New),
        );
        tasks.extend(progress.verification_due(day).into_iter().map(Task::Verify));
        Self { tasks, at: 0 }
    }

    pub fn current(&self) -> Option<Task> {
        self.tasks.get(self.at).copied()
    }

    pub fn advance(&mut self) {
        self.at += 1;
    }

    /// Puts the current task at the back — used when an answer was wrong, so
    /// the item comes round again before the session ends (spec 3.3).
    pub fn requeue(&mut self) {
        if let Some(task) = self.current() {
            self.tasks.push(task);
        }
        self.advance();
    }

    pub fn done(&self) -> usize {
        self.at
    }

    pub fn total(&self) -> usize {
        self.tasks.len()
    }

    pub fn is_finished(&self) -> bool {
        self.at >= self.tasks.len()
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let mut out = (0, 0, 0);
        for task in &self.tasks[self.at.min(self.tasks.len())..] {
            match task {
                Task::Review(_) => out.0 += 1,
                Task::New(_) => out.1 += 1,
                Task::Verify(_) => out.2 += 1,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::Source;
    use crate::srs::Outcome;

    fn dict() -> Dict {
        Dict::load()
    }

    fn placed(frontier: u32) -> Progress {
        let mut p = Progress::default();
        p.apply_placement(frontier, (frontier as f32).ln());
        p
    }

    #[test]
    fn suggestions_come_from_the_window_above_the_frontier() {
        let d = dict();
        let p = placed(3_000);
        let picks = suggest(&d, &p, 10);
        assert_eq!(picks.len(), 10);
        for id in picks {
            let s = d.sense(id);
            assert!(
                s.rank >= p.frontier,
                "rank {} below frontier {}",
                s.rank,
                p.frontier
            );
            assert!(
                s.rank <= p.frontier + FEED_WINDOW,
                "rank {} past the window",
                s.rank
            );
            assert_eq!(p.state(&s), State::Unexplored);
        }
    }

    #[test]
    fn nearer_the_frontier_wins_all_else_equal() {
        let d = dict();
        let p = placed(5_000);
        let picks = suggest(&d, &p, 5);
        let ranks: Vec<u32> = picks.iter().map(|&id| d.sense(id).rank).collect();
        // Not strictly sorted — relevance and the interference rule reorder
        // things — but the picks must stay near the frontier, not at the far
        // edge of the 500-wide window.
        let average = ranks.iter().sum::<u32>() as f32 / ranks.len() as f32;
        assert!(average < (p.frontier + FEED_WINDOW / 2) as f32, "{ranks:?}");
    }

    #[test]
    fn a_looked_up_word_is_promoted() {
        let d = dict();
        let base = placed(2_000);
        let far = (base.frontier + FEED_WINDOW - 1..base.frontier + FEED_WINDOW)
            .filter_map(|r| d.at_rank(r))
            .find(|s| s.teachable() && base.state(s) == State::Unexplored)
            .expect("an item at the far edge of the window");
        assert!(
            !suggest(&d, &base, 5).contains(&far.id),
            "already suggested without the boost"
        );

        let mut boosted = base.clone();
        boosted.lookups.insert(far.word);
        let picks = suggest(&d, &boosted, 5);
        let plain_rank = |id: &SenseId| d.sense(*id).rank;
        assert!(
            picks.contains(&far.id) || picks.iter().map(plain_rank).max() < Some(far.rank),
            "looking it up did not help: {:?}",
            picks.iter().map(plain_rank).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_session_never_teaches_two_senses_of_one_word() {
        // Spec 2.3, step 3.
        let d = dict();
        for frontier in [500u32, 2_000, 9_000] {
            let picks = suggest(&d, &placed(frontier), 20);
            let mut words: Vec<_> = picks.iter().map(|&id| d.sense(id).word).collect();
            let before = words.len();
            words.sort_unstable();
            words.dedup();
            assert_eq!(
                words.len(),
                before,
                "repeated headword at frontier {frontier}"
            );
        }
    }

    #[test]
    fn a_session_never_teaches_a_synonym_pair() {
        let d = dict();
        let picks = suggest(&d, &placed(1_500), 20);
        for (i, &a) in picks.iter().enumerate() {
            let word_a = d.word(d.sense(a).word).text;
            let related: Vec<String> = d
                .relations(d.sense(a).word)
                .into_iter()
                .filter(|(k, _)| matches!(k, Relation::Synonym | Relation::Antonym))
                .map(|(_, t)| search::normalize(t))
                .collect();
            for &b in &picks[i + 1..] {
                let word_b = search::normalize(d.word(d.sense(b).word).text);
                assert!(
                    !related.contains(&word_b),
                    "{word_a} and {word_b} in one session"
                );
            }
        }
    }

    #[test]
    fn suggestions_respect_the_daily_allowance() {
        let d = dict();
        let mut p = placed(1_000);
        p.daily_goal = 3;
        let day = 100;
        let session = Session::build(&d, &p, day);
        assert_eq!(session.counts().1, 3);

        p.new_today = 3;
        assert_eq!(Session::build(&d, &p, day).counts().1, 0);
    }

    #[test]
    fn the_session_runs_reviews_then_new_then_checks() {
        let d = dict();
        let mut p = placed(1_000);
        p.daily_goal = 2;
        let day = 500;
        // One overdue review…
        let due = d.at_rank(700).unwrap();
        p.start_learning(due.id, Source::Manual, day - 10);
        p.answer(
            due.id,
            Outcome {
                correct: true,
                hesitated: false,
                level: 1,
            },
            day - 10,
        );
        // …and one known item overdue a check.
        let known = d.at_rank(200).unwrap();
        p.set_state(known.id, State::Known, Source::Manual, 0);
        p.new_today = 0;

        let session = Session::build(&d, &p, day);
        let kinds: Vec<u8> = (0..session.total())
            .map(|i| match session.tasks[i] {
                Task::Review(_) => 0,
                Task::New(_) => 1,
                Task::Verify(_) => 2,
            })
            .collect();
        assert!(
            kinds.windows(2).all(|w| w[0] <= w[1]),
            "out of order: {kinds:?}"
        );
        assert!(
            kinds.contains(&0) && kinds.contains(&1) && kinds.contains(&2),
            "{kinds:?}"
        );
    }

    #[test]
    fn a_wrong_answer_puts_the_item_back_in_the_queue() {
        let d = dict();
        let p = placed(1_000);
        let mut session = Session::build(&d, &p, 100);
        let total = session.total();
        let first = session.current().unwrap();
        session.requeue();
        assert_eq!(session.total(), total + 1);
        assert_eq!(session.tasks.last().copied(), Some(first));
    }

    #[test]
    fn quick_scan_samples_below_the_frontier() {
        let d = dict();
        let p = placed(4_000);
        let mut rng = Rng::seeded(11);
        let picks = quick_scan(&d, &p, &mut rng, 20);
        assert_eq!(picks.len(), 20);
        for id in &picks {
            let s = d.sense(*id);
            assert!(
                s.rank < p.frontier,
                "rank {} is not below {}",
                s.rank,
                p.frontier
            );
            assert_eq!(p.state(&s), State::AssumedKnown);
        }
        let unique: std::collections::BTreeSet<_> = picks.iter().collect();
        assert_eq!(unique.len(), picks.len(), "duplicates in one scan");
    }

    #[test]
    fn quick_scan_has_nothing_to_offer_a_beginner() {
        let d = dict();
        let mut rng = Rng::seeded(12);
        assert!(quick_scan(&d, &Progress::default(), &mut rng, 20).is_empty());
    }

    #[test]
    fn exercises_match_their_level() {
        let d = dict();
        let mut rng = Rng::seeded(13);
        let shown = Shown::default();
        let sense = d.at_rank(1_000).unwrap();
        assert_eq!(exercise(&d, &mut rng, &sense, 0, &shown).level(), 1);
        assert!(matches!(
            exercise(&d, &mut rng, &sense, 1, &shown),
            Exercise::Meaning(_)
        ));
        assert_eq!(exercise(&d, &mut rng, &sense, 2, &shown).level(), 2);
        assert!(matches!(
            exercise(&d, &mut rng, &sense, 3, &shown),
            Exercise::Spell
        ));
    }

    #[test]
    fn turning_off_typing_stops_the_spelling_exercise() {
        // The setting has to change what a session asks, not just persist.
        let d = dict();
        let mut rng = Rng::seeded(21);
        let shown = Shown::default();
        let sense = d.at_rank(1_000).unwrap();
        let all = crate::progress::Challenges::default();
        assert!(matches!(
            exercise(&d, &mut rng, &sense, all.level_for(3), &shown),
            Exercise::Spell
        ));

        let no_typing = crate::progress::Challenges {
            produce: false,
            ..all
        };
        assert!(!matches!(
            exercise(&d, &mut rng, &sense, no_typing.level_for(3), &shown),
            Exercise::Spell
        ));
    }

    #[test]
    fn a_shown_sentence_is_not_reused_as_a_gap_fill() {
        // Spec 1.4: testing on the sentence just read is not a test.
        let d = dict();
        let mut rng = Rng::seeded(14);
        let sense = (1..6_000)
            .filter_map(|r| d.at_rank(r))
            .find(|s| quiz::cloze(&d, s).is_some())
            .expect("an item with a usable example");
        let fresh = Shown::default();
        assert!(matches!(
            exercise(&d, &mut rng, &sense, 2, &fresh),
            Exercise::Fill(_)
        ));

        let mut shown = Shown::default();
        shown.mark(sense.id);
        assert!(
            !matches!(exercise(&d, &mut rng, &sense, 2, &shown), Exercise::Fill(_)),
            "reused the sentence the user just read"
        );
    }

    #[test]
    fn spelling_is_checked_leniently() {
        let d = dict();
        let sense = d.exact("apple").unwrap().senses().next().unwrap();
        let sense = d.sense(sense);
        assert!(spelling_accepts(&d, &sense, "apple"));
        assert!(spelling_accepts(&d, &sense, "  APPLE "));
        assert!(!spelling_accepts(&d, &sense, "aple"));
        assert!(!spelling_accepts(&d, &sense, ""));
    }
}
