//! What the user knows: one card per Learning Item they have touched, plus the
//! placement result and the day counters.
//!
//! This is spec 0.2's state table and spec 3.1's transitions. The whole thing
//! serialises into eframe's storage (a file on desktop and Android, local
//! storage on the web), so progress survives a restart without a server.
//!
//! Only touched items get a card. Everything below the placement test's
//! frontier is [`State::AssumedKnown`] by rule rather than by row, which is
//! what keeps a 25.000-item list down to a few kilobytes of saved state.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::dict::{Sense, SenseId, WordId};
use crate::srs::{Grade, Memory, Outcome};

/// Whole days since the Unix epoch, UTC. Scheduling is day-granular, so this is
/// all the calendar the app needs.
pub type Day = i64;

/// Today's day number.
pub fn today() -> Day {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as Day)
}

/// Spec 3.4: a card is mastered once it is this stable, among other things.
pub const MASTERED_STABILITY: f32 = 60.0;
/// Spec 3.6: this many failures makes an item a leech.
pub const LEECH_LAPSES: u32 = 8;
/// Spec 3.6: and a leech is then put aside for this long.
pub const LEECH_POSTPONE: Day = 3;
/// Spec 2.2: at most this many Quick Scan cards a day.
pub const QUICK_SCAN_DAILY: u32 = 20;
/// Spec 3.5: at most this many Known checks per session…
pub const VERIFY_PER_SESSION: usize = 2;
/// …and only for items not checked in this many days.
pub const VERIFY_MIN_GAP: Day = 30;
/// Spec 2.3: width of the Smart Feeding candidate window.
pub const FEED_WINDOW: u32 = 500;
/// Spec 3.6: new items pause once the review backlog is this many times the
/// daily goal.
pub const BACKLOG_FACTOR: u32 = 3;
/// Stability (days) at which an item moves up an exercise level (spec 3.3).
const LEVEL2_STABILITY: f32 = 7.0;
const LEVEL3_STABILITY: f32 = 21.0;
/// Successful reviews before a Learning item graduates to Review (spec 3.1).
const GRADUATE_REPS: u32 = 2;

/// Unix seconds. Reminders are the one thing here finer-grained than a day.
pub fn now_secs() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// How often to nudge the user back to studying (spec 3.6: "Nhắc ôn qua thông
/// báo đẩy vào khung giờ người dùng hay học").
///
/// An interval rather than a clock time on purpose: the app has no reliable
/// local timezone — `SystemTime` is UTC everywhere — so "every four hours" is
/// a promise it can keep and "every day at 8pm" is not.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Reminders {
    Off,
    EveryFourHours,
    EveryEightHours,
    #[default]
    Daily,
}

impl Reminders {
    pub const ALL: [Self; 4] = [
        Self::Off,
        Self::EveryFourHours,
        Self::EveryEightHours,
        Self::Daily,
    ];

    /// Hours between nudges, or `None` when switched off.
    pub fn every_hours(self) -> Option<i64> {
        match self {
            Self::Off => None,
            Self::EveryFourHours => Some(4),
            Self::EveryEightHours => Some(8),
            Self::Daily => Some(24),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::EveryFourHours => "Every 4h",
            Self::EveryEightHours => "Every 8h",
            Self::Daily => "Once a day",
        }
    }
}

/// Which colour scheme to draw in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Theme {
    /// The default. Most reading here is Vietnamese prose in a dictionary, and
    /// that is what paper-like contrast suits.
    #[default]
    Light,
    Dark,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }
}

/// Spec 0.2's six states.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum State {
    Unexplored,
    AssumedKnown,
    Known,
    Learning,
    Review,
    Mastered,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unexplored => "New",
            Self::AssumedKnown => "Probably known",
            Self::Known => "Known",
            Self::Learning => "Learning",
            Self::Review => "In review",
            Self::Mastered => "Mastered",
        }
    }

    /// Is this item on the study path (spec 3.6's session queue)?
    pub fn in_study(self) -> bool {
        matches!(self, Self::Learning | Self::Review | Self::Mastered)
    }
}

/// How an item reached its state (spec 1.2's `UserSenseState.source`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Source {
    /// The placement test inferred it.
    Test,
    /// The user said so: "I Know This", or a Quick Scan swipe.
    Manual,
    /// It came out of a study session.
    Study,
}

/// One Learning Item's record. Only items the user has touched have one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Card {
    pub state: State,
    /// FSRS memory; `None` until the first graded answer.
    pub memory: Option<Memory>,
    pub due: Day,
    pub last_review: Day,
    pub reps: u32,
    pub lapses: u32,
    /// Exercise level 1–3 (spec 3.3).
    pub level: u8,
    /// Spec 3.4 requires at least one correct level-3 answer.
    pub passed_level3: bool,
    /// Low three bits: was each of the last three answers an Again? (spec 3.4)
    pub recent_again: u8,
    pub source: Source,
    /// Day of the last Known verification (spec 3.5).
    pub verified: Day,
    /// Days until the next verification; doubles after each pass (spec 3.5).
    pub verify_gap: u16,
}

impl Card {
    fn new(state: State, source: Source, day: Day) -> Self {
        Self {
            state,
            memory: None,
            due: day,
            last_review: day,
            reps: 0,
            lapses: 0,
            level: 1,
            passed_level3: false,
            recent_again: 0,
            source,
            verified: day,
            verify_gap: VERIFY_MIN_GAP as u16,
        }
    }

    /// Spec 3.6: too many failures, so it needs a different approach.
    pub fn is_leech(&self) -> bool {
        self.lapses >= LEECH_LAPSES
    }

    /// Probability of recall right now, for ordering the review queue.
    pub fn retrievability(&self, day: Day) -> f32 {
        match self.memory {
            Some(m) => m.retrievability((day - self.last_review).max(0) as f32),
            None => 0.0,
        }
    }

    /// Spec 3.4: stable for 60 days, one level-3 answer, and no Again in the
    /// last three reviews.
    fn is_mastered(&self) -> bool {
        self.memory
            .is_some_and(|m| m.stability >= MASTERED_STABILITY)
            && self.passed_level3
            && self.recent_again & 0b111 == 0
    }
}

/// Everything the app remembers about one user.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    cards: BTreeMap<SenseId, Card>,
    /// Spec 2.2: every item at or below this rank is assumed known.
    pub assumed_below: u32,
    /// Spec 2.3: where new items are fed from. Raising it above
    /// `assumed_below` is the Skip Band — the gap is not assumed known, it is
    /// merely skipped, and Quick Scan keeps offering it.
    pub frontier: u32,
    /// IRT ability estimate from the placement test (spec 2.2), in log-rank
    /// units, kept up to date by later answers.
    pub theta: f32,
    pub placement_done: bool,
    /// New items per day, chosen at onboarding (spec 3.6).
    pub daily_goal: u32,
    /// FSRS desired retention, 0,8–0,95 (spec 3.2).
    pub retention: f32,
    pub theme: Theme,
    /// How often to nudge about studying (spec 3.6).
    pub reminders: Reminders,
    /// Unix seconds of the last nudge, so one interval means one nudge.
    pub last_reminded: i64,
    pub streak: u32,
    pub best_streak: u32,
    /// Last day the user studied, for the streak.
    pub last_active: Day,
    /// Spec 3.6: the streak freeze, usable once a week.
    pub freeze_used: Day,
    /// The day the counters below belong to.
    pub day: Day,
    pub new_today: u32,
    pub scanned_today: u32,
    pub reviews_today: u32,
    /// Headwords the user has looked up: the `Lookup` term of spec 2.3's
    /// Relevance, and the personal boost in spec 1.1's ranking.
    pub lookups: BTreeSet<WordId>,
    /// Bumped by every change below. Screens that cache an expensive derived
    /// view — the map's block tallies, the session badge in the top bar —
    /// compare this instead of recomputing on every frame. Not persisted: a
    /// fresh session just recomputes once.
    #[serde(skip)]
    rev: u64,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            cards: BTreeMap::new(),
            assumed_below: 0,
            frontier: 1,
            // ln(3000): a mid-list starting guess until the test runs.
            theta: 8.0,
            placement_done: false,
            daily_goal: 10,
            retention: 0.9,
            theme: Theme::Light,
            reminders: Reminders::default(),
            last_reminded: 0,
            streak: 0,
            best_streak: 0,
            last_active: 0,
            freeze_used: 0,
            day: today(),
            new_today: 0,
            scanned_today: 0,
            reviews_today: 0,
            lookups: BTreeSet::new(),
            rev: 0,
        }
    }
}

/// Enough to put one item back the way it was — spec 1.4's five-second Undo.
#[derive(Clone, Debug)]
pub struct Undo {
    pub sense: SenseId,
    pub before: Option<Card>,
    pub message: String,
}

impl Progress {
    // --- reading ---------------------------------------------------------

    /// The state of one item (spec 0.2).
    ///
    /// Untouched items have no card: they are assumed known when the placement
    /// test put them below the frontier, and unexplored otherwise.
    pub fn state(&self, sense: &Sense) -> State {
        match self.cards.get(&sense.id) {
            Some(card) => card.state,
            None if sense.rank > 0 && sense.rank <= self.assumed_below => State::AssumedKnown,
            None => State::Unexplored,
        }
    }

    /// Changes whenever anything in here does. See the field's note.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    pub fn card(&self, sense: SenseId) -> Option<&Card> {
        self.cards.get(&sense)
    }

    pub fn cards(&self) -> impl Iterator<Item = (SenseId, &Card)> {
        self.cards.iter().map(|(&id, card)| (id, card))
    }

    /// Cards due on or before `day`, hardest-to-recall first (spec 3.6).
    pub fn due_cards(&self, day: Day) -> Vec<SenseId> {
        let mut due: Vec<SenseId> = self
            .cards
            .iter()
            .filter(|(_, c)| c.state.in_study() && c.due <= day)
            .map(|(&id, _)| id)
            .collect();
        due.sort_by(|a, b| {
            let (ra, rb) = (
                self.cards[a].retrievability(day),
                self.cards[b].retrievability(day),
            );
            ra.total_cmp(&rb).then(a.cmp(b))
        });
        due
    }

    /// Is a study nudge due? (spec 3.6)
    ///
    /// Two guards beyond the interval: nothing is sent when the queue is empty,
    /// because a notification with nothing behind it is the fastest way to have
    /// notifications turned off; and studying counts as a nudge answered, so
    /// finishing a session buys a full interval of quiet.
    pub fn reminder_due(&self, now: i64, waiting: usize) -> bool {
        let Some(every) = self.reminders.every_hours() else {
            return false;
        };
        waiting > 0 && now.saturating_sub(self.last_reminded) >= every * 3_600
    }

    /// Restarts the reminder interval — after a nudge, or after studying.
    pub fn mark_reminded(&mut self, now: i64) {
        self.last_reminded = now;
        self.rev += 1;
    }

    /// Spec 3.6: once the backlog is this deep, stop feeding new items.
    pub fn backlog_is_heavy(&self, day: Day) -> bool {
        self.due_cards(day).len() as u32 > self.daily_goal * BACKLOG_FACTOR
    }

    /// How many new items are still allowed today (spec 3.6).
    pub fn new_allowance(&self, day: Day) -> u32 {
        if self.backlog_is_heavy(day) {
            return 0;
        }
        self.daily_goal.saturating_sub(self.new_today)
    }

    /// Known items that are due a spot-check (spec 3.5).
    pub fn verification_due(&self, day: Day) -> Vec<SenseId> {
        let mut out: Vec<SenseId> = self
            .cards
            .iter()
            .filter(|(_, c)| {
                matches!(c.state, State::Known | State::AssumedKnown)
                    && day - c.verified >= c.verify_gap as Day
            })
            .map(|(&id, _)| id)
            .collect();
        out.sort_by_key(|id| self.cards[id].verified);
        out.truncate(VERIFY_PER_SESSION);
        out
    }

    /// Counts of each state across the whole learning list, for the map.
    pub fn tally(&self, ranks: impl Iterator<Item = (SenseId, u32)>) -> [u32; 6] {
        let mut out = [0u32; 6];
        for (id, rank) in ranks {
            let state = match self.cards.get(&id) {
                Some(card) => card.state,
                None if rank > 0 && rank <= self.assumed_below => State::AssumedKnown,
                None => State::Unexplored,
            };
            out[state as usize] += 1;
        }
        out
    }

    // --- writing ---------------------------------------------------------

    /// Records a lookup: spec 2.3's `Lookup` signal, and spec 2.2's hint that
    /// an assumed-known item may not be known after all.
    pub fn note_lookup(&mut self, word: WordId, senses: &[Sense]) {
        if self.lookups.insert(word) {
            self.rev += 1;
        }
        // Spec 3.1: looking an assumed-known item up again drops it back to
        // Unexplored, so it can be offered properly.
        for sense in senses {
            if self.state(sense) == State::AssumedKnown && !self.cards.contains_key(&sense.id) {
                self.cards.insert(
                    sense.id,
                    Card::new(State::Unexplored, Source::Test, self.day),
                );
                self.rev += 1;
            }
        }
    }

    /// Moves one item to `state`, returning what is needed to undo it.
    pub fn set_state(&mut self, sense: SenseId, state: State, source: Source, day: Day) -> Undo {
        self.rev += 1;
        let before = self.cards.get(&sense).cloned();
        let card = self
            .cards
            .entry(sense)
            .or_insert_with(|| Card::new(state, source, day));
        card.state = state;
        card.source = source;
        if state == State::Learning && card.memory.is_none() {
            card.due = day;
        }
        if matches!(state, State::Known) {
            card.verified = day;
            card.verify_gap = VERIFY_MIN_GAP as u16;
        }
        Undo {
            sense,
            before,
            message: format!("Moved to “{}”", state.label()),
        }
    }

    /// Puts back what [`Self::set_state`] or [`Self::answer`] changed.
    pub fn undo(&mut self, undo: Undo) {
        self.rev += 1;
        match undo.before {
            Some(card) => {
                self.cards.insert(undo.sense, card);
            }
            None => {
                self.cards.remove(&undo.sense);
            }
        }
    }

    /// Spec 1.4's "Learn This", and the same path Smart Feeding uses.
    pub fn start_learning(&mut self, sense: SenseId, source: Source, day: Day) -> Undo {
        let is_new = !self.cards.contains_key(&sense);
        let undo = self.set_state(sense, State::Learning, source, day);
        if is_new {
            self.new_today += 1;
        }
        undo
    }

    /// Grades one answer and reschedules the card (spec 3.1–3.4).
    pub fn answer(&mut self, sense: SenseId, outcome: Outcome, day: Day) -> (Grade, Undo) {
        self.rev += 1;
        let grade = outcome.grade();
        let before = self.cards.get(&sense).cloned();
        let retention = self.retention;
        let card = self
            .cards
            .entry(sense)
            .or_insert_with(|| Card::new(State::Learning, Source::Study, day));

        let elapsed = (day - card.last_review).max(0) as f32;
        let memory = match card.memory {
            Some(m) => m.review(grade, elapsed),
            None => Memory::first(grade),
        };
        card.memory = Some(memory);
        card.last_review = day;
        card.recent_again = (card.recent_again << 1) | u8::from(grade == Grade::Again);

        if grade == Grade::Again {
            card.lapses += 1;
            // Spec 3.1: a wrong answer sends anything back to Learning, and
            // spec 3.3 restarts it at level 1.
            card.state = State::Learning;
            card.level = 1;
        } else {
            card.reps += 1;
            if outcome.level >= 3 {
                card.passed_level3 = true;
            }
            // Spec 3.3: level up once the card is stable enough.
            card.level = match memory.stability {
                s if s >= LEVEL3_STABILITY => 3,
                s if s >= LEVEL2_STABILITY => 2,
                _ => 1,
            };
            // Spec 3.1: Learning graduates to Review once the initial steps
            // are done; anything already past Learning stays in the cycle.
            if card.state != State::Learning || card.reps >= GRADUATE_REPS {
                card.state = State::Review;
            }
            if card.is_mastered() {
                card.state = State::Mastered;
            }
        }

        // Spec 3.6: a leech is set aside for a few days rather than drilled.
        card.due = if card.is_leech() && grade == Grade::Again {
            day + LEECH_POSTPONE
        } else {
            day + memory.interval(retention).round() as Day
        };
        self.reviews_today += 1;
        (
            grade,
            Undo {
                sense,
                before,
                message: String::new(),
            },
        )
    }

    /// Spec 3.5: the result of a Known spot-check.
    pub fn verify(&mut self, sense: SenseId, correct: bool, day: Day) {
        self.rev += 1;
        let Some(card) = self.cards.get_mut(&sense) else {
            return;
        };
        card.verified = day;
        if correct {
            // Passed, so ask again twice as far out.
            card.verify_gap = card.verify_gap.saturating_mul(2).min(365);
        } else {
            // Spec 3.1: a failed check means it was never really known.
            card.state = State::Learning;
            card.level = 1;
            card.due = day;
            card.verify_gap = VERIFY_MIN_GAP as u16;
        }
    }

    /// Spec 2.2: records the placement result.
    pub fn apply_placement(&mut self, frontier: u32, theta: f32) {
        self.rev += 1;
        self.assumed_below = frontier;
        self.frontier = frontier.saturating_add(1);
        self.theta = theta;
        self.placement_done = true;
    }

    /// Spec 2.3, rule 1: the frontier advances once the next block is mostly
    /// explored.
    pub fn advance_frontier(&mut self, explored_ratio: f32) {
        if explored_ratio >= 0.9 {
            self.frontier = self.frontier.saturating_add(1_000);
            self.rev += 1;
        }
    }

    /// Rolls the day over: resets the daily counters and updates the streak.
    ///
    /// Spec 3.6 gives one streak freeze a week, which covers a single missed
    /// day instead of resetting the streak to zero.
    pub fn roll_to(&mut self, day: Day) {
        if day == self.day {
            return;
        }
        let missed = day - self.last_active;
        if self.last_active != 0 && missed > 1 {
            let can_freeze = missed == 2 && day - self.freeze_used >= 7;
            if can_freeze {
                self.freeze_used = day;
            } else {
                self.streak = 0;
            }
        }
        self.day = day;
        self.new_today = 0;
        self.scanned_today = 0;
        self.reviews_today = 0;
        self.rev += 1;
    }

    /// Marks today as studied, extending the streak at most once a day.
    pub fn mark_active(&mut self, day: Day) {
        if self.last_active == day {
            return;
        }
        self.streak = if self.last_active == day - 1 || self.last_active == 0 {
            self.streak + 1
        } else {
            self.streak.max(1)
        };
        self.best_streak = self.best_streak.max(self.streak);
        self.last_active = day;
        self.rev += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::Dict;

    fn sense(dict: &Dict, rank: u32) -> Sense {
        dict.at_rank(rank).expect("rank in range")
    }

    fn right(level: u8) -> Outcome {
        Outcome {
            correct: true,
            hesitated: false,
            level,
        }
    }

    fn wrong() -> Outcome {
        Outcome {
            correct: false,
            hesitated: false,
            level: 1,
        }
    }

    #[test]
    fn untouched_items_follow_the_frontier() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let (low, high) = (sense(&dict, 100), sense(&dict, 9_000));
        assert_eq!(p.state(&low), State::Unexplored);
        p.apply_placement(3_450, 8.1);
        assert_eq!(p.state(&low), State::AssumedKnown);
        assert_eq!(p.state(&high), State::Unexplored);
        // Spec 2.2's own worked example: learning starts at 3.451, with no gap.
        assert_eq!(p.frontier, 3_451);
        assert_eq!(p.state(&sense(&dict, 3_450)), State::AssumedKnown);
        assert_eq!(p.state(&sense(&dict, 3_451)), State::Unexplored);
    }

    #[test]
    fn assumed_known_is_soft() {
        // Spec 3.1: looking one up again drops it back to Unexplored.
        let dict = Dict::load();
        let mut p = Progress::default();
        p.apply_placement(3_000, 8.0);
        let s = sense(&dict, 500);
        assert_eq!(p.state(&s), State::AssumedKnown);
        p.note_lookup(s.word, &[s]);
        assert_eq!(p.state(&s), State::Unexplored);
    }

    #[test]
    fn undo_restores_the_previous_state() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 1_200);
        let undo = p.set_state(s.id, State::Known, Source::Manual, 100);
        assert_eq!(p.state(&s), State::Known);
        p.undo(undo);
        assert_eq!(p.state(&s), State::Unexplored);

        let first = p.set_state(s.id, State::Known, Source::Manual, 100);
        let second = p.set_state(s.id, State::Learning, Source::Study, 100);
        p.undo(second);
        assert_eq!(p.state(&s), State::Known);
        drop(first);
    }

    #[test]
    fn learning_graduates_to_review_then_mastered() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 2_000);
        let mut day = 100;
        p.start_learning(s.id, Source::Manual, day);
        assert_eq!(p.state(&s), State::Learning);

        p.answer(s.id, right(1), day);
        assert_eq!(
            p.state(&s),
            State::Learning,
            "one correct answer is not enough"
        );
        p.answer(s.id, right(1), day);
        assert_eq!(p.state(&s), State::Review);

        for _ in 0..25 {
            day = p.card(s.id).unwrap().due;
            p.answer(s.id, right(3), day);
            if p.state(&s) == State::Mastered {
                break;
            }
        }
        assert_eq!(p.state(&s), State::Mastered);
        let card = p.card(s.id).unwrap();
        assert!(card.memory.unwrap().stability >= MASTERED_STABILITY);
        assert!(card.passed_level3);

        // Spec 3.1: a wrong answer drops Mastered back down.
        p.answer(s.id, wrong(), day + 1);
        assert_eq!(p.state(&s), State::Learning);
    }

    #[test]
    fn a_wrong_answer_resets_the_exercise_level() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 4_000);
        let mut day = 10;
        p.start_learning(s.id, Source::Manual, day);
        for _ in 0..6 {
            p.answer(s.id, right(2), day);
            day = p.card(s.id).unwrap().due;
        }
        assert!(p.card(s.id).unwrap().level > 1);
        p.answer(s.id, wrong(), day);
        assert_eq!(p.card(s.id).unwrap().level, 1);
    }

    #[test]
    fn leeches_are_postponed_not_drilled() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 5_000);
        let day = 50;
        p.start_learning(s.id, Source::Manual, day);
        for _ in 0..LEECH_LAPSES {
            p.answer(s.id, wrong(), day);
        }
        let card = p.card(s.id).unwrap();
        assert!(card.is_leech());
        assert_eq!(card.due, day + LEECH_POSTPONE);
    }

    #[test]
    fn the_review_queue_is_hardest_first() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let day = 200;
        // Three cards, all overdue, with different stabilities.
        for (rank, stability) in [(1_000, 60.0), (1_100, 2.0), (1_200, 20.0)] {
            let s = sense(&dict, rank);
            p.start_learning(s.id, Source::Manual, day - 30);
            let card = p.cards.get_mut(&s.id).unwrap();
            card.state = State::Review;
            card.memory = Some(Memory {
                stability,
                difficulty: 5.0,
            });
            card.last_review = day - 30;
            card.due = day - 1;
        }
        let queue = p.due_cards(day);
        assert_eq!(queue.len(), 3);
        let r: Vec<f32> = queue
            .iter()
            .map(|id| p.card(*id).unwrap().retrievability(day))
            .collect();
        assert!(r[0] <= r[1] && r[1] <= r[2], "{r:?}");
    }

    #[test]
    fn new_items_pause_when_reviews_pile_up() {
        let dict = Dict::load();
        let mut p = Progress {
            daily_goal: 5,
            ..Progress::default()
        };
        let day = 300;
        assert_eq!(p.new_allowance(day), 5);
        for rank in 1_000..1_000 + p.daily_goal * BACKLOG_FACTOR + 1 {
            let s = sense(&dict, rank);
            p.start_learning(s.id, Source::Manual, day);
            p.cards.get_mut(&s.id).unwrap().due = day - 1;
        }
        assert!(p.backlog_is_heavy(day));
        assert_eq!(p.new_allowance(day), 0);
    }

    #[test]
    fn known_checks_come_due_and_back_off() {
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 800);
        p.set_state(s.id, State::Known, Source::Manual, 0);
        assert!(p.verification_due(VERIFY_MIN_GAP - 1).is_empty());
        assert_eq!(p.verification_due(VERIFY_MIN_GAP), vec![s.id]);

        p.verify(s.id, true, VERIFY_MIN_GAP);
        assert_eq!(p.card(s.id).unwrap().verify_gap, VERIFY_MIN_GAP as u16 * 2);
        assert!(p.verification_due(VERIFY_MIN_GAP + 1).is_empty());

        // Spec 3.5: failing a check means it was not known.
        p.verify(s.id, false, 500);
        assert_eq!(p.state(&s), State::Learning);
    }

    #[test]
    fn at_most_two_checks_per_session() {
        let dict = Dict::load();
        let mut p = Progress::default();
        for rank in 1..10 {
            p.set_state(sense(&dict, rank).id, State::Known, Source::Manual, 0);
        }
        assert_eq!(p.verification_due(100).len(), VERIFY_PER_SESSION);
    }

    #[test]
    fn streaks_extend_break_and_freeze() {
        let mut p = Progress::default();
        p.roll_to(10);
        p.mark_active(10);
        assert_eq!(p.streak, 1);
        // Same day twice does not double-count.
        p.mark_active(10);
        assert_eq!(p.streak, 1);

        p.roll_to(11);
        p.mark_active(11);
        assert_eq!(p.streak, 2);

        // One missed day is covered by the weekly freeze.
        p.roll_to(13);
        assert_eq!(p.streak, 2);
        p.mark_active(13);

        // A second gap the same week is not.
        p.roll_to(15);
        assert_eq!(p.streak, 0);
    }

    #[test]
    fn day_rollover_clears_the_counters() {
        let mut p = Progress::default();
        p.roll_to(400);
        p.new_today = 7;
        p.scanned_today = 3;
        p.roll_to(401);
        assert_eq!((p.new_today, p.scanned_today), (0, 0));
    }

    #[test]
    fn the_frontier_advances_once_a_block_is_explored() {
        // Spec 2.3, rule 1.
        let dict = Dict::load();
        let mut p = Progress::default();
        p.apply_placement(2_000, 7.6);
        let start = p.frontier;

        // 80% explored is not enough.
        for rank in start..start + 800 {
            p.set_state(sense(&dict, rank).id, State::Known, Source::Manual, 5);
        }
        let block = |p: &Progress| p.tally(dict.learn_span(start..start + 1_000));
        let counts = block(&p);
        let total: u32 = counts.iter().sum();
        let explored = |c: [u32; 6]| (total - c[State::Unexplored as usize]) as f32 / total as f32;
        p.advance_frontier(explored(counts));
        assert_eq!(p.frontier, start, "advanced at 80%");

        // 90% is.
        for rank in start + 800..start + 900 {
            p.set_state(sense(&dict, rank).id, State::Known, Source::Manual, 5);
        }
        p.advance_frontier(explored(block(&p)));
        assert_eq!(p.frontier, start + 1_000);
    }

    #[test]
    fn the_revision_moves_on_every_change() {
        // Screens cache derived views against this. A state change that leaves
        // the card count alone still has to invalidate them, which is what an
        // earlier count-based stamp got wrong.
        let dict = Dict::load();
        let mut p = Progress::default();
        let s = sense(&dict, 700);
        let mut seen = vec![p.revision()];
        let note = |p: &Progress, seen: &mut Vec<u64>| {
            assert!(
                !seen.contains(&p.revision()),
                "revision {} repeated",
                p.revision()
            );
            seen.push(p.revision());
        };

        p.apply_placement(1_000, 6.9);
        note(&p, &mut seen);
        let undo = p.set_state(s.id, State::Known, Source::Manual, 10);
        note(&p, &mut seen);
        // Known -> Learning: same card, same count, different picture.
        p.set_state(s.id, State::Learning, Source::Study, 10);
        note(&p, &mut seen);
        p.answer(s.id, right(1), 10);
        note(&p, &mut seen);
        p.verify(s.id, true, 40);
        note(&p, &mut seen);
        p.undo(undo);
        note(&p, &mut seen);
        p.note_lookup(s.word, &[]);
        note(&p, &mut seen);
        p.roll_to(11);
        note(&p, &mut seen);
        p.mark_active(11);
        note(&p, &mut seen);
    }

    #[test]
    fn reminders_respect_the_chosen_interval() {
        let mut p = Progress {
            reminders: Reminders::EveryFourHours,
            ..Progress::default()
        };
        p.mark_reminded(0);
        let hour = 3_600;

        assert!(!p.reminder_due(3 * hour, 5), "fired early");
        assert!(p.reminder_due(4 * hour, 5), "did not fire on time");
        assert!(p.reminder_due(40 * hour, 5), "did not fire late");

        // A nudge restarts the interval, so one interval means one nudge.
        p.mark_reminded(4 * hour);
        assert!(!p.reminder_due(5 * hour, 5));
        assert!(p.reminder_due(8 * hour, 5));
    }

    #[test]
    fn reminders_stay_quiet_with_nothing_to_study() {
        // A notification with nothing behind it is how notifications get
        // switched off for good.
        let mut p = Progress {
            reminders: Reminders::Daily,
            ..Progress::default()
        };
        p.mark_reminded(0);
        assert!(
            !p.reminder_due(100 * 3_600, 0),
            "nagged about an empty queue"
        );
        assert!(p.reminder_due(100 * 3_600, 1));
    }

    #[test]
    fn reminders_off_means_off() {
        let mut p = Progress {
            reminders: Reminders::Off,
            ..Progress::default()
        };
        p.mark_reminded(0);
        assert_eq!(Reminders::Off.every_hours(), None);
        assert!(!p.reminder_due(10_000 * 3_600, 99));
    }

    #[test]
    fn every_reminder_rate_is_distinct_and_named() {
        let mut seen = std::collections::BTreeSet::new();
        for rate in Reminders::ALL {
            assert!(!rate.label().is_empty());
            assert!(
                seen.insert(rate.every_hours()),
                "{rate:?} duplicates another"
            );
        }
        // Longer settings really are longer.
        let hours: Vec<_> = Reminders::ALL
            .iter()
            .filter_map(|r| r.every_hours())
            .collect();
        assert!(
            hours.windows(2).all(|w| w[0] < w[1]),
            "{hours:?} not ascending"
        );
    }

    #[test]
    fn studying_buys_a_full_interval_of_quiet() {
        let dict = Dict::load();
        let mut p = Progress {
            reminders: Reminders::EveryFourHours,
            ..Progress::default()
        };
        let hour = 3_600;
        p.mark_reminded(0);
        assert!(p.reminder_due(5 * hour, 3));

        // Finishing a session marks the nudge answered.
        let s = sense(&dict, 900);
        p.start_learning(s.id, Source::Manual, 5);
        p.mark_reminded(5 * hour);
        assert!(!p.reminder_due(6 * hour, 3));
    }

    #[test]
    fn progress_survives_a_round_trip() {
        let dict = Dict::load();
        let mut p = Progress::default();
        p.apply_placement(2_500, 7.9);
        let s = sense(&dict, 3_000);
        p.start_learning(s.id, Source::Manual, 42);
        p.answer(s.id, right(2), 42);
        p.note_lookup(s.word, &[]);

        p.reminders = Reminders::EveryEightHours;
        p.mark_reminded(1_700_000_000);

        let text = ron::to_string(&p).expect("serialises");
        let back: Progress = ron::from_str(&text).expect("deserialises");
        assert_eq!(back.assumed_below, 2_500);
        assert_eq!(back.reminders, Reminders::EveryEightHours);
        assert_eq!(back.last_reminded, 1_700_000_000);
        assert_eq!(back.state(&s), p.state(&s));
        assert_eq!(back.card(s.id).unwrap().reps, 1);
        assert!(back.lookups.contains(&s.word));
    }
}
