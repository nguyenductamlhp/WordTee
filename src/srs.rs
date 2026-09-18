//! FSRS, the scheduler spec 3.2 asks for ("dùng FSRS … thay cho SM-2").
//!
//! A card's memory is two numbers: *stability* (S), the number of days until
//! recall probability falls to 90%, and *difficulty* (D) on a 1–10 scale. After
//! every review both are updated from the grade and from how retrievable the
//! card was at that moment, and the next interval is whatever keeps recall at
//! the user's desired retention.
//!
//! Spec 3.2 also says the user never presses Again/Hard/Good/Easy themselves —
//! [`Outcome::grade`] derives the grade from how the exercise went.

use serde::{Deserialize, Serialize};

/// FSRS-5 default weights, from the open-source FSRS project the spec points
/// at. Spec 3.2 re-fits these per user after ~1.000 reviews; this build ships
/// the defaults and does not re-fit (see the README).
const W: [f32; 19] = [
    0.40255, 1.18385, 3.173, 15.69105, 7.1949, 0.5345, 1.4604, 0.0046, 1.54575, 0.1192, 1.01925,
    1.9395, 0.11, 0.29605, 2.2698, 0.2315, 2.9898, 0.51655, 0.6621,
];

/// Forgetting-curve exponent, fixed by the FSRS-5 model.
const DECAY: f32 = -0.5;
/// Chosen so that `retrievability(S, S) == 0.9`.
const FACTOR: f32 = 19.0 / 81.0;

/// Intervals never drop below a day or run past a century.
const MIN_INTERVAL: f32 = 1.0;
const MAX_INTERVAL: f32 = 36_500.0;

/// What the user pressed — or rather, what [`Outcome::grade`] worked out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    fn n(self) -> f32 {
        self as u8 as f32
    }
}

/// How one exercise went, which spec 3.2 maps onto a grade.
#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub correct: bool,
    /// The user revealed the hint, or took long enough to count as hesitant.
    pub hesitated: bool,
    /// Exercise level, 1–3 (spec 3.3).
    pub level: u8,
}

impl Outcome {
    /// Spec 3.2's table: wrong → Again, right but slow or hinted → Hard,
    /// right → Good, right and quick at level 3 → Easy.
    pub fn grade(self) -> Grade {
        match self {
            Self { correct: false, .. } => Grade::Again,
            Self {
                hesitated: true, ..
            } => Grade::Hard,
            Self { level, .. } if level >= 3 => Grade::Easy,
            _ => Grade::Good,
        }
    }
}

/// The two numbers FSRS keeps per card.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Memory {
    /// Days until recall probability reaches 90%.
    pub stability: f32,
    /// 1 (easy) to 10 (hard).
    pub difficulty: f32,
}

impl Memory {
    /// State after the very first review of a card.
    pub fn first(grade: Grade) -> Self {
        Self {
            stability: W[grade as usize - 1].max(0.1),
            difficulty: initial_difficulty(grade),
        }
    }

    /// Probability of recall `elapsed` days after the last review.
    pub fn retrievability(self, elapsed: f32) -> f32 {
        (1.0 + FACTOR * elapsed.max(0.0) / self.stability.max(0.1)).powf(DECAY)
    }

    /// Days to wait so that recall probability lands on `retention`.
    pub fn interval(self, retention: f32) -> f32 {
        let retention = retention.clamp(0.5, 0.99);
        let days = self.stability / FACTOR * (retention.powf(1.0 / DECAY) - 1.0);
        days.clamp(MIN_INTERVAL, MAX_INTERVAL)
    }

    /// Applies a review that happened `elapsed` days after the previous one.
    pub fn review(self, grade: Grade, elapsed: f32) -> Self {
        let r = self.retrievability(elapsed);
        let stability = if grade == Grade::Again {
            // A lapse cannot make a card more stable than it already was.
            let forget = W[11]
                * self.difficulty.powf(-W[12])
                * ((self.stability + 1.0).powf(W[13]) - 1.0)
                * (W[14] * (1.0 - r)).exp();
            forget.min(self.stability)
        } else {
            let hard = if grade == Grade::Hard { W[15] } else { 1.0 };
            let easy = if grade == Grade::Easy { W[16] } else { 1.0 };
            let gain = (W[8]).exp()
                * (11.0 - self.difficulty)
                * self.stability.powf(-W[9])
                * ((W[10] * (1.0 - r)).exp() - 1.0)
                * hard
                * easy;
            self.stability * (1.0 + gain)
        };
        // Difficulty drifts with the grade, then reverts towards the "easy"
        // baseline so one bad day does not brand a card hard forever.
        let drifted = self.difficulty - W[6] * (grade.n() - 3.0);
        let difficulty = W[7] * initial_difficulty(Grade::Easy) + (1.0 - W[7]) * drifted;
        Self {
            stability: stability.clamp(0.1, MAX_INTERVAL),
            difficulty: difficulty.clamp(1.0, 10.0),
        }
    }
}

fn initial_difficulty(grade: Grade) -> f32 {
    (W[4] - (W[5] * (grade.n() - 1.0)).exp() + 1.0).clamp(1.0, 10.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(correct: bool, hesitated: bool, level: u8) -> Grade {
        Outcome {
            correct,
            hesitated,
            level,
        }
        .grade()
    }

    #[test]
    fn grades_follow_the_specs_table() {
        assert_eq!(outcome(false, false, 1), Grade::Again);
        assert_eq!(outcome(false, true, 3), Grade::Again);
        assert_eq!(outcome(true, true, 1), Grade::Hard);
        assert_eq!(outcome(true, false, 1), Grade::Good);
        assert_eq!(outcome(true, false, 2), Grade::Good);
        // Only a quick, unaided answer at level 3 counts as Easy.
        assert_eq!(outcome(true, false, 3), Grade::Easy);
    }

    #[test]
    fn a_better_first_grade_gives_a_longer_first_interval() {
        let days = |g| Memory::first(g).interval(0.9);
        assert!(days(Grade::Again) < days(Grade::Hard));
        assert!(days(Grade::Hard) < days(Grade::Good));
        assert!(days(Grade::Good) < days(Grade::Easy));
    }

    #[test]
    fn stability_is_the_interval_at_ninety_percent() {
        // The model is defined so that R(S, S) == 0.9.
        for stability in [1.0, 7.5, 60.0, 400.0] {
            let m = Memory {
                stability,
                difficulty: 5.0,
            };
            assert!(
                (m.retrievability(stability) - 0.9).abs() < 1e-3,
                "S={stability}"
            );
        }
    }

    #[test]
    fn retrievability_decays_from_one_towards_zero() {
        let m = Memory {
            stability: 10.0,
            difficulty: 5.0,
        };
        assert!((m.retrievability(0.0) - 1.0).abs() < 1e-6);
        let mut last = 1.0;
        for days in [1.0, 5.0, 10.0, 100.0, 10_000.0] {
            let r = m.retrievability(days);
            assert!(r < last && r > 0.0, "{days} days: {r}");
            last = r;
        }
    }

    #[test]
    fn a_higher_desired_retention_means_shorter_intervals() {
        // Spec 3.2 lets the user move retention between 0,8 and 0,95.
        let m = Memory {
            stability: 30.0,
            difficulty: 5.0,
        };
        assert!(m.interval(0.95) < m.interval(0.9));
        assert!(m.interval(0.9) < m.interval(0.8));
    }

    #[test]
    fn success_grows_stability_and_a_lapse_never_does() {
        let m = Memory::first(Grade::Good);
        let due = m.interval(0.9);
        assert!(m.review(Grade::Good, due).stability > m.stability);
        assert!(m.review(Grade::Easy, due).stability > m.review(Grade::Good, due).stability);
        let lapsed = m.review(Grade::Again, due);
        assert!(lapsed.stability <= m.stability, "{lapsed:?} vs {m:?}");
    }

    #[test]
    fn difficulty_rises_on_failure_and_eases_on_success() {
        let m = Memory::first(Grade::Good);
        assert!(m.review(Grade::Again, 5.0).difficulty > m.difficulty);
        assert!(m.review(Grade::Easy, 5.0).difficulty < m.difficulty);
    }

    #[test]
    fn difficulty_stays_inside_one_to_ten() {
        let mut m = Memory::first(Grade::Again);
        for _ in 0..200 {
            m = m.review(Grade::Again, 1.0);
            assert!((1.0..=10.0).contains(&m.difficulty), "{m:?}");
        }
        for _ in 0..200 {
            m = m.review(Grade::Easy, m.interval(0.9));
            assert!((1.0..=10.0).contains(&m.difficulty), "{m:?}");
        }
    }

    #[test]
    fn repeated_success_reaches_the_mastered_threshold() {
        // Spec 3.4 wants stability >= 60 days to be attainable, and in a
        // sensible number of reviews.
        let mut m = Memory::first(Grade::Good);
        let mut reviews = 0;
        while m.stability < 60.0 && reviews < 50 {
            m = m.review(Grade::Good, m.interval(0.9));
            reviews += 1;
        }
        assert!(
            m.stability >= 60.0,
            "stalled at {m:?} after {reviews} reviews"
        );
        assert!(reviews <= 10, "took {reviews} reviews");
    }

    #[test]
    fn numbers_stay_finite() {
        let mut m = Memory::first(Grade::Good);
        for i in 0..500 {
            let grade = match i % 4 {
                0 => Grade::Again,
                1 => Grade::Hard,
                2 => Grade::Good,
                _ => Grade::Easy,
            };
            m = m.review(grade, m.interval(0.9));
            assert!(
                m.stability.is_finite() && m.difficulty.is_finite(),
                "{i}: {m:?}"
            );
            assert!(m.stability > 0.0, "{i}: {m:?}");
        }
    }
}
