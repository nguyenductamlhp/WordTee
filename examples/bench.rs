//! Checks the lookup pipeline against the performance targets in spec 1.1:
//! suggestions after every keystroke in under 50 ms, inflection lookup under
//! 5 ms. Run with `cargo run --release --example bench`.

use std::time::Instant;
use wordtee::{dict::Dict, search};

fn time(label: &str, budget_ms: f64, runs: usize, mut body: impl FnMut() -> usize) {
    let started = Instant::now();
    let mut found = 0;
    for _ in 0..runs {
        found += body();
    }
    let each = started.elapsed().as_secs_f64() * 1e3 / runs as f64;
    let verdict = if each <= budget_ms { "ok" } else { "OVER" };
    println!("  {label:<34} {each:7.2} ms  (budget {budget_ms:>2} ms) {verdict}   {found} hits");
}

fn main() {
    let started = Instant::now();
    let dict = Dict::load();
    println!(
        "Dict::load()  {:.3} ms — {} headwords, {} senses, {} learning items\n",
        started.elapsed().as_secs_f64() * 1e3,
        dict.word_count(),
        dict.sense_count(),
        dict.learn_count()
    );

    let none = |_| false;
    // Typing "decision" one letter at a time is the real per-keystroke load.
    time("typing 'decision' (per keystroke)", 50.0, 8, || {
        (1..=8)
            .map(|n| {
                search::lookup(&dict, &"decision"[..n], &none, 30)
                    .words
                    .len()
            })
            .sum()
    });
    time("worst-case prefix 'a'", 50.0, 20, || {
        search::lookup(&dict, "a", &none, 30).words.len()
    });
    time("exact hit 'hello'", 50.0, 200, || {
        search::lookup(&dict, "hello", &none, 30).words.len()
    });
    time("inflection 'swimming'", 5.0, 200, || {
        search::lookup(&dict, "swimming", &none, 30).words.len()
    });
    // Tier T4 only runs when the cheap tiers came up short — this is it.
    time("fuzzy 'fonetic' (full T4 scan)", 50.0, 20, || {
        search::lookup(&dict, "fonetic", &none, 30).words.len()
    });
    time("fuzzy 'teh' (full T4 scan)", 50.0, 20, || {
        search::lookup(&dict, "teh", &none, 30).words.len()
    });
    time("reverse 'quyết định'", 50.0, 50, || {
        search::lookup(&dict, "quyết định", &none, 30).reverse.len()
    });

    coverage(&dict);
}

/// How much of the frequency list the pack can actually answer.
///
/// Counts a word as answered if the pipeline returns it as an exact hit or
/// resolves it to a lemma — "made" has no entry of its own, but finding "make"
/// is the right answer, not a miss. Needs the frequency list the build script
/// caches; without it this is skipped.
fn coverage(dict: &Dict) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dict-cache/en_50k.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        println!("\n(frequency list not cached — run scripts/build-dict.py for coverage)");
        return;
    };
    let words: Vec<&str> = text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();

    println!("\nCoverage of the OpenSubtitles frequency list:");
    let none = |_| false;
    for top in [1_000usize, 3_000, 10_000] {
        let band = &words[..top.min(words.len())];
        let (mut exact, mut lemma) = (0, 0);
        for word in band {
            let hits = search::lookup(dict, word, &none, 4).words;
            if hits.iter().any(|h| h.tier == search::Tier::Exact) {
                exact += 1;
            } else if hits.iter().any(|h| h.tier == search::Tier::Inflection) {
                lemma += 1;
            }
        }
        let answered = exact + lemma;
        println!(
            "  top {top:>6}   {answered:>5} answered ({:.1}%)  = {exact} direct + {lemma} via a lemma",
            100.0 * answered as f64 / band.len() as f64
        );
    }
}
