//! Reader for `assets/wordtee.dict`, the offline dictionary built by
//! `scripts/build-dict.py`.
//!
//! The pack is `include_bytes!`d into the executable, so the app carries its
//! own database: no SQLite, no file paths, no network, and the exact same code
//! on desktop, Android and the web. Every accessor borrows straight out of that
//! `&'static [u8]` — nothing is parsed or allocated at startup.
//!
//! Layout (little-endian), written by `scripts/build-dict.py`:
//!
//! ```text
//! magic "WTEEDICT" | header (96 B) | words | senses | learn | rels | infl
//!                  | tokens | postings | strings
//! ```

use std::ops::Range;

/// The database itself. ~19 MB of Wiktionary/TVTD data, see the README.
static PACK: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/wordtee.dict"));

const MAGIC: &[u8; 8] = b"WTEEDICT";
const WORD_REC: usize = 24;
const SENSE_REC: usize = 20;
const REL_REC: usize = 12;
const INFL_REC: usize = 12;
const TOK_REC: usize = 16;

/// Index of a headword, `0..Dict::word_count()`.
pub type WordId = u32;
/// Index of a sense. A sense with a `rank` is a Learning Item (spec 0.1).
pub type SenseId = u32;

// -------------------------------------------------------------------------
// small enums
// -------------------------------------------------------------------------

/// Part of speech, as the source dictionary tags it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pos(u8);

impl Pos {
    /// Short chip label, e.g. `n.`, `v.`, `adj.`.
    pub fn short(self) -> &'static str {
        const SHORT: [&str; 13] = [
            "", "n.", "v.", "adj.", "adv.", "prep.", "conj.", "pron.", "num.", "interj.", "idiom",
            "affix", "misc.",
        ];
        SHORT.get(self.0 as usize).copied().unwrap_or("")
    }

    /// Vietnamese name, spelled out.
    pub fn label(self) -> &'static str {
        const LABEL: [&str; 13] = [
            "",
            "noun",
            "verb",
            "adjective",
            "adverb",
            "preposition",
            "conjunction",
            "pronoun",
            "numeral",
            "interjection",
            "idiom",
            "affix",
            "other",
        ];
        LABEL.get(self.0 as usize).copied().unwrap_or("")
    }
}

/// Shape of a headword: spec 1.2 keeps phrases as entries in their own right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Single,
    Phrase,
    Affix,
}

/// Which grammatical form an inflected headword is (spec 1.1, `form_tag`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormTag(u8);

impl FormTag {
    pub fn label(self) -> &'static str {
        const LABEL: [&str; 9] = [
            "plural",
            "3rd person singular",
            "past / past participle",
            "present participle / gerund",
            "comparative",
            "superlative",
            "variant spelling",
            "abbreviation",
            "misspelling",
        ];
        LABEL
            .get(self.0 as usize)
            .copied()
            .unwrap_or("another form")
    }
}

/// How a related word relates to the headword (feeds the "Cách dùng" block in
/// spec 1.3 and the interference rule in spec 2.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Relation {
    Synonym,
    Antonym,
    Derived,
    Related,
}

impl Relation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Synonym => "Synonyms",
            Self::Antonym => "Antonyms",
            Self::Derived => "Word family",
            Self::Related => "Related",
        }
    }
}

/// The four tiers of spec 1.3's commonness indicator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Band {
    Core,
    Advanced,
    Academic,
    Rare,
}

impl Band {
    /// Which band a learning-item rank falls in. Rank 0 means "not in the
    /// learning list at all", which is the `Rare` bucket.
    pub fn of(rank: u32) -> Self {
        match rank {
            0 => Self::Rare,
            1..=3_000 => Self::Core,
            3_001..=10_000 => Self::Advanced,
            10_001..=25_000 => Self::Academic,
            _ => Self::Rare,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Advanced => "Advanced",
            Self::Academic => "Academic",
            Self::Rare => "Rare",
        }
    }

    /// Rank range covered, for the map legend.
    pub fn range(self) -> &'static str {
        match self {
            Self::Core => "1 – 3,000",
            Self::Advanced => "3,001 – 10,000",
            Self::Academic => "10,001 – 25,000",
            Self::Rare => "outside the learning list",
        }
    }
}

// -------------------------------------------------------------------------
// borrowed records
// -------------------------------------------------------------------------

/// One headword.
#[derive(Clone, Copy, Debug)]
pub struct Word {
    pub id: WordId,
    /// As it is spelled and displayed.
    pub text: &'static str,
    /// Lowercase, accent-free search key. Headwords are sorted by this.
    pub norm: &'static str,
    /// IPA including the slashes, or `""`.
    pub ipa: &'static str,
    /// Position in the OpenSubtitles frequency list; 0 when it is not in it.
    pub freq_rank: u32,
    pub kind: Kind,
    /// Coarse or offensive: searchable, never suggested (spec 2.1).
    pub offensive: bool,
    sense_start: SenseId,
    sense_count: u32,
}

impl Word {
    /// Every sense the dictionary lists, inflection pointers included.
    pub fn senses(&self) -> Range<SenseId> {
        self.sense_start..self.sense_start + self.sense_count
    }
}

/// One sense. A sense whose `rank` is non-zero is a Learning Item (spec 0.1).
#[derive(Clone, Copy, Debug)]
pub struct Sense {
    pub id: SenseId,
    pub word: WordId,
    /// Vietnamese definition.
    pub def: &'static str,
    /// Example sentence, or `""`. Only ~19k senses have one.
    pub example: &'static str,
    /// Learning-item rank, 1..=25000, or 0 when outside the learning list.
    pub rank: u32,
    pub pos: Pos,
    /// True for "số nhiều của …"-style entries: a pointer to a lemma rather
    /// than a meaning, so it is never shown as a sense card or taught.
    pub is_inflection: bool,
    pub offensive: bool,
}

impl Sense {
    /// Can this sense be put on the learning path? (spec 2.1)
    pub fn teachable(&self) -> bool {
        self.rank > 0 && !self.is_inflection && !self.offensive
    }

    pub fn band(&self) -> Band {
        Band::of(self.rank)
    }
}

// -------------------------------------------------------------------------
// the pack
// -------------------------------------------------------------------------

/// Section offsets, read once from the header.
pub struct Dict {
    words: usize,
    senses: usize,
    learn: usize,
    rels: usize,
    infl: usize,
    toks: usize,
    posts: usize,
    strs: usize,
    word_count: u32,
    sense_count: u32,
    learn_count: u32,
    rel_count: u32,
    infl_count: u32,
    tok_count: u32,
}

impl Dict {
    /// Parses the header of the embedded pack. Panics only if the pack was
    /// built by a different version of `scripts/build-dict.py`.
    pub fn load() -> Self {
        assert!(PACK.len() > 96, "dictionary pack is truncated");
        assert_eq!(&PACK[..8], MAGIC, "not a WordTee dictionary pack");
        let h = |i: usize| u32_at(PACK, 8 + i * 4) as usize;
        assert_eq!(h(0), 1, "unsupported pack version");
        Self {
            word_count: h(1) as u32,
            sense_count: h(2) as u32,
            learn_count: h(3) as u32,
            words: h(4),
            senses: h(5),
            learn: h(6),
            rels: h(7),
            rel_count: h(8) as u32,
            toks: h(9),
            tok_count: h(10) as u32,
            posts: h(11),
            strs: h(12),
            infl: h(15),
            infl_count: h(16) as u32,
        }
    }

    pub fn word_count(&self) -> u32 {
        self.word_count
    }

    pub fn sense_count(&self) -> u32 {
        self.sense_count
    }

    /// Size of the learning list — 25.000 items, spec 2.1.
    pub fn learn_count(&self) -> u32 {
        self.learn_count
    }

    // --- records ---------------------------------------------------------

    pub fn word(&self, id: WordId) -> Word {
        let at = self.words + id as usize * WORD_REC;
        let start = u32_at(PACK, at + 16);
        let count = u16_at(PACK, at + 20) as u32;
        Word {
            id,
            text: self.str_at(u32_at(PACK, at)),
            norm: self.str_at(u32_at(PACK, at + 4)),
            ipa: self.str_at(u32_at(PACK, at + 8)),
            freq_rank: u32_at(PACK, at + 12),
            kind: match PACK[at + 22] {
                1 => Kind::Phrase,
                2 => Kind::Affix,
                _ => Kind::Single,
            },
            offensive: PACK[at + 23] & 1 != 0,
            sense_start: start,
            sense_count: count,
        }
    }

    pub fn sense(&self, id: SenseId) -> Sense {
        let at = self.senses + id as usize * SENSE_REC;
        let flags = PACK[at + 17];
        Sense {
            id,
            word: u32_at(PACK, at + 12),
            def: self.str_at(u32_at(PACK, at)),
            example: self.str_at(u32_at(PACK, at + 4)),
            rank: u32_at(PACK, at + 8),
            pos: Pos(PACK[at + 16]),
            offensive: flags & 1 != 0,
            is_inflection: flags & 2 != 0,
        }
    }

    /// Just the search key of a headword. The tier T3/T4 scans touch every
    /// headword, so they read this rather than building a whole [`Word`].
    pub fn word_norm(&self, id: WordId) -> &'static str {
        self.str_at(u32_at(PACK, self.words + id as usize * WORD_REC + 4))
    }

    /// Just the frequency rank of a headword, for the same reason.
    pub fn word_freq(&self, id: WordId) -> u32 {
        u32_at(PACK, self.words + id as usize * WORD_REC + 12)
    }

    /// The learning item at `rank` (1-based), as spec 2.1 orders them.
    pub fn at_rank(&self, rank: u32) -> Option<Sense> {
        if rank == 0 || rank > self.learn_count {
            return None;
        }
        Some(self.sense(u32_at(PACK, self.learn + (rank as usize - 1) * 4)))
    }

    /// `(sense, rank)` for a span of learning-item ranks, reading only the
    /// rank index.
    ///
    /// The knowledge map tallies all 25.000 items every frame; going through
    /// [`Self::at_rank`] would decode two strings per item for nothing.
    pub fn learn_span(&self, ranks: Range<u32>) -> impl Iterator<Item = (SenseId, u32)> + '_ {
        let last = self.learn_count;
        ranks
            .filter(move |&r| r >= 1 && r <= last)
            .map(move |rank| (u32_at(PACK, self.learn + (rank as usize - 1) * 4), rank))
    }

    // --- lookup ----------------------------------------------------------

    /// Exact headword match on the normalized form (spec 1.1, tier T1).
    pub fn exact(&self, norm: &str) -> Option<Word> {
        let i = self.lower_bound(norm);
        (i < self.word_count && self.word_norm(i) == norm).then(|| self.word(i))
    }

    /// Every headword starting with `prefix` (spec 1.1, tier T3), in rank order
    /// of the underlying B-tree — i.e. alphabetical; the caller re-ranks.
    pub fn with_prefix(&self, prefix: &str) -> impl Iterator<Item = Word> + '_ {
        let start = self.lower_bound(prefix);
        let prefix = prefix.to_owned();
        (start..self.word_count)
            .take_while(move |&i| self.word_norm(i).starts_with(&prefix))
            .map(move |i| self.word(i))
    }

    /// First headword index whose norm is >= `key`.
    fn lower_bound(&self, key: &str) -> WordId {
        let (mut lo, mut hi) = (0u32, self.word_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.word_norm(mid) < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// Lemmas this headword is an inflected form of, with the form tag
    /// (spec 1.1, tier T2). A form can belong to several lemmas.
    pub fn inflection_of(&self, id: WordId) -> Vec<(Word, FormTag)> {
        self.records(self.infl, self.infl_count, INFL_REC, id)
            .map(|at| (self.word(u32_at(PACK, at + 4)), FormTag(PACK[at + 8])))
            .collect()
    }

    /// Synonyms, antonyms and word-family members (spec 1.3, "Cách dùng").
    pub fn relations(&self, id: WordId) -> Vec<(Relation, &'static str)> {
        self.records(self.rels, self.rel_count, REL_REC, id)
            .map(|at| {
                let kind = match PACK[at + 8] {
                    0 => Relation::Synonym,
                    1 => Relation::Antonym,
                    2 => Relation::Derived,
                    _ => Relation::Related,
                };
                (kind, self.str_at(u32_at(PACK, at + 4)))
            })
            .collect()
    }

    /// Byte offsets of the records in a `word_idx`-sorted section that belong
    /// to `id`. Both sections are sorted, so one binary search finds the run.
    fn records(
        &self,
        base: usize,
        count: u32,
        size: usize,
        id: WordId,
    ) -> impl Iterator<Item = usize> + '_ {
        let (mut lo, mut hi) = (0u32, count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if u32_at(PACK, base + mid as usize * size) < id {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (lo..count)
            .map(move |i| base + i as usize * size)
            .take_while(move |&at| u32_at(PACK, at) == id)
    }

    /// Senses whose Vietnamese definition contains `token` (spec 5.2, the
    /// Vietnamese → English direction). Postings are delta-encoded varints.
    pub fn vi_postings(&self, token: &str) -> Vec<SenseId> {
        let (mut lo, mut hi) = (0u32, self.tok_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let at = self.toks + mid as usize * TOK_REC;
            if self.str_at(u32_at(PACK, at)) < token {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo >= self.tok_count {
            return Vec::new();
        }
        let at = self.toks + lo as usize * TOK_REC;
        if self.str_at(u32_at(PACK, at)) != token {
            return Vec::new();
        }
        let (mut p, count) = (
            self.posts + u32_at(PACK, at + 4) as usize,
            u32_at(PACK, at + 8),
        );
        let mut out = Vec::with_capacity(count as usize);
        let mut acc = 0u32;
        for _ in 0..count {
            let (delta, next) = varint(PACK, p);
            p = next;
            acc += delta;
            out.push(acc);
        }
        out
    }

    /// Reads the length-prefixed string at `off` in the string pool.
    fn str_at(&self, off: u32) -> &'static str {
        if off == 0 {
            return "";
        }
        let (len, start) = varint(PACK, self.strs + off as usize);
        PACK.get(start..start + len as usize)
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }
}

// -------------------------------------------------------------------------
// primitives
// -------------------------------------------------------------------------

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

/// Reads a LEB128 varint, returning it and the offset just past it.
fn varint(b: &[u8], mut at: usize) -> (u32, usize) {
    let (mut n, mut shift) = (0u32, 0);
    loop {
        let byte = b[at];
        at += 1;
        n |= ((byte & 0x7f) as u32) << shift;
        if byte & 0x80 == 0 {
            return (n, at);
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        Dict::load()
    }

    #[test]
    fn header_is_consistent() {
        let d = dict();
        assert!(d.word_count() > 100_000, "{} headwords", d.word_count());
        assert_eq!(d.learn_count(), 25_000);
    }

    #[test]
    fn headwords_are_sorted_by_norm() {
        // `exact` and `with_prefix` binary-search this order.
        let d = dict();
        let mut prev = "";
        for i in 0..d.word_count() {
            let w = d.word(i);
            assert!(w.norm >= prev, "{prev:?} then {:?}", w.norm);
            prev = w.norm;
        }
    }

    #[test]
    fn looks_up_an_exact_headword() {
        let d = dict();
        let w = d.exact("hello").expect("hello is in the dictionary");
        assert_eq!(w.text, "hello");
        assert!(w.ipa.starts_with('/'));
        assert!(w.senses().len() > 1);
        assert!(w.freq_rank > 0);
    }

    #[test]
    fn prefix_search_stays_inside_the_prefix() {
        let d = dict();
        let hits: Vec<_> = d.with_prefix("appl").take(20).collect();
        assert!(hits.iter().all(|w| w.norm.starts_with("appl")));
        assert!(hits.iter().any(|w| w.text == "apple"));
    }

    #[test]
    fn inflections_point_at_their_lemma() {
        // Spec 1.1's own example: "saw" is both a lemma and a form of "see".
        let d = dict();
        let saw = d.exact("saw").unwrap();
        let lemmas = d.inflection_of(saw.id);
        assert!(lemmas.iter().any(|(w, _)| w.text == "see"), "{lemmas:?}");
        // …and it keeps its own noun senses.
        assert!(
            saw.senses()
                .map(|s| d.sense(s))
                .any(|s| !s.is_inflection && s.pos.short() == "n.")
        );
    }

    #[test]
    fn the_learning_list_is_ranked_and_teachable() {
        let d = dict();
        for rank in [1, 2, 500, 12_345, 25_000] {
            let s = d.at_rank(rank).expect("rank is in range");
            assert_eq!(s.rank, rank);
            assert!(s.teachable(), "rank {rank}: {s:?}");
        }
        assert!(d.at_rank(0).is_none());
        assert!(d.at_rank(25_001).is_none());
    }

    #[test]
    fn reverse_index_finds_english_from_vietnamese() {
        let d = dict();
        let hits = d.vi_postings("chào");
        assert!(!hits.is_empty());
        let words: Vec<_> = hits.iter().map(|&s| d.word(d.sense(s).word).text).collect();
        assert!(
            words.contains(&"hello"),
            "{:?}",
            &words[..words.len().min(20)]
        );
    }

    #[test]
    fn postings_are_ascending_and_in_range() {
        let d = dict();
        for token in ["chào", "nước", "người", "ăn"] {
            let hits = d.vi_postings(token);
            assert!(hits.windows(2).all(|w| w[0] < w[1]), "{token} unsorted");
            assert!(hits.iter().all(|&s| s < d.sense_count()));
        }
        assert!(d.vi_postings("zzzznotatoken").is_empty());
    }
}
