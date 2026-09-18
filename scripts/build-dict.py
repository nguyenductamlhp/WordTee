#!/usr/bin/env python3
"""Build `assets/wordtee.dict`, the offline dictionary the app ships with.

Inputs (downloaded into a cache dir, both free to redistribute with attribution):

  * dictionary.db  -- https://github.com/minhqnd/dictionary (Wiktionary, CC BY-SA)
  * en_50k.txt     -- OpenSubtitles word counts from hermitdave/FrequencyWords
                      (CC BY-SA), the "spoken corpus" of the spec's section 2.1.

Output is one self-contained little-endian blob that `src/dict.rs` reads
zero-copy, so the app needs no SQLite, no C toolchain and no network. See
`docs/dict-format.md` for the layout this writes.

    python3 scripts/build-dict.py [--cache DIR] [--out FILE]
"""

from __future__ import annotations

import argparse
import math
import os
import re
import sqlite3
import struct
import sys
import unicodedata
import urllib.request
from collections import defaultdict

DB_URL = "https://github.com/minhqnd/dictionary/releases/download/v2.0.0/dictionary.db"
FREQ_URL = (
    "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/"
    "content/2018/en/en_50k.txt"
)

MAGIC = b"WTEEDICT"
VERSION = 1
HEADER_LEN = 96

# Learning-list shape, from spec 2.1: 25 blocks of 1.000 learning items.
LEARN_SIZE = 25_000
# Spec 1.2: at most 5 senses of a headword enter the learning path.
MAX_LEARN_SENSES = 5

# `definitions.pos` uses the source dictionary's one-letter codes. Index = the
# `pos` byte written into the pack; `src/dict.rs` has the matching table.
POS_CODES = ["?", "N", "V", "A", "D", "E", "C", "P", "M", "O", "S", "Z", "X"]
POS_INDEX = {code: i for i, code in enumerate(POS_CODES)}

KIND_SINGLE, KIND_PHRASE, KIND_AFFIX = 0, 1, 2

SENSE_OFFENSIVE = 1 << 0
SENSE_INFLECTION = 1 << 1  # "số nhiều của …" & friends: a pointer, not a sense

WORD_OFFENSIVE = 1 << 0

# Spec 1.1 wants a `word_inflections(inflection, lemma_id, form_tag)` table so
# that looking up "swimming" also offers "swim · present participle". The source
# dictionary has no such table, but it does carry one cross-reference *sense*
# per inflected headword, phrased from a small set of templates. Parsing those
# gives the table for free -- and tells us which senses are not learning items.
# Index = the `tag` byte written into the pack; `src/dict.rs` has the labels.
INFLECTION_PATTERNS = [
    (1, r"động từ chia ở ngôi thứ ba số ít"),
    (1, r"dạng ngôi thứ ba số ít[^;]*"),
    (2, r"(?:dạng )?quá khứ và phân từ quá khứ"),
    (2, r"dạng quá khứ đơn và phân từ quá khứ"),
    (2, r"(?:dạng )?phân từ quá khứ"),
    (2, r"(?:động từ |thì |dạng )?quá khứ(?: đơn)?"),
    (3, r"dạng phân từ hiện tại và danh động từ \(gerund\)"),
    (3, r"(?:dạng |thì )?(?:hiện tại )?phân từ hiện tại"),
    (3, r"(?:dạng )?hiện tại phân từ"),
    (3, r"danh động từ(?: \(gerund\))?"),
    (0, r"số nhiều"),
    (4, r"(?:cấp )?so sánh hơn"),
    (4, r"cấp so sánh"),
    (5, r"(?:cấp )?so sánh (?:nhất|cao nhất)"),
    (6, r"dạng (?:thay thế|viết khác|viết hoa|nhấn mạnh|viết phân biệt[^;]*)"),
    (6, r"cách viết khác"),
    (6, r"viết theo cách phát âm"),
    (6, r"(?:dạng không còn dùng|lỗi thời)"),
    (7, r"(?:dạng |từ )?viết tắt(?: từ chữ đầu[^;]*)?"),
    (7, r"\(viết tắt\)"),
    (8, r"(?:từ |dạng )?sai chính tả"),
]
INFLECTION_RE = [
    (tag, re.compile(r"^" + pat + r"\s+của\s+(.+?)\.?$", re.IGNORECASE))
    for tag, pat in INFLECTION_PATTERNS
]


def parse_inflection(definition: str):
    """`"Số nhiều của cat"` -> `(0, "cat")`, otherwise `None`."""
    for tag, rx in INFLECTION_RE:
        m = rx.match(definition)
        if m:
            lemma = m.group(1).strip().strip('"\u201c\u201d')
            # Only single headwords, never a whole explanatory clause.
            if lemma and len(lemma) < 40 and "," not in lemma:
                return tag, lemma
    return None

REL_KINDS = {"s": 0, "a": 1, "d": 2, "r": 3}  # synonym, antonym, derived, related

# Spec 2.1: drop bare function words from the learning path (they stay
# searchable). Only the small, purely grammatical set listed there.
FUNCTION_WORDS = {
    "the", "a", "an",
    "i", "you", "he", "she", "it", "we", "they", "me", "him", "her", "us", "them",
    "my", "your", "his", "its", "our", "their", "mine", "yours", "ours", "theirs",
    "be", "am", "is", "are", "was", "were", "been", "being",
    "have", "has", "had", "having", "do", "does", "did", "doing", "done",
    "of", "in", "on", "at", "to", "for", "with", "by", "from",
}

# Spec 2.1: coarse/offensive senses stay searchable but are never suggested.
OFFENSIVE = {
    "fuck", "fucker", "fucking", "shit", "shitty", "bullshit", "cunt", "cock",
    "dick", "pussy", "bitch", "bastard", "asshole", "arsehole", "whore", "slut",
    "nigger", "nigga", "faggot", "fag", "twat", "wanker", "prick", "piss",
    "motherfucker", "goddamn", "damn", "crap", "bollocks", "retard", "retarded",
}


# --------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------

def log(msg: str) -> None:
    print(f"  {msg}", file=sys.stderr, flush=True)


def fetch(url: str, path: str) -> str:
    """Download `url` to `path` unless it is already there."""
    if os.path.exists(path) and os.path.getsize(path) > 0:
        log(f"cached  {os.path.basename(path)} ({os.path.getsize(path)/1e6:.1f} MB)")
        return path
    log(f"fetch   {url}")
    tmp = path + ".part"
    with urllib.request.urlopen(url) as r, open(tmp, "wb") as f:
        while chunk := r.read(1 << 20):
            f.write(chunk)
    os.replace(tmp, path)
    log(f"saved   {os.path.basename(path)} ({os.path.getsize(path)/1e6:.1f} MB)")
    return path


def strip_accents(s: str) -> str:
    """Latin diacritics off, Vietnamese đ/Đ included (spec 1.1, normalization)."""
    s = s.replace("đ", "d").replace("Đ", "D")
    return "".join(
        c for c in unicodedata.normalize("NFD", s)
        if unicodedata.category(c) != "Mn"
    )


def normalize(s: str) -> str:
    """Search key: lowercase, accent-free, whitespace collapsed.

    Hyphens, apostrophes and spaces survive, so `mother-in-law`, `don't` and
    `give up` stay distinct headwords (spec 1.1).
    """
    s = s.replace("\u2019", "'").replace("\u2018", "'")
    return " ".join(strip_accents(s.lower()).split())


def vi_tokens(s: str) -> list[str]:
    """Vietnamese definition -> search tokens, diacritics kept.

    Diacritics are what tells a Vietnamese query apart from an English one, so
    unlike `normalize` this keeps them.
    """
    out, cur = [], []
    for ch in s.lower():
        if ch.isalnum():
            cur.append(ch)
        elif cur:
            out.append("".join(cur))
            cur = []
    if cur:
        out.append("".join(cur))
    return [t for t in out if len(t) > 1]


def varint(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        out.append(b | (0x80 if n else 0))
        if not n:
            return bytes(out)


class Strings:
    """Deduplicating string pool. Offset 0 is always the empty string."""

    def __init__(self) -> None:
        self.buf = bytearray()
        self.seen: dict[str, int] = {}
        assert self.put("") == 0

    def put(self, s: str) -> int:
        if s in self.seen:
            return self.seen[s]
        off = len(self.buf)
        raw = s.encode("utf-8")
        self.buf += varint(len(raw)) + raw
        self.seen[s] = off
        return off


# --------------------------------------------------------------------------
# load
# --------------------------------------------------------------------------

def load_frequency(path: str) -> tuple[dict[str, int], dict[str, float]]:
    """OpenSubtitles counts -> (rank, Zipf) per word, following spec 2.1."""
    counts: dict[str, int] = {}
    for line in open(path, encoding="utf-8"):
        parts = line.split()
        if len(parts) == 2 and parts[0] not in counts:
            counts[parts[0]] = int(parts[1])
    total = sum(counts.values())
    rank, zipf = {}, {}
    for i, (w, c) in enumerate(sorted(counts.items(), key=lambda kv: -kv[1])):
        rank[w] = i + 1
        # f_pm = count / corpus_size * 1e6 ; Zipf = log10(f_pm) + 3
        zipf[w] = math.log10(c / total * 1e6) + 3.0
    log(f"freq    {len(rank)} words, corpus {total/1e6:.0f}M tokens")
    return rank, zipf


def load_dictionary(db_path: str):
    """All English headwords with Vietnamese senses, in source order."""
    con = sqlite3.connect(db_path)
    con.text_factory = str

    senses: dict[str, list] = defaultdict(list)
    translations: dict[str, list[str]] = {}
    seen: set[tuple[str, str]] = set()
    rows = con.execute(
        """
        SELECT w.word, d.pos, d.definition, wd.example
          FROM words w
          JOIN word_definitions wd ON wd.word_id = w.id
          JOIN definitions d ON d.id = wd.definition_id
         WHERE w.lang_code = 'en' AND d.definition_lang = 'vi'
         ORDER BY w.word, wd.id
        """
    )
    for word, pos, definition, example in rows:
        word = (word or "").strip()
        definition = " ".join((definition or "").split())
        if not word or not definition or definition == ".":
            continue
        key = (word, definition)
        if key in seen:
            continue
        seen.add(key)
        senses[word].append(
            (pos or "?", definition, " ".join((example or "").split()),
             parse_inflection(definition))
        )
    n_infl = sum(1 for v in senses.values() for s in v if s[3])
    log(f"senses  {sum(len(v) for v in senses.values())} over {len(senses)} headwords")
    log(f"infl    {n_infl} of them are inflection cross-references")

    # The Wiktionary half of the source has real holes: `why`, `city` and
    # `easy` have no entry at all, `new` and `eat` have an entry with no
    # definitions. The other half is a Vietnamese dictionary whose entries carry
    # English translations, and inverting those fills the gaps -- `why` -> "vì
    # sao", `city` -> "thành phố". Only used where we have nothing else, so it
    # never competes with a real entry.
    filled = 0
    for english, vietnamese in con.execute(
        """
        SELECT t.translation, w.word
          FROM translations t JOIN words w ON w.id = t.word_id
         WHERE t.lang_code = 'en' AND w.lang_code = 'vi'
         ORDER BY t.id
        """
    ):
        english = " ".join((english or "").strip().lower().split())
        vietnamese = " ".join((vietnamese or "").split())
        if not english or not vietnamese or english in senses:
            continue
        if not english.isascii() or not all(c.isalpha() or c in " -'" for c in english):
            continue
        gloss = translations.setdefault(english, [])
        if vietnamese not in gloss:
            gloss.append(vietnamese)
    for english, gloss in translations.items():
        if english not in senses:
            senses[english] = [("?", "; ".join(gloss[:6]), "", None)]
            filled += 1
    log(f"transl  {filled} headwords filled in from the translations table")

    ipa: dict[str, str] = {}
    for word, p, region in con.execute(
        """
        SELECT w.word, p.ipa, p.region
          FROM words w JOIN pronunciations p ON p.word_id = w.id
         WHERE w.lang_code = 'en' AND p.ipa != ''
         ORDER BY w.word, (p.region IS NULL), p.id
        """
    ):
        word = (word or "").strip()
        if word and word not in ipa:
            t = (p or "").strip().strip("/").strip()
            if t:
                ipa[word] = f"/{t}/"
    log(f"ipa     {len(ipa)} headwords")

    rels: dict[str, list[tuple[int, str]]] = defaultdict(list)
    for word, related, kind in con.execute(
        """
        SELECT w.word, r.related_word, r.relation_type
          FROM words w JOIN word_relations r ON r.word_id = w.id
         WHERE w.lang_code = 'en'
        """
    ):
        word = (word or "").strip()
        related = " ".join((related or "").split())
        k = REL_KINDS.get(kind)
        if word and related and related != word and k is not None:
            rels[word].append((k, related))
    con.close()
    log(f"rels    {sum(len(v) for v in rels.values())} relations")
    return senses, ipa, rels


# --------------------------------------------------------------------------
# ranking (spec 1.2 + 2.1)
# --------------------------------------------------------------------------

def word_kind(word: str) -> int:
    if word.startswith("-") or word.endswith("-"):
        return KIND_AFFIX
    return KIND_PHRASE if " " in word else KIND_SINGLE


def learnable(word: str, norm: str) -> bool:
    """Is this headword allowed into the learning path? (spec 2.1, filters)"""
    if word_kind(word) == KIND_AFFIX:
        return False
    if norm in FUNCTION_WORDS:
        return False
    if len(norm) < 2 and norm not in ("i", "a"):
        return False
    if word[:1].isupper():  # proper nouns are looked up, never taught
        return False
    return all(c.isalpha() or c in " -'" for c in norm)


def sense_ranks(words, senses, zipf):
    """Spread a headword's Zipf score over its senses, then rank globally.

    The spec's recipe (sample corpus sentences, have a model label each one with
    a sense, weight by the split) needs a tagged corpus we do not ship. This is
    the cheap stand-in it degrades to: senses keep the source dictionary's
    order, which puts the everyday sense first, and take a 1/(i+1) share of the
    headword's frequency. Ordering within a headword is therefore exact; the
    absolute split between headwords is an estimate.
    """
    scored = []
    for wi, word in enumerate(words):
        z = zipf.get(normalize(word))
        if z is None:
            continue
        norm = normalize(word)
        if not learnable(word, norm):
            continue
        # An inflected form is a pointer to its lemma, not something to learn.
        pool = [(i, s) for i, s in enumerate(senses[word]) if not s[3]][:MAX_LEARN_SENSES]
        if not pool:
            continue
        share = sum(1.0 / (i + 1) for i in range(len(pool)))
        for slot, (si, _) in enumerate(pool):
            scored.append((z + math.log10((1.0 / (slot + 1)) / share), wi, si))
    scored.sort(key=lambda t: -t[0])
    ranks: dict[tuple[int, int], int] = {}
    for i, (_, wi, si) in enumerate(scored[:LEARN_SIZE]):
        ranks[(wi, si)] = i + 1
    log(f"learn   {len(ranks)} learning items of {len(scored)} candidates")
    return ranks


# --------------------------------------------------------------------------
# write
# --------------------------------------------------------------------------

def build(db_path: str, freq_path: str, out_path: str) -> None:
    freq_rank, zipf = load_frequency(freq_path)
    senses, ipa, rels = load_dictionary(db_path)

    # Headwords sorted by search key: the reader binary-searches this for exact
    # and prefix matches (spec 1.1, tiers T1 and T3).
    words = sorted(senses.keys(), key=lambda w: (normalize(w), w))
    ranks = sense_ranks(words, senses, zipf)

    strings = Strings()
    word_recs = bytearray()
    sense_recs = bytearray()
    learn: list[tuple[int, int]] = []          # (sense_rank, sense_idx)
    postings: dict[str, list[int]] = defaultdict(list)
    rel_recs: list[tuple[int, int, int]] = []  # (word_idx, str_off, kind)

    index_of = {w: i for i, w in enumerate(words)}
    infl_recs: list[tuple[int, int, int]] = []  # (word_idx, lemma_idx, tag)

    sense_idx = 0
    for wi, word in enumerate(words):
        norm = normalize(word)
        offensive = norm in OFFENSIVE or any(p in OFFENSIVE for p in norm.split())
        start = sense_idx
        for pos, definition, example, infl in senses[word]:
            rank = ranks.get((wi, sense_idx - start), 0)
            flags = SENSE_OFFENSIVE if offensive else 0
            if infl:
                flags |= SENSE_INFLECTION
                tag, lemma = infl
                target = index_of.get(lemma, index_of.get(lemma.lower()))
                if target is not None and target != wi:
                    infl_recs.append((wi, target, tag))
            sense_recs += struct.pack(
                "<IIIIBBH",
                strings.put(definition),
                strings.put(example),
                rank,
                wi,
                POS_INDEX.get(pos, 0),
                flags,
                0,
            )
            if rank:
                learn.append((rank, sense_idx))
            # Vietnamese -> English reverse index (spec 5.2).
            for tok in set(vi_tokens(definition)):
                postings[tok].append(sense_idx)
            sense_idx += 1

        word_recs += struct.pack(
            "<IIIIIHBB",
            strings.put(word),
            strings.put(norm) if norm != word else strings.seen[word],
            strings.put(ipa.get(word, "")),
            freq_rank.get(norm, 0),
            start,
            sense_idx - start,
            word_kind(word),
            WORD_OFFENSIVE if offensive else 0,
        )

        for kind, related in rels.get(word, []):
            rel_recs.append((wi, strings.put(related), kind))

    learn.sort()
    learn_idx = b"".join(struct.pack("<I", s) for _, s in learn)

    # Sorted by inflected form, so the reader can binary-search it (spec 1.1 T2).
    infl_recs = sorted(set(infl_recs))
    infl_blob = b"".join(struct.pack("<IIBBH", w, l, t, 0, 0) for w, l, t in infl_recs)
    log(f"infl    {len(infl_recs)} inflection -> lemma mappings")

    rel_recs.sort()
    rel_blob = b"".join(struct.pack("<IIBBH", w, s, k, 0, 0) for w, s, k in rel_recs)

    # Token table is sorted so the reader can binary-search it; postings are
    # delta-encoded varints, which is what keeps the reverse index small.
    tok_table, post_blob = bytearray(), bytearray()
    for tok in sorted(postings):
        ids = postings[tok]
        off, prev = len(post_blob), 0
        for i in ids:
            post_blob += varint(i - prev)
            prev = i
        tok_table += struct.pack("<IIII", strings.put(tok), off, len(ids), 0)

    # Assemble: header, then each section 8-byte aligned.
    parts, offsets, cursor = [], {}, HEADER_LEN
    for name, blob in (
        ("words", bytes(word_recs)),
        ("senses", bytes(sense_recs)),
        ("learn", learn_idx),
        ("rels", rel_blob),
        ("infl", infl_blob),
        ("toks", bytes(tok_table)),
        ("posts", bytes(post_blob)),
        ("strs", bytes(strings.buf)),
    ):
        pad = (-cursor) % 8
        if pad:
            parts.append(b"\0" * pad)
            cursor += pad
        offsets[name] = cursor
        parts.append(blob)
        cursor += len(blob)
        log(f"{name:>7} {len(blob)/1e6:7.2f} MB")

    header = MAGIC + struct.pack(
        "<22I",
        VERSION,
        len(words),
        sense_idx,
        len(learn),
        offsets["words"],
        offsets["senses"],
        offsets["learn"],
        offsets["rels"],
        len(rel_recs),
        offsets["toks"],
        len(postings),
        offsets["posts"],
        offsets["strs"],
        len(strings.buf),
        LEARN_SIZE,
        offsets["infl"],
        len(infl_recs),
        *([0] * 5),
    )
    assert len(header) == HEADER_LEN, len(header)

    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    with open(out_path, "wb") as f:
        f.write(header)
        for p in parts:
            f.write(p)
    log(f"wrote   {out_path} ({os.path.getsize(out_path)/1e6:.1f} MB)")


def main() -> None:
    here = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--cache", default=os.path.join(here, "target", "dict-cache"))
    ap.add_argument("--out", default=os.path.join(here, "assets", "wordtee.dict"))
    args = ap.parse_args()

    os.makedirs(args.cache, exist_ok=True)
    db = fetch(DB_URL, os.path.join(args.cache, "dictionary.db"))
    freq = fetch(FREQ_URL, os.path.join(args.cache, "en_50k.txt"))
    build(db, freq, args.out)


if __name__ == "__main__":
    main()
