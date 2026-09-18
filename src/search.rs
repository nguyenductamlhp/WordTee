//! The lookup pipeline of spec 1.1: normalize, then four match tiers, then one
//! ranking pass.
//!
//! ```text
//! T1 exact       headword matches the typed form
//! T2 inflection  typed form is a form of one or more lemmas ("saw" -> see)
//! T3 prefix      B-tree range scan, ranked by frequency, not alphabetically
//! T4 fuzzy       only when T1-T3 returned < 5 hits
//! ```
//!
//! Ranking is the spec's: `tier ASC, edit_distance ASC, personal_boost DESC,
//! frequency_rank ASC`.

use crate::dict::{Dict, SenseId, Word, WordId};

/// Spec 1.1: tier T4 only runs when the cheaper tiers came up short.
const FUZZY_TRIGGER: usize = 5;
/// Spec 1.1: "<= 4 ký tự -> tối đa 1 lỗi; >= 5 ký tự -> tối đa 2 lỗi".
fn edit_budget(len: usize) -> usize {
    if len <= 4 { 1 } else { 2 }
}

/// Which tier a hit came from. Ordering is the ranking order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    Exact,
    Inflection,
    Prefix,
    Fuzzy,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "khớp chính xác",
            Self::Inflection => "dạng biến thể",
            Self::Prefix => "bắt đầu bằng",
            Self::Fuzzy => "gần giống",
        }
    }
}

/// "…is the past tense of…", shown under a lemma reached through tier T2.
#[derive(Clone, Debug)]
pub struct FormNote {
    /// What the user actually typed.
    pub surface: String,
    pub tag: &'static str,
}

/// One headword the pipeline decided to offer.
#[derive(Clone, Debug)]
pub struct Hit {
    pub word: WordId,
    pub tier: Tier,
    pub edits: u8,
    pub form: Option<FormNote>,
}

/// Everything one query produced.
#[derive(Clone, Debug, Default)]
pub struct Results {
    /// English headwords, best first.
    pub words: Vec<Hit>,
    /// Senses whose Vietnamese definition matches — the Việt → Anh direction
    /// of spec 5.2. Only filled when the query looks Vietnamese.
    pub reverse: Vec<SenseId>,
}

impl Results {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.reverse.is_empty()
    }
}

// -------------------------------------------------------------------------
// normalization (spec 1.1, step 1)
// -------------------------------------------------------------------------

/// Latin letters that carry a diacritic, mapped to their base letter.
///
/// Generated to match `strip_accents()` in `scripts/build-dict.py` exactly:
/// the pack's search keys were folded with that function, so this table has to
/// agree with it or an exact match would miss. Sorted by code point.
const FOLD: [(u32, char); 490] = [
    (0x00C0, 'A'),
    (0x00C1, 'A'),
    (0x00C2, 'A'),
    (0x00C3, 'A'),
    (0x00C4, 'A'),
    (0x00C5, 'A'),
    (0x00C7, 'C'),
    (0x00C8, 'E'),
    (0x00C9, 'E'),
    (0x00CA, 'E'),
    (0x00CB, 'E'),
    (0x00CC, 'I'),
    (0x00CD, 'I'),
    (0x00CE, 'I'),
    (0x00CF, 'I'),
    (0x00D1, 'N'),
    (0x00D2, 'O'),
    (0x00D3, 'O'),
    (0x00D4, 'O'),
    (0x00D5, 'O'),
    (0x00D6, 'O'),
    (0x00D9, 'U'),
    (0x00DA, 'U'),
    (0x00DB, 'U'),
    (0x00DC, 'U'),
    (0x00DD, 'Y'),
    (0x00E0, 'a'),
    (0x00E1, 'a'),
    (0x00E2, 'a'),
    (0x00E3, 'a'),
    (0x00E4, 'a'),
    (0x00E5, 'a'),
    (0x00E7, 'c'),
    (0x00E8, 'e'),
    (0x00E9, 'e'),
    (0x00EA, 'e'),
    (0x00EB, 'e'),
    (0x00EC, 'i'),
    (0x00ED, 'i'),
    (0x00EE, 'i'),
    (0x00EF, 'i'),
    (0x00F1, 'n'),
    (0x00F2, 'o'),
    (0x00F3, 'o'),
    (0x00F4, 'o'),
    (0x00F5, 'o'),
    (0x00F6, 'o'),
    (0x00F9, 'u'),
    (0x00FA, 'u'),
    (0x00FB, 'u'),
    (0x00FC, 'u'),
    (0x00FD, 'y'),
    (0x00FF, 'y'),
    (0x0100, 'A'),
    (0x0101, 'a'),
    (0x0102, 'A'),
    (0x0103, 'a'),
    (0x0104, 'A'),
    (0x0105, 'a'),
    (0x0106, 'C'),
    (0x0107, 'c'),
    (0x0108, 'C'),
    (0x0109, 'c'),
    (0x010A, 'C'),
    (0x010B, 'c'),
    (0x010C, 'C'),
    (0x010D, 'c'),
    (0x010E, 'D'),
    (0x010F, 'd'),
    (0x0110, 'D'),
    (0x0111, 'd'),
    (0x0112, 'E'),
    (0x0113, 'e'),
    (0x0114, 'E'),
    (0x0115, 'e'),
    (0x0116, 'E'),
    (0x0117, 'e'),
    (0x0118, 'E'),
    (0x0119, 'e'),
    (0x011A, 'E'),
    (0x011B, 'e'),
    (0x011C, 'G'),
    (0x011D, 'g'),
    (0x011E, 'G'),
    (0x011F, 'g'),
    (0x0120, 'G'),
    (0x0121, 'g'),
    (0x0122, 'G'),
    (0x0123, 'g'),
    (0x0124, 'H'),
    (0x0125, 'h'),
    (0x0128, 'I'),
    (0x0129, 'i'),
    (0x012A, 'I'),
    (0x012B, 'i'),
    (0x012C, 'I'),
    (0x012D, 'i'),
    (0x012E, 'I'),
    (0x012F, 'i'),
    (0x0130, 'I'),
    (0x0134, 'J'),
    (0x0135, 'j'),
    (0x0136, 'K'),
    (0x0137, 'k'),
    (0x0139, 'L'),
    (0x013A, 'l'),
    (0x013B, 'L'),
    (0x013C, 'l'),
    (0x013D, 'L'),
    (0x013E, 'l'),
    (0x0143, 'N'),
    (0x0144, 'n'),
    (0x0145, 'N'),
    (0x0146, 'n'),
    (0x0147, 'N'),
    (0x0148, 'n'),
    (0x014C, 'O'),
    (0x014D, 'o'),
    (0x014E, 'O'),
    (0x014F, 'o'),
    (0x0150, 'O'),
    (0x0151, 'o'),
    (0x0154, 'R'),
    (0x0155, 'r'),
    (0x0156, 'R'),
    (0x0157, 'r'),
    (0x0158, 'R'),
    (0x0159, 'r'),
    (0x015A, 'S'),
    (0x015B, 's'),
    (0x015C, 'S'),
    (0x015D, 's'),
    (0x015E, 'S'),
    (0x015F, 's'),
    (0x0160, 'S'),
    (0x0161, 's'),
    (0x0162, 'T'),
    (0x0163, 't'),
    (0x0164, 'T'),
    (0x0165, 't'),
    (0x0168, 'U'),
    (0x0169, 'u'),
    (0x016A, 'U'),
    (0x016B, 'u'),
    (0x016C, 'U'),
    (0x016D, 'u'),
    (0x016E, 'U'),
    (0x016F, 'u'),
    (0x0170, 'U'),
    (0x0171, 'u'),
    (0x0172, 'U'),
    (0x0173, 'u'),
    (0x0174, 'W'),
    (0x0175, 'w'),
    (0x0176, 'Y'),
    (0x0177, 'y'),
    (0x0178, 'Y'),
    (0x0179, 'Z'),
    (0x017A, 'z'),
    (0x017B, 'Z'),
    (0x017C, 'z'),
    (0x017D, 'Z'),
    (0x017E, 'z'),
    (0x01A0, 'O'),
    (0x01A1, 'o'),
    (0x01AF, 'U'),
    (0x01B0, 'u'),
    (0x01CD, 'A'),
    (0x01CE, 'a'),
    (0x01CF, 'I'),
    (0x01D0, 'i'),
    (0x01D1, 'O'),
    (0x01D2, 'o'),
    (0x01D3, 'U'),
    (0x01D4, 'u'),
    (0x01D5, 'U'),
    (0x01D6, 'u'),
    (0x01D7, 'U'),
    (0x01D8, 'u'),
    (0x01D9, 'U'),
    (0x01DA, 'u'),
    (0x01DB, 'U'),
    (0x01DC, 'u'),
    (0x01DE, 'A'),
    (0x01DF, 'a'),
    (0x01E0, 'A'),
    (0x01E1, 'a'),
    (0x01E6, 'G'),
    (0x01E7, 'g'),
    (0x01E8, 'K'),
    (0x01E9, 'k'),
    (0x01EA, 'O'),
    (0x01EB, 'o'),
    (0x01EC, 'O'),
    (0x01ED, 'o'),
    (0x01F0, 'j'),
    (0x01F4, 'G'),
    (0x01F5, 'g'),
    (0x01F8, 'N'),
    (0x01F9, 'n'),
    (0x01FA, 'A'),
    (0x01FB, 'a'),
    (0x0200, 'A'),
    (0x0201, 'a'),
    (0x0202, 'A'),
    (0x0203, 'a'),
    (0x0204, 'E'),
    (0x0205, 'e'),
    (0x0206, 'E'),
    (0x0207, 'e'),
    (0x0208, 'I'),
    (0x0209, 'i'),
    (0x020A, 'I'),
    (0x020B, 'i'),
    (0x020C, 'O'),
    (0x020D, 'o'),
    (0x020E, 'O'),
    (0x020F, 'o'),
    (0x0210, 'R'),
    (0x0211, 'r'),
    (0x0212, 'R'),
    (0x0213, 'r'),
    (0x0214, 'U'),
    (0x0215, 'u'),
    (0x0216, 'U'),
    (0x0217, 'u'),
    (0x0218, 'S'),
    (0x0219, 's'),
    (0x021A, 'T'),
    (0x021B, 't'),
    (0x021E, 'H'),
    (0x021F, 'h'),
    (0x0226, 'A'),
    (0x0227, 'a'),
    (0x0228, 'E'),
    (0x0229, 'e'),
    (0x022A, 'O'),
    (0x022B, 'o'),
    (0x022C, 'O'),
    (0x022D, 'o'),
    (0x022E, 'O'),
    (0x022F, 'o'),
    (0x0230, 'O'),
    (0x0231, 'o'),
    (0x0232, 'Y'),
    (0x0233, 'y'),
    (0x1E00, 'A'),
    (0x1E01, 'a'),
    (0x1E02, 'B'),
    (0x1E03, 'b'),
    (0x1E04, 'B'),
    (0x1E05, 'b'),
    (0x1E06, 'B'),
    (0x1E07, 'b'),
    (0x1E08, 'C'),
    (0x1E09, 'c'),
    (0x1E0A, 'D'),
    (0x1E0B, 'd'),
    (0x1E0C, 'D'),
    (0x1E0D, 'd'),
    (0x1E0E, 'D'),
    (0x1E0F, 'd'),
    (0x1E10, 'D'),
    (0x1E11, 'd'),
    (0x1E12, 'D'),
    (0x1E13, 'd'),
    (0x1E14, 'E'),
    (0x1E15, 'e'),
    (0x1E16, 'E'),
    (0x1E17, 'e'),
    (0x1E18, 'E'),
    (0x1E19, 'e'),
    (0x1E1A, 'E'),
    (0x1E1B, 'e'),
    (0x1E1C, 'E'),
    (0x1E1D, 'e'),
    (0x1E1E, 'F'),
    (0x1E1F, 'f'),
    (0x1E20, 'G'),
    (0x1E21, 'g'),
    (0x1E22, 'H'),
    (0x1E23, 'h'),
    (0x1E24, 'H'),
    (0x1E25, 'h'),
    (0x1E26, 'H'),
    (0x1E27, 'h'),
    (0x1E28, 'H'),
    (0x1E29, 'h'),
    (0x1E2A, 'H'),
    (0x1E2B, 'h'),
    (0x1E2C, 'I'),
    (0x1E2D, 'i'),
    (0x1E2E, 'I'),
    (0x1E2F, 'i'),
    (0x1E30, 'K'),
    (0x1E31, 'k'),
    (0x1E32, 'K'),
    (0x1E33, 'k'),
    (0x1E34, 'K'),
    (0x1E35, 'k'),
    (0x1E36, 'L'),
    (0x1E37, 'l'),
    (0x1E38, 'L'),
    (0x1E39, 'l'),
    (0x1E3A, 'L'),
    (0x1E3B, 'l'),
    (0x1E3C, 'L'),
    (0x1E3D, 'l'),
    (0x1E3E, 'M'),
    (0x1E3F, 'm'),
    (0x1E40, 'M'),
    (0x1E41, 'm'),
    (0x1E42, 'M'),
    (0x1E43, 'm'),
    (0x1E44, 'N'),
    (0x1E45, 'n'),
    (0x1E46, 'N'),
    (0x1E47, 'n'),
    (0x1E48, 'N'),
    (0x1E49, 'n'),
    (0x1E4A, 'N'),
    (0x1E4B, 'n'),
    (0x1E4C, 'O'),
    (0x1E4D, 'o'),
    (0x1E4E, 'O'),
    (0x1E4F, 'o'),
    (0x1E50, 'O'),
    (0x1E51, 'o'),
    (0x1E52, 'O'),
    (0x1E53, 'o'),
    (0x1E54, 'P'),
    (0x1E55, 'p'),
    (0x1E56, 'P'),
    (0x1E57, 'p'),
    (0x1E58, 'R'),
    (0x1E59, 'r'),
    (0x1E5A, 'R'),
    (0x1E5B, 'r'),
    (0x1E5C, 'R'),
    (0x1E5D, 'r'),
    (0x1E5E, 'R'),
    (0x1E5F, 'r'),
    (0x1E60, 'S'),
    (0x1E61, 's'),
    (0x1E62, 'S'),
    (0x1E63, 's'),
    (0x1E64, 'S'),
    (0x1E65, 's'),
    (0x1E66, 'S'),
    (0x1E67, 's'),
    (0x1E68, 'S'),
    (0x1E69, 's'),
    (0x1E6A, 'T'),
    (0x1E6B, 't'),
    (0x1E6C, 'T'),
    (0x1E6D, 't'),
    (0x1E6E, 'T'),
    (0x1E6F, 't'),
    (0x1E70, 'T'),
    (0x1E71, 't'),
    (0x1E72, 'U'),
    (0x1E73, 'u'),
    (0x1E74, 'U'),
    (0x1E75, 'u'),
    (0x1E76, 'U'),
    (0x1E77, 'u'),
    (0x1E78, 'U'),
    (0x1E79, 'u'),
    (0x1E7A, 'U'),
    (0x1E7B, 'u'),
    (0x1E7C, 'V'),
    (0x1E7D, 'v'),
    (0x1E7E, 'V'),
    (0x1E7F, 'v'),
    (0x1E80, 'W'),
    (0x1E81, 'w'),
    (0x1E82, 'W'),
    (0x1E83, 'w'),
    (0x1E84, 'W'),
    (0x1E85, 'w'),
    (0x1E86, 'W'),
    (0x1E87, 'w'),
    (0x1E88, 'W'),
    (0x1E89, 'w'),
    (0x1E8A, 'X'),
    (0x1E8B, 'x'),
    (0x1E8C, 'X'),
    (0x1E8D, 'x'),
    (0x1E8E, 'Y'),
    (0x1E8F, 'y'),
    (0x1E90, 'Z'),
    (0x1E91, 'z'),
    (0x1E92, 'Z'),
    (0x1E93, 'z'),
    (0x1E94, 'Z'),
    (0x1E95, 'z'),
    (0x1E96, 'h'),
    (0x1E97, 't'),
    (0x1E98, 'w'),
    (0x1E99, 'y'),
    (0x1EA0, 'A'),
    (0x1EA1, 'a'),
    (0x1EA2, 'A'),
    (0x1EA3, 'a'),
    (0x1EA4, 'A'),
    (0x1EA5, 'a'),
    (0x1EA6, 'A'),
    (0x1EA7, 'a'),
    (0x1EA8, 'A'),
    (0x1EA9, 'a'),
    (0x1EAA, 'A'),
    (0x1EAB, 'a'),
    (0x1EAC, 'A'),
    (0x1EAD, 'a'),
    (0x1EAE, 'A'),
    (0x1EAF, 'a'),
    (0x1EB0, 'A'),
    (0x1EB1, 'a'),
    (0x1EB2, 'A'),
    (0x1EB3, 'a'),
    (0x1EB4, 'A'),
    (0x1EB5, 'a'),
    (0x1EB6, 'A'),
    (0x1EB7, 'a'),
    (0x1EB8, 'E'),
    (0x1EB9, 'e'),
    (0x1EBA, 'E'),
    (0x1EBB, 'e'),
    (0x1EBC, 'E'),
    (0x1EBD, 'e'),
    (0x1EBE, 'E'),
    (0x1EBF, 'e'),
    (0x1EC0, 'E'),
    (0x1EC1, 'e'),
    (0x1EC2, 'E'),
    (0x1EC3, 'e'),
    (0x1EC4, 'E'),
    (0x1EC5, 'e'),
    (0x1EC6, 'E'),
    (0x1EC7, 'e'),
    (0x1EC8, 'I'),
    (0x1EC9, 'i'),
    (0x1ECA, 'I'),
    (0x1ECB, 'i'),
    (0x1ECC, 'O'),
    (0x1ECD, 'o'),
    (0x1ECE, 'O'),
    (0x1ECF, 'o'),
    (0x1ED0, 'O'),
    (0x1ED1, 'o'),
    (0x1ED2, 'O'),
    (0x1ED3, 'o'),
    (0x1ED4, 'O'),
    (0x1ED5, 'o'),
    (0x1ED6, 'O'),
    (0x1ED7, 'o'),
    (0x1ED8, 'O'),
    (0x1ED9, 'o'),
    (0x1EDA, 'O'),
    (0x1EDB, 'o'),
    (0x1EDC, 'O'),
    (0x1EDD, 'o'),
    (0x1EDE, 'O'),
    (0x1EDF, 'o'),
    (0x1EE0, 'O'),
    (0x1EE1, 'o'),
    (0x1EE2, 'O'),
    (0x1EE3, 'o'),
    (0x1EE4, 'U'),
    (0x1EE5, 'u'),
    (0x1EE6, 'U'),
    (0x1EE7, 'u'),
    (0x1EE8, 'U'),
    (0x1EE9, 'u'),
    (0x1EEA, 'U'),
    (0x1EEB, 'u'),
    (0x1EEC, 'U'),
    (0x1EED, 'u'),
    (0x1EEE, 'U'),
    (0x1EEF, 'u'),
    (0x1EF0, 'U'),
    (0x1EF1, 'u'),
    (0x1EF2, 'Y'),
    (0x1EF3, 'y'),
    (0x1EF4, 'Y'),
    (0x1EF5, 'y'),
    (0x1EF6, 'Y'),
    (0x1EF7, 'y'),
    (0x1EF8, 'Y'),
    (0x1EF9, 'y'),
];

/// Strips one diacritic, leaving anything else alone.
fn fold(c: char) -> char {
    let cp = c as u32;
    match FOLD.binary_search_by_key(&cp, |&(k, _)| k) {
        Ok(i) => FOLD[i].1,
        Err(_) => c,
    }
}

/// Lowercase, accent-free, whitespace collapsed — the pack's search key.
///
/// Hyphens, apostrophes and spaces survive so `mother-in-law`, `don't` and
/// `give up` stay distinct headwords; curly apostrophes fold to straight ones
/// so both spellings find the same entry.
pub fn normalize(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut pending_space = false;
    for c in query.chars() {
        let c = match c {
            '\u{2019}' | '\u{2018}' => '\'',
            c => c,
        };
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        for lower in fold(c).to_lowercase() {
            out.push(lower);
        }
    }
    out
}

/// Does the query carry a diacritic? That is what routes it to the Vietnamese →
/// English flow in spec 1.1, step 2.
pub fn looks_vietnamese(query: &str) -> bool {
    query.chars().any(|c| fold(c) != c)
}

/// Splits a Vietnamese string the same way `scripts/build-dict.py` did when it
/// built the reverse index — diacritics kept, since they are the signal.
pub fn vi_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.retain(|t| t.chars().count() > 1);
    out
}

// -------------------------------------------------------------------------
// tier T2: morphology
// -------------------------------------------------------------------------

/// Irregular forms, which no suffix rule can reach.
///
/// The pack's inflection table covers the irregulars the source dictionary
/// happens to gloss ("saw" -> see, "went" -> go), but it says nothing about
/// `made`, `came`, `found`, `ran` or `felt` — all of them in the thousand most
/// frequent English words. Both members of each pair are checked against the
/// dictionary before anything is offered, so a pair the pack does not have
/// simply does nothing.
/// `true` marks a verb form (past or participle); `false` an irregular plural.
const IRREGULAR: [(&str, &str, bool); 201] = [
    ("analyses", "analysis", false),
    ("appendices", "appendix", false),
    ("ate", "eat", true),
    ("bases", "basis", false),
    ("beaten", "beat", true),
    ("became", "become", true),
    ("been", "be", true),
    ("began", "begin", true),
    ("begun", "begin", true),
    ("bent", "bend", true),
    ("bit", "bite", true),
    ("bitten", "bite", true),
    ("bled", "bleed", true),
    ("blew", "blow", true),
    ("blown", "blow", true),
    ("bore", "bear", true),
    ("born", "bear", true),
    ("borne", "bear", true),
    ("bought", "buy", true),
    ("bound", "bind", true),
    ("bred", "breed", true),
    ("broke", "break", true),
    ("broken", "break", true),
    ("brought", "bring", true),
    ("built", "build", true),
    ("burnt", "burn", true),
    ("burst", "burst", true),
    ("cacti", "cactus", false),
    ("came", "come", true),
    ("caught", "catch", true),
    ("children", "child", false),
    ("chose", "choose", true),
    ("chosen", "choose", true),
    ("clung", "cling", true),
    ("cost", "cost", true),
    ("crept", "creep", true),
    ("crises", "crisis", false),
    ("criteria", "criterion", false),
    ("data", "datum", false),
    ("dealt", "deal", true),
    ("did", "do", true),
    ("done", "do", true),
    ("drank", "drink", true),
    ("drawn", "draw", true),
    ("dreamt", "dream", true),
    ("drew", "draw", true),
    ("driven", "drive", true),
    ("drove", "drive", true),
    ("drunk", "drink", true),
    ("dug", "dig", true),
    ("eaten", "eat", true),
    ("fallen", "fall", true),
    ("fed", "feed", true),
    ("feet", "foot", false),
    ("fell", "fall", true),
    ("felt", "feel", true),
    ("fled", "flee", true),
    ("flew", "fly", true),
    ("flown", "fly", true),
    ("flung", "fling", true),
    ("forbade", "forbid", true),
    ("forbidden", "forbid", true),
    ("forgave", "forgive", true),
    ("forgiven", "forgive", true),
    ("forgot", "forget", true),
    ("forgotten", "forget", true),
    ("fought", "fight", true),
    ("found", "find", true),
    ("froze", "freeze", true),
    ("frozen", "freeze", true),
    ("fungi", "fungus", false),
    ("gave", "give", true),
    ("geese", "goose", false),
    ("given", "give", true),
    ("gone", "go", true),
    ("got", "get", true),
    ("gotten", "get", true),
    ("grew", "grow", true),
    ("ground", "grind", true),
    ("grown", "grow", true),
    ("had", "have", true),
    ("heard", "hear", true),
    ("held", "hold", true),
    ("hid", "hide", true),
    ("hidden", "hide", true),
    ("hung", "hang", true),
    ("indices", "index", false),
    ("kept", "keep", true),
    ("knelt", "kneel", true),
    ("knew", "know", true),
    ("known", "know", true),
    ("laid", "lay", true),
    ("lain", "lie", true),
    ("leant", "lean", true),
    ("leapt", "leap", true),
    ("learnt", "learn", true),
    ("led", "lead", true),
    ("left", "leave", true),
    ("lent", "lend", true),
    ("lice", "louse", false),
    ("lit", "light", true),
    ("lost", "lose", true),
    ("made", "make", true),
    ("matrices", "matrix", false),
    ("meant", "mean", true),
    ("media", "medium", false),
    ("men", "man", false),
    ("met", "meet", true),
    ("mice", "mouse", false),
    ("nuclei", "nucleus", false),
    ("oxen", "ox", false),
    ("paid", "pay", true),
    ("people", "person", false),
    ("phenomena", "phenomenon", false),
    ("radii", "radius", false),
    ("ran", "run", true),
    ("rang", "ring", true),
    ("ridden", "ride", true),
    ("risen", "rise", true),
    ("rode", "ride", true),
    ("rose", "rise", true),
    ("rung", "ring", true),
    ("said", "say", true),
    ("sang", "sing", true),
    ("sank", "sink", true),
    ("sat", "sit", true),
    ("saw", "see", true),
    ("seen", "see", true),
    ("sent", "send", true),
    ("sewn", "sew", true),
    ("shaken", "shake", true),
    ("shed", "shed", true),
    ("shone", "shine", true),
    ("shook", "shake", true),
    ("shot", "shoot", true),
    ("shown", "show", true),
    ("shrank", "shrink", true),
    ("shrunk", "shrink", true),
    ("slept", "sleep", true),
    ("slid", "slide", true),
    ("sold", "sell", true),
    ("sought", "seek", true),
    ("spat", "spit", true),
    ("sped", "speed", true),
    ("spelt", "spell", true),
    ("spent", "spend", true),
    ("spilt", "spill", true),
    ("split", "split", true),
    ("spoke", "speak", true),
    ("spoken", "speak", true),
    ("sprang", "spring", true),
    ("spread", "spread", true),
    ("sprung", "spring", true),
    ("spun", "spin", true),
    ("stank", "stink", true),
    ("stimuli", "stimulus", false),
    ("stole", "steal", true),
    ("stolen", "steal", true),
    ("stood", "stand", true),
    ("struck", "strike", true),
    ("stuck", "stick", true),
    ("stung", "sting", true),
    ("stunk", "stink", true),
    ("sung", "sing", true),
    ("sunk", "sink", true),
    ("swam", "swim", true),
    ("swept", "sweep", true),
    ("swore", "swear", true),
    ("sworn", "swear", true),
    ("swum", "swim", true),
    ("swung", "swing", true),
    ("taken", "take", true),
    ("taught", "teach", true),
    ("teeth", "tooth", false),
    ("theses", "thesis", false),
    ("thought", "think", true),
    ("threw", "throw", true),
    ("thrown", "throw", true),
    ("told", "tell", true),
    ("took", "take", true),
    ("tore", "tear", true),
    ("torn", "tear", true),
    ("understood", "understand", true),
    ("vertices", "vertex", false),
    ("was", "be", true),
    ("went", "go", true),
    ("wept", "weep", true),
    ("were", "be", true),
    ("withdrawn", "withdraw", true),
    ("withdrew", "withdraw", true),
    ("woke", "wake", true),
    ("woken", "wake", true),
    ("women", "woman", false),
    ("won", "win", true),
    ("wore", "wear", true),
    ("worn", "wear", true),
    ("wound", "wind", true),
    ("wove", "weave", true),
    ("woven", "weave", true),
    ("written", "write", true),
    ("wrote", "write", true),
];

/// Lemma candidates for an inflected form, by rule.
///
/// The pack carries an explicit inflection table (spec 1.1), but the source
/// dictionary only spells that out for the entries that happen to have a
/// "số nhiều của …" gloss — `swimming` and `leaves`, for instance, have none.
/// These rules cover the regular remainder. Every candidate is checked against
/// the dictionary before it is offered, so an over-eager rule costs nothing.
fn rule_lemmas(norm: &str) -> Vec<(String, &'static str)> {
    const PLURAL: &str = "số nhiều / ngôi 3 số ít";
    const PAST: &str = "quá khứ / phân từ II";
    const GERUND: &str = "hiện tại phân từ / danh động từ";
    const COMPARATIVE: &str = "so sánh hơn";
    const SUPERLATIVE: &str = "so sánh nhất";
    const ADVERB: &str = "trạng từ";

    let n = norm.chars().count();
    let mut out: Vec<(String, &'static str)> = Vec::new();

    // Irregulars first: no suffix rule turns "made" into "make".
    for (form, lemma, is_verb) in IRREGULAR {
        if form == norm {
            out.push((lemma.to_string(), if is_verb { PAST } else { PLURAL }));
        }
    }
    let mut push = |stem: &str, extra: &str, tag: &'static str| {
        if stem.chars().count() >= 2 {
            out.push((format!("{stem}{extra}"), tag));
        }
    };
    // `stem(k)` is `norm` without its last `k` characters.
    let stem = |k: usize| -> &str { &norm[..norm.len() - k] };
    // "stopped" -> "stop": the suffix was preceded by a doubled consonant.
    let undouble = |s: &str| -> Option<String> {
        let mut it = s.chars().rev();
        let (a, b) = (it.next()?, it.next()?);
        (a == b && a.is_alphabetic() && !"aeiou".contains(a))
            .then(|| s[..s.len() - a.len_utf8()].to_owned())
    };

    if n > 4 && norm.ends_with("ies") {
        push(stem(3), "y", PLURAL);
    }
    if n > 3 && norm.ends_with("ves") {
        push(stem(3), "f", PLURAL);
        push(stem(3), "fe", PLURAL);
    }
    if n > 3 && (norm.ends_with("es") && !norm.ends_with("ses")) {
        push(stem(2), "", PLURAL);
    }
    if n > 2 && norm.ends_with('s') && !norm.ends_with("ss") && !norm.ends_with("us") {
        push(stem(1), "", PLURAL);
    }
    if n > 4 && norm.ends_with("ied") {
        push(stem(3), "y", PAST);
    }
    if n > 3 && norm.ends_with("ed") {
        push(stem(2), "", PAST);
        push(stem(1), "", PAST); // "hoped" -> "hope"
        if let Some(s) = undouble(stem(2)) {
            push(&s, "", PAST); // "stopped" -> "stop"
        }
    }
    if n > 4 && norm.ends_with("ing") {
        push(stem(3), "", GERUND);
        push(stem(3), "e", GERUND); // "hoping" -> "hope"
        if let Some(s) = undouble(stem(3)) {
            push(&s, "", GERUND); // "running" -> "run"
        }
    }
    if n > 4 && norm.ends_with("ier") {
        push(stem(3), "y", COMPARATIVE);
    }
    if n > 3 && norm.ends_with("er") {
        push(stem(2), "", COMPARATIVE);
        push(stem(1), "", COMPARATIVE);
        if let Some(s) = undouble(stem(2)) {
            push(&s, "", COMPARATIVE);
        }
    }
    if n > 5 && norm.ends_with("iest") {
        push(stem(4), "y", SUPERLATIVE);
    }
    if n > 4 && norm.ends_with("est") {
        push(stem(3), "", SUPERLATIVE);
        push(stem(2), "", SUPERLATIVE);
        if let Some(s) = undouble(stem(3)) {
            push(&s, "", SUPERLATIVE);
        }
    }
    if n > 4 && norm.ends_with("ly") {
        push(stem(2), "", ADVERB);
    }
    out.retain(|(w, _)| w != norm);
    out.dedup();
    out
}

// -------------------------------------------------------------------------
// tier T4: edit distance
// -------------------------------------------------------------------------

/// Reusable buffers for [`Osa::distance`], so the T4 scan allocates nothing.
#[derive(Default)]
struct Osa {
    prev2: Vec<usize>,
    prev: Vec<usize>,
    cur: Vec<usize>,
    b: Vec<char>,
}

impl Osa {
    /// Optimal string alignment distance, which counts a transposition as one
    /// edit — that is what catches the spec's "teh" → "the". Returns `None`
    /// as soon as the distance is known to exceed `max`.
    fn distance(&mut self, a: &[char], b_str: &str, max: usize) -> Option<usize> {
        self.b.clear();
        self.b.extend(b_str.chars());
        let (n, m) = (a.len(), self.b.len());
        if n.abs_diff(m) > max {
            return None;
        }
        for buf in [&mut self.prev2, &mut self.prev, &mut self.cur] {
            buf.clear();
            buf.resize(m + 1, 0);
        }
        for (j, slot) in self.prev.iter_mut().enumerate() {
            *slot = j;
        }
        for i in 1..=n {
            self.cur[0] = i;
            let mut row_min = i;
            for j in 1..=m {
                let cost = usize::from(a[i - 1] != self.b[j - 1]);
                let mut v = (self.prev[j] + 1)
                    .min(self.cur[j - 1] + 1)
                    .min(self.prev[j - 1] + cost);
                if i > 1 && j > 1 && a[i - 1] == self.b[j - 2] && a[i - 2] == self.b[j - 1] {
                    v = v.min(self.prev2[j - 2] + 1);
                }
                self.cur[j] = v;
                row_min = row_min.min(v);
            }
            if row_min > max {
                return None;
            }
            std::mem::swap(&mut self.prev2, &mut self.prev);
            std::mem::swap(&mut self.prev, &mut self.cur);
        }
        let d = self.prev[m];
        (d <= max).then_some(d)
    }
}

// -------------------------------------------------------------------------
// the pipeline
// -------------------------------------------------------------------------

/// Runs the whole pipeline. `boosted` marks headwords the user is already
/// learning or has looked up, which is the `personal_boost` of the spec's
/// ORDER BY.
pub fn lookup(dict: &Dict, query: &str, boosted: &dyn Fn(WordId) -> bool, limit: usize) -> Results {
    let norm = normalize(query);
    if norm.is_empty() {
        return Results::default();
    }
    // Tiers are collected first and de-duplicated afterwards: a prefix like "a"
    // matches thousands of headwords, and checking each new hit against the
    // ones already found would make that quadratic.
    let mut hits: Vec<Hit> = Vec::new();
    let plain = |word: WordId, tier: Tier| Hit {
        word,
        tier,
        edits: 0,
        form: None,
    };
    let lemma_of = |word: WordId, tag: &'static str| Hit {
        word,
        tier: Tier::Inflection,
        edits: 0,
        form: Some(FormNote {
            surface: norm.clone(),
            tag,
        }),
    };

    // T1 — exact. Spec 1.1: this always outranks the lemma it may also be a
    // form of, so "news" is never rewritten to "new".
    let typed = dict.exact(&norm);
    if let Some(w) = typed {
        hits.push(plain(w.id, Tier::Exact));
    }

    // T2 — inflections, from the pack's table first, then by rule.
    for (lemma, tag) in forms_of(dict, &norm) {
        hits.push(lemma_of(lemma.id, tag));
    }

    // T3 — prefix. Every match is collected, because the ranking below picks
    // the frequent ones; an alphabetical cut would keep the wrong words.
    hits.extend(dict.with_prefix(&norm).map(|w| plain(w.id, Tier::Prefix)));

    // T4 — fuzzy, only if the cheap tiers came up short.
    if hits.len() < FUZZY_TRIGGER {
        let chars: Vec<char> = norm.chars().collect();
        let max = edit_budget(chars.len());
        let mut osa = Osa::default();
        for id in 0..dict.word_count() {
            let candidate = dict.word_norm(id);
            // Only plain single words: nobody mistypes their way to the entry
            // "$100 hamburger", "12" or the affix "-ment", and skipping them
            // keeps the full-table scan inside the spec's 50 ms budget.
            if !candidate.chars().all(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            if let Some(d) = osa.distance(&chars, candidate, max) {
                hits.push(Hit {
                    word: id,
                    tier: Tier::Fuzzy,
                    edits: d as u8,
                    form: None,
                });
            }
        }
    }

    // One headword, one row. Grouping by headword puts each word's best tier
    // first within its run, and `dedup_by_key` keeps that one — so a word found
    // by both T1 and T3 is listed as the exact match it is.
    hits.sort_by_key(|h| (h.word, h.tier, h.edits));
    hits.dedup_by_key(|h| h.word);

    // Spec 1.1: tier, then edit distance, then what the user is working on,
    // then real-world frequency. Rank 0 means "not in the frequency list",
    // which has to sort last rather than first.
    hits.sort_by_key(|h| {
        let freq = match dict.word_freq(h.word) {
            0 => u32::MAX,
            r => r,
        };
        (h.tier, h.edits, !boosted(h.word), freq, h.word)
    });
    hits.truncate(limit);

    let reverse = if looks_vietnamese(query) {
        reverse_lookup(dict, query, limit)
    } else {
        Vec::new()
    };
    Results {
        words: hits,
        reverse,
    }
}

/// Every lemma `norm` could be an inflected form of, with its form tag.
///
/// Combines the two sources: the pack's inflection table where the source
/// dictionary spelled the relationship out, and the rules above for the
/// regular remainder. Used both by tier T2 and by the word page, which shows
/// the same information as spec 1.1's "cũng là dạng của …" line.
pub fn forms_of(dict: &Dict, norm: &str) -> Vec<(Word, &'static str)> {
    let mut out: Vec<(Word, &'static str)> = Vec::new();
    if let Some(w) = dict.exact(norm) {
        for (lemma, tag) in dict.inflection_of(w.id) {
            out.push((lemma, tag.label()));
        }
    }
    for (candidate, tag) in rule_lemmas(norm) {
        if let Some(w) = dict.exact(&candidate) {
            out.push((w, tag));
        }
    }
    out.dedup_by_key(|(w, _)| w.id);
    out
}

/// Is `candidate` within one edit of a real headword?
///
/// Spec 2.2 uses this to vet the placement test's pseudo-words: they must
/// "khác ít nhất 2 ký tự so với mọi từ thật", because a fake that is one letter
/// off a real word ("manticate" against "masticate") tests inference, not
/// vocabulary, and the user who answers it is not guessing.
pub fn near_a_real_word(dict: &Dict, candidate: &str) -> bool {
    let chars: Vec<char> = candidate.chars().collect();
    let mut osa = Osa::default();
    (0..dict.word_count()).any(|id| {
        let word = dict.word_norm(id);
        word.chars().all(|c| c.is_ascii_alphabetic()) && osa.distance(&chars, word, 1).is_some()
    })
}

/// Vietnamese → English (spec 5.2), over the pack's token index.
///
/// Senses matching every token come first; if nothing matches all of them, the
/// most selective single token is used instead, so a long query still answers.
pub fn reverse_lookup(dict: &Dict, query: &str, limit: usize) -> Vec<SenseId> {
    let tokens = vi_tokens(query);
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut lists: Vec<Vec<SenseId>> = tokens.iter().map(|t| dict.vi_postings(t)).collect();
    lists.retain(|l| !l.is_empty());
    if lists.is_empty() {
        return Vec::new();
    }
    // Rarest token first: the intersection can only shrink from there.
    lists.sort_by_key(Vec::len);
    let mut acc = lists[0].clone();
    for list in &lists[1..] {
        let next: Vec<SenseId> = acc
            .iter()
            .copied()
            .filter(|s| list.binary_search(s).is_ok())
            .collect();
        if next.is_empty() {
            break;
        }
        acc = next;
    }
    // Common words first: a learning item's rank is exactly that ordering, and
    // senses outside the learning list go last.
    acc.sort_by_key(|&s| {
        let sense = dict.sense(s);
        let rank = if sense.rank == 0 {
            u32::MAX
        } else {
            sense.rank
        };
        (sense.is_inflection, rank, s)
    });
    acc.truncate(limit);
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict() -> Dict {
        Dict::load()
    }

    fn words(dict: &Dict, r: &Results) -> Vec<&'static str> {
        r.words.iter().map(|h| dict.word(h.word).text).collect()
    }

    #[test]
    fn normalizes_the_way_the_pack_was_built() {
        assert_eq!(normalize("  Café   au  LAIT "), "cafe au lait");
        assert_eq!(normalize("Don\u{2019}t"), "don't");
        // Spec 1.1 keeps these three characters.
        assert_eq!(normalize("mother-in-law"), "mother-in-law");
        assert_eq!(normalize("give up"), "give up");
        assert_eq!(normalize("Tiếng Việt"), "tieng viet");
    }

    #[test]
    fn detects_the_vietnamese_flow() {
        assert!(looks_vietnamese("chào"));
        assert!(looks_vietnamese("quyết định"));
        assert!(!looks_vietnamese("decision"));
        assert!(!looks_vietnamese("don't"));
    }

    #[test]
    fn exact_match_comes_first() {
        let d = dict();
        let r = lookup(&d, "apple", &|_| false, 10);
        assert_eq!(r.words[0].tier, Tier::Exact);
        assert_eq!(d.word(r.words[0].word).text, "apple");
    }

    #[test]
    fn prefix_hits_are_ranked_by_frequency_not_alphabet() {
        // Spec 1.1's own example: appear/apply/approach come before the rare
        // apposition, even though "apposition" sorts earlier alphabetically.
        let d = dict();
        let names = words(&d, &lookup(&d, "app", &|_| false, 300));
        let pos = |w: &str| names.iter().position(|n| *n == w).expect(w);
        let rare = pos("apposition");
        for common in ["appear", "apply", "approach"] {
            assert!(pos(common) < rare, "{common} after apposition: {names:?}");
        }
    }

    #[test]
    fn an_exact_hit_is_never_displaced_by_its_lemma() {
        // Spec 1.1: "news" and "glasses" keep their own entry at the top.
        let d = dict();
        for (form, lemma) in [("news", "new"), ("glasses", "glass")] {
            let r = lookup(&d, form, &|_| false, 10);
            assert_eq!(d.word(r.words[0].word).text, form);
            let names = words(&d, &r);
            assert!(names.contains(&lemma), "{form}: {names:?}");
        }
    }

    #[test]
    fn maps_regular_inflections_onto_their_lemma() {
        let d = dict();
        for (form, lemma) in [
            ("swimming", "swim"),
            ("studies", "study"),
            ("stopped", "stop"),
            ("happier", "happy"), // "happier" is not itself a headword
            ("sadly", "sad"),     // …nor is "sadly"
            ("quickly", "quick"), // …but "quickly" is, and still finds "quick"
            // Irregulars, which no suffix rule reaches. All of these are in
            // the thousand most frequent English words.
            ("made", "make"),
            ("came", "come"),
            ("found", "find"),
            ("ran", "run"),
            ("felt", "feel"),
            ("spent", "spend"),
            ("children", "child"),
            ("feet", "foot"),
        ] {
            let names = words(&d, &lookup(&d, form, &|_| false, 25));
            assert!(names.contains(&lemma), "{form} -> {lemma}? got {names:?}");
        }
    }

    #[test]
    fn one_form_can_belong_to_several_lemmas() {
        // Spec 1.1: "leaves" is both leaf (plural) and leave (3rd person).
        let d = dict();
        let names = words(&d, &lookup(&d, "leaves", &|_| false, 25));
        assert!(names.contains(&"leaf"), "{names:?}");
        assert!(names.contains(&"leave"), "{names:?}");
    }

    #[test]
    fn fuzzy_catches_the_specs_two_typos() {
        let d = dict();
        for (typo, want) in [("teh", "the"), ("fonetic", "phonetic")] {
            let names = words(&d, &lookup(&d, typo, &|_| false, 15));
            assert!(names.contains(&want), "{typo} -> {want}? got {names:?}");
        }
    }

    #[test]
    fn fuzzy_stays_off_when_the_cheap_tiers_answer() {
        let d = dict();
        let r = lookup(&d, "app", &|_| false, 40);
        assert!(r.words.iter().all(|h| h.tier != Tier::Fuzzy));
    }

    #[test]
    fn personal_boost_lifts_a_word_the_user_is_learning() {
        let d = dict();
        let plain = words(&d, &lookup(&d, "app", &|_| false, 300));
        let target = d.exact("apposition").unwrap().id;
        let boosted = words(&d, &lookup(&d, "app", &|w| w == target, 300));
        let pos = |v: &Vec<&str>| v.iter().position(|n| *n == "apposition").unwrap();
        assert!(pos(&boosted) < pos(&plain), "{boosted:?}");
    }

    #[test]
    fn vietnamese_query_searches_definitions() {
        let d = dict();
        let r = lookup(&d, "quyết định", &|_| false, 20);
        assert!(!r.reverse.is_empty());
        let got: Vec<_> = r
            .reverse
            .iter()
            .map(|&s| d.word(d.sense(s).word).text)
            .collect();
        assert!(
            got.iter().any(|w| *w == "decide" || *w == "decision"),
            "{got:?}"
        );
    }

    #[test]
    fn english_query_does_not_run_the_reverse_index() {
        let d = dict();
        assert!(lookup(&d, "decision", &|_| false, 10).reverse.is_empty());
    }

    #[test]
    fn empty_query_returns_nothing() {
        let d = dict();
        assert!(lookup(&d, "   ", &|_| false, 10).is_empty());
    }
}
