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
//!
//! With Google sign-in, a copy also lives in the user's Google Drive and other
//! devices fold it into theirs with [`Progress::merge`]. Every change carries a
//! [`Stamp`] for that, so two devices that studied apart lose nothing.

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

/// When something changed, in milliseconds since the Unix epoch. Sync keeps
/// whichever side changed a thing last; 0 means before stamps existed.
pub type Stamp = u64;

/// The [`Stamp`] for now.
pub fn now() -> Stamp {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as Stamp)
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
    /// When this card last changed.
    #[serde(default)]
    pub changed: Stamp,
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
            changed: now(),
        }
    }

    /// Of two copies of one card, is this the one to keep? The later change,
    /// and for saves from before stamps, the later review.
    fn newer_than(&self, other: &Card) -> bool {
        (self.changed, self.last_review, self.reps + self.lapses)
            > (other.changed, other.last_review, other.reps + other.lapses)
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
    /// Cards an Undo took away, and when. Without these, sync would bring a
    /// card straight back from a copy made before the Undo.
    removed: BTreeMap<SenseId, Stamp>,
    /// When "Erase progress" last ran. Sync drops whatever either side
    /// changed before it.
    reset: Stamp,

    // The fields below are public to read. Change them through the `set_*`
    // methods, which stamp the change for sync.
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
    /// When any of the four placement fields above last changed.
    placement_changed: Stamp,
    /// New items per day, chosen at onboarding (spec 3.6).
    pub daily_goal: u32,
    /// FSRS desired retention, 0,8–0,95 (spec 3.2).
    pub retention: f32,
    pub theme: Theme,
    /// When any of the three settings above last changed.
    settings_changed: Stamp,
    pub streak: u32,
    pub best_streak: u32,
    /// Last day the user studied, for the streak.
    pub last_active: Day,
    /// Spec 3.6: the streak freeze, usable once a week.
    pub freeze_used: Day,
    /// When any of the four streak fields above last changed.
    streak_changed: Stamp,
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
            removed: BTreeMap::new(),
            reset: 0,
            assumed_below: 0,
            frontier: 1,
            // ln(3000): a mid-list starting guess until the test runs.
            theta: 8.0,
            placement_done: false,
            placement_changed: 0,
            daily_goal: 10,
            retention: 0.9,
            theme: Theme::Light,
            settings_changed: 0,
            streak: 0,
            best_streak: 0,
            last_active: 0,
            freeze_used: 0,
            streak_changed: 0,
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
                self.removed.remove(&sense.id);
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
        card.changed = now();
        self.removed.remove(&sense);
        Undo {
            sense,
            before,
            message: format!("Moved to “{}”", state.label()),
        }
    }

    /// Puts back what [`Self::set_state`] or [`Self::answer`] changed.
    pub fn undo(&mut self, undo: Undo) {
        self.rev += 1;
        // Putting a card back is itself a change: stamped now, it wins over a
        // copy of the undone version that sync may already have sent.
        match undo.before {
            Some(card) => {
                self.cards.insert(
                    undo.sense,
                    Card {
                        changed: now(),
                        ..card
                    },
                );
            }
            None => {
                self.cards.remove(&undo.sense);
                self.removed.insert(undo.sense, now());
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
        card.changed = now();
        self.removed.remove(&sense);
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
        card.changed = now();
    }

    /// Spec 2.2: records the placement result.
    pub fn apply_placement(&mut self, frontier: u32, theta: f32) {
        self.rev += 1;
        self.assumed_below = frontier;
        self.frontier = frontier.saturating_add(1);
        self.theta = theta;
        self.placement_done = true;
        self.placement_changed = now();
    }

    /// Spec 2.3, rule 1: the frontier advances once the next block is mostly
    /// explored.
    pub fn advance_frontier(&mut self, explored_ratio: f32) {
        if explored_ratio >= 0.9 {
            self.frontier = self.frontier.saturating_add(1_000);
            self.placement_changed = now();
            self.rev += 1;
        }
    }

    /// Spec 2.3, rule 3: moves where new items come from — the Skip Band, or
    /// back to just above the assumed-known range.
    pub fn set_frontier(&mut self, frontier: u32) {
        self.frontier = frontier;
        self.placement_changed = now();
        self.rev += 1;
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.settings_changed();
    }

    pub fn set_daily_goal(&mut self, goal: u32) {
        self.daily_goal = goal;
        self.settings_changed();
    }

    pub fn set_retention(&mut self, retention: f32) {
        self.retention = retention;
        self.settings_changed();
    }

    fn settings_changed(&mut self) {
        self.settings_changed = now();
        self.rev += 1;
    }

    /// "Erase progress": back to a fresh start, and a mark that tells sync to
    /// erase every other copy too rather than restore from it.
    pub fn erase(&mut self, day: Day) {
        *self = Self {
            reset: now(),
            rev: self.rev + 1,
            ..Self::default()
        };
        self.roll_to(day);
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
            self.streak_changed = now();
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
        self.streak_changed = now();
        self.rev += 1;
    }

    // --- sync ------------------------------------------------------------

    /// Folds another copy of this progress into this one, as sync does with
    /// the copy kept in Google Drive. Returns whether anything here changed.
    ///
    /// Each part keeps whichever side changed it last, so merging in either
    /// order, or the same copy twice, comes out the same:
    ///
    /// - a card: the later-changed copy; an Undo that took one away wins over
    ///   any copy older than the Undo
    /// - placement, and the settings: whichever side set them last
    /// - the streak: whichever side studied last
    /// - today's counters: the later day, or the higher count on the same day,
    ///   so a daily limit used up on one device is used up on all
    /// - lookups: everything either side looked up
    ///
    /// "Erase progress" on either side first drops everything the other side
    /// changed before it.
    pub fn merge(&mut self, other: &Progress) -> bool {
        let reset = self.reset.max(other.reset);
        let mut changed = self.forget_before(reset);
        let mut theirs = other.clone();
        theirs.forget_before(reset);

        for (&id, card) in &theirs.cards {
            let newer = match (self.cards.get(&id), self.removed.get(&id)) {
                (Some(mine), _) => card.newer_than(mine),
                (None, Some(&gone)) => card.changed > gone,
                (None, None) => true,
            };
            if newer {
                self.cards.insert(id, card.clone());
                self.removed.remove(&id);
                changed = true;
            }
        }
        // A removal stamped the same millisecond as a card wins, whichever
        // side each is on, so the two loops agree.
        for (&id, &gone) in &theirs.removed {
            let newer = match (self.cards.get(&id), self.removed.get(&id)) {
                (Some(mine), _) => gone >= mine.changed,
                (None, Some(&mine)) => gone > mine,
                (None, None) => true,
            };
            if newer {
                self.cards.remove(&id);
                self.removed.insert(id, gone);
                changed = true;
            }
        }

        // Saves from before stamps have 0 on both sides; a placement that was
        // actually taken still beats none.
        let placement = |p: &Progress| (p.placement_changed, p.placement_done, p.assumed_below);
        if placement(&theirs) > placement(self) {
            self.assumed_below = theirs.assumed_below;
            self.frontier = theirs.frontier;
            self.theta = theirs.theta;
            self.placement_done = theirs.placement_done;
            self.placement_changed = theirs.placement_changed;
            changed = true;
        }
        if theirs.settings_changed > self.settings_changed {
            self.daily_goal = theirs.daily_goal;
            self.retention = theirs.retention;
            self.theme = theirs.theme;
            self.settings_changed = theirs.settings_changed;
            changed = true;
        }
        // By who studied last rather than by stamp: opening a long-idle device
        // breaks its own streak (`roll_to`), which must not break the one kept
        // up on another device.
        if (theirs.last_active, theirs.streak_changed) > (self.last_active, self.streak_changed) {
            self.streak = theirs.streak;
            self.best_streak = theirs.best_streak;
            self.last_active = theirs.last_active;
            self.freeze_used = theirs.freeze_used;
            self.streak_changed = theirs.streak_changed;
            changed = true;
        }
        if theirs.day > self.day {
            self.day = theirs.day;
            self.new_today = theirs.new_today;
            self.scanned_today = theirs.scanned_today;
            self.reviews_today = theirs.reviews_today;
            changed = true;
        } else if theirs.day == self.day {
            let counters = |p: &Progress| [p.new_today, p.scanned_today, p.reviews_today];
            let max = std::array::from_fn(|i| counters(self)[i].max(counters(&theirs)[i]));
            if max != counters(self) {
                [self.new_today, self.scanned_today, self.reviews_today] = max;
                changed = true;
            }
        }
        for &word in &theirs.lookups {
            changed |= self.lookups.insert(word);
        }

        if changed {
            self.rev += 1;
        }
        changed
    }

    /// Applies an "Erase progress" made at `reset`, if this copy has not seen
    /// it yet: drops everything changed before then. Lookups carry no stamp, so
    /// they all go.
    fn forget_before(&mut self, reset: Stamp) -> bool {
        if reset <= self.reset {
            return false;
        }
        let fresh = Self::default();
        self.cards.retain(|_, card| card.changed >= reset);
        self.removed.retain(|_, &mut gone| gone >= reset);
        if self.placement_changed < reset {
            self.assumed_below = fresh.assumed_below;
            self.frontier = fresh.frontier;
            self.theta = fresh.theta;
            self.placement_done = fresh.placement_done;
            self.placement_changed = 0;
        }
        if self.settings_changed < reset {
            self.daily_goal = fresh.daily_goal;
            self.retention = fresh.retention;
            self.theme = fresh.theme;
            self.settings_changed = 0;
        }
        if self.streak_changed < reset {
            self.streak = 0;
            self.best_streak = 0;
            self.last_active = 0;
            self.freeze_used = 0;
            self.streak_changed = 0;
        }
        self.lookups.clear();
        self.reset = reset;
        self.rev += 1;
        true
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
    fn progress_survives_a_round_trip() {
        let dict = Dict::load();
        let mut p = Progress::default();
        p.apply_placement(2_500, 7.9);
        let s = sense(&dict, 3_000);
        p.start_learning(s.id, Source::Manual, 42);
        p.answer(s.id, right(2), 42);
        p.note_lookup(s.word, &[]);

        let text = ron::to_string(&p).expect("serialises");
        let back: Progress = ron::from_str(&text).expect("deserialises");
        assert_eq!(back.assumed_below, 2_500);
        assert_eq!(back.state(&s), p.state(&s));
        assert_eq!(back.card(s.id).unwrap().reps, 1);
        assert!(back.lookups.contains(&s.word));
    }

    // --- sync ------------------------------------------------------------

    /// What sync compares: everything that is saved.
    fn saved(p: &Progress) -> String {
        serde_json::to_string(p).expect("serialises")
    }

    /// Two copies of one starting point, as two devices hold after a sync.
    fn two_devices() -> (Progress, Progress) {
        let mut p = Progress::default();
        p.apply_placement(2_000, 7.6);
        p.start_learning(10, Source::Manual, 100);
        p.set_state(11, State::Known, Source::Manual, 100);
        (p.clone(), p)
    }

    /// Back-dates a card, so tests do not depend on the clock moving between
    /// two calls.
    fn stamp(p: &mut Progress, id: SenseId, at: Stamp) {
        p.cards.get_mut(&id).expect("card exists").changed = at;
    }

    /// Lets the millisecond clock move on, for "later" in tests that need it.
    fn later() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    #[test]
    fn merging_keeps_what_both_devices_did() {
        let (mut phone, mut laptop) = two_devices();
        phone.answer(10, right(1), 101);
        phone.start_learning(20, Source::Study, 101);
        laptop.set_state(30, State::Known, Source::Manual, 101);
        laptop.note_lookup(7, &[]);

        assert!(phone.merge(&laptop));
        assert_eq!(phone.card(10).unwrap().reps, 1, "the phone's answer stays");
        assert!(phone.card(20).is_some());
        assert_eq!(phone.card(30).unwrap().state, State::Known);
        assert!(phone.lookups.contains(&7));
        assert!(phone.revision() > 0);
    }

    #[test]
    fn the_later_change_to_a_card_wins() {
        let (mut phone, mut laptop) = two_devices();
        phone.set_state(10, State::Known, Source::Manual, 101);
        stamp(&mut phone, 10, 2_000);
        laptop.answer(10, wrong(), 101);
        stamp(&mut laptop, 10, 3_000);

        phone.merge(&laptop);
        assert_eq!(phone.card(10).unwrap().state, State::Learning);
        assert_eq!(phone.card(10).unwrap().lapses, 1);
    }

    #[test]
    fn merging_is_order_independent_and_settles() {
        let (mut a, mut b) = two_devices();
        a.answer(10, right(2), 101);
        stamp(&mut a, 10, 5_000);
        b.answer(10, wrong(), 101);
        stamp(&mut b, 10, 4_000);
        a.start_learning(40, Source::Study, 101);
        b.set_state(41, State::Known, Source::Manual, 101);
        b.set_theme(Theme::Dark);
        a.mark_active(101);
        b.scanned_today = 6;
        a.note_lookup(3, &[]);
        b.note_lookup(4, &[]);

        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(saved(&ab), saved(&ba));

        // Nothing left to learn from either side.
        assert!(!ab.merge(&a));
        assert!(!ab.merge(&b));
        assert!(!ab.merge(&ba));
    }

    #[test]
    fn an_undo_is_not_undone_by_sync() {
        let (mut phone, mut laptop) = two_devices();
        let undo = phone.start_learning(50, Source::Manual, 101);
        laptop.merge(&phone);
        assert!(laptop.card(50).is_some(), "synced before the Undo");

        phone.undo(undo);
        assert!(!phone.merge(&laptop), "the old copy must not bring it back");
        assert!(phone.card(50).is_none());
        laptop.merge(&phone);
        assert!(laptop.card(50).is_none(), "and the Undo reaches the laptop");

        // Learning it again later brings it back everywhere.
        later();
        phone.start_learning(50, Source::Manual, 102);
        laptop.merge(&phone);
        assert!(laptop.card(50).is_some());
    }

    #[test]
    fn erasing_progress_reaches_other_devices() {
        let (mut phone, mut laptop) = two_devices();
        laptop.set_theme(Theme::Dark);
        later();
        phone.erase(101);

        laptop.merge(&phone);
        assert!(laptop.card(10).is_none() && laptop.card(11).is_none());
        assert!(!laptop.placement_done);
        assert_eq!(laptop.theme, Theme::Light);

        // What the laptop does after the erase is kept.
        laptop.start_learning(60, Source::Manual, 101);
        phone.merge(&laptop);
        assert!(phone.card(60).is_some());
        assert!(phone.card(10).is_none());
    }

    #[test]
    fn saves_from_before_sync_merge_by_what_they_hold() {
        let mut old = Progress::default();
        old.apply_placement(3_000, 8.0);
        old.answer(10, right(1), 90);
        old.placement_changed = 0;
        stamp(&mut old, 10, 0);
        // The fields sync added are missing from such a save.
        let mut value = serde_json::to_value(&old).expect("serialises");
        let fields = value.as_object_mut().expect("a struct");
        for key in [
            "removed",
            "reset",
            "placement_changed",
            "settings_changed",
            "streak_changed",
        ] {
            assert!(fields.remove(key).is_some(), "{key} is saved");
        }
        for card in fields["cards"].as_object_mut().expect("a map").values_mut() {
            card.as_object_mut().expect("a struct").remove("changed");
        }
        let old: Progress = serde_json::from_value(value).expect("an old save still loads");

        // A fresh install signing in takes the placement that was taken…
        let mut fresh = Progress::default();
        fresh.merge(&old);
        assert!(fresh.placement_done);
        assert_eq!(fresh.assumed_below, 3_000);

        // …and of two unstamped copies of a card, the later review.
        let mut other = old.clone();
        other.answer(10, wrong(), 95);
        stamp(&mut other, 10, 0);
        let mut merged = old.clone();
        merged.merge(&other);
        assert_eq!(merged.card(10).unwrap().last_review, 95);
    }

    #[test]
    fn the_streak_follows_whoever_studied_last() {
        let (mut phone, mut laptop) = two_devices();
        for day in 90..=100 {
            phone.roll_to(day);
            phone.mark_active(day);
        }
        // The laptop was last used long ago; opening it breaks its streak,
        // and that is the newer change.
        laptop.mark_active(50);
        laptop.roll_to(100);
        assert_eq!(laptop.streak, 0);

        laptop.merge(&phone);
        assert_eq!(laptop.streak, 11);
        assert_eq!(laptop.last_active, 100);
    }

    #[test]
    fn todays_counters_take_the_higher_count() {
        let (mut phone, mut laptop) = two_devices();
        phone.roll_to(200);
        laptop.roll_to(200);
        phone.new_today = 7;
        laptop.new_today = 3;
        laptop.reviews_today = 12;
        laptop.merge(&phone);
        assert_eq!((laptop.new_today, laptop.reviews_today), (7, 12));

        // Yesterday's counts do not override today's.
        let mut stale = phone.clone();
        stale.day = 199;
        stale.new_today = 50;
        assert!(!laptop.merge(&stale));
        assert_eq!(laptop.new_today, 7);
    }

    #[test]
    fn progress_survives_json() {
        let (mut p, _) = two_devices();
        p.note_lookup(9, &[]);
        let undo = p.start_learning(70, Source::Manual, 100);
        p.undo(undo);
        let back: Progress = serde_json::from_str(&saved(&p)).expect("deserialises");
        assert_eq!(saved(&back), saved(&p));
    }
}
