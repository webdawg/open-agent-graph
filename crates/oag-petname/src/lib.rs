//! Deterministic, human-legible peer display names.
//!
//! A `PeerId` (spec section 12) is a `BLAKE3(public_key)` hash — correct for
//! machines, unreadable for humans. This crate derives a 128-word name from
//! that same id: the same peer always gets the same name, and different
//! peers get different names because their underlying keys differ. The
//! `PeerId` remains the real cryptographic identity; this is purely a
//! display label (kept in its own leaf crate rather than `oag-crypto` so
//! the dictionary dependency below doesn't bloat every crate that merely
//! needs a `PeerId` — see the workspace `Cargo.toml`, only `oag-cli`
//! depends on this crate).
//!
//! ## Wordlist provenance and stability
//!
//! Words come from `open-english-pronouncing-dictionary`'s embedded ~280k-
//! word corpus (data licensed CC-BY-SA 4.0 — see this repository's `NOTICE`
//! file for attribution; the crate's own code is MIT). We parse the crate's
//! raw embedded JSON (`CORPUS_JSON`) ourselves rather than going through its
//! `Corpus`/`Trie` API: that API builds a phoneme trie for pronunciation
//! lookups, which costs a real parse+build pass we don't need, and has no
//! "list every headword" accessor at all (checked directly against
//! `phonetics-rs`'s `Corpus` struct, which only exposes `pronunciations`,
//! `preferred_ipa`, `transcribe`, `word_count` — no iterator over words).
//! The JSON itself is a flat object keyed by headword, so `serde_json`
//! alone gets us the exact word list, faster and without the extra
//! dependency surface.
//!
//! The dependency is pinned to an **exact** version (`=0.1.0`, see the
//! workspace `Cargo.toml`) rather than a semver range: any future version
//! could reorder or change the corpus contents, which would silently change
//! every peer's name. Bumping this dependency must be a conscious decision.

use std::collections::HashMap;
use std::sync::OnceLock;

use oag_crypto::PeerId;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::de::IgnoredAny;

/// How many words make up one peer's name.
const WORD_COUNT: usize = 128;

fn wordlist() -> &'static [String] {
    static WORDS: OnceLock<Vec<String>> = OnceLock::new();
    WORDS.get_or_init(|| {
        // `IgnoredAny` values: we only want the object's keys, so there's no
        // need to materialize the rarity/ipa/alt_display payload per entry.
        let raw: HashMap<String, IgnoredAny> =
            serde_json::from_str(open_english_pronouncing_dictionary::CORPUS_JSON).expect(
                "bundled corpus JSON is well-formed — it ships compiled into the pinned \
                 open-english-pronouncing-dictionary crate, not read from an external source",
            );
        // Keep only clean single-token alphabetic headwords: the corpus is a
        // pronunciation dictionary, not a curated name list, so a few
        // entries carry punctuation/digits that would look wrong in a name.
        let mut words: Vec<String> = raw
            .into_keys()
            .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic()))
            .collect();
        // Fixed, version-independent-of-hashmap-iteration-order ordering —
        // required for `peer_name` to be reproducible at all.
        words.sort();
        words.dedup();
        words
    })
}

/// A peer's deterministic, human-legible display name: [`WORD_COUNT`] words
/// drawn (with replacement) from the wordlist, seeded from `peer_id`'s own
/// bytes. The same `peer_id` always yields the same words; two different
/// `peer_id`s yield different words because their seeds differ.
///
/// The wordlist is loaded lazily on first call (see [`wordlist`]) — a
/// process that never calls this function never pays the corpus parse cost.
pub fn peer_name(peer_id: &PeerId) -> Vec<String> {
    let words = wordlist();
    let mut rng = ChaCha8Rng::from_seed(*peer_id.as_bytes());
    (0..WORD_COUNT).map(|_| words[rng.gen_range(0..words.len())].clone()).collect()
}

/// [`peer_name`], joined into one hyphen-separated display string.
pub fn peer_name_string(peer_id: &PeerId) -> String {
    peer_name(peer_id).join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Arbitrary-but-fixed byte patterns rather than a real generated key —
    // this crate never touches signing keys, only the 32 raw `PeerId` bytes,
    // so there's no need to pull ed25519-dalek/a matching rand version into
    // a leaf crate whose whole job is staying small.
    fn peer_id_from_tag(tag: u8) -> PeerId {
        PeerId::from_bytes([tag; 32])
    }

    #[test]
    fn same_peer_id_yields_same_name_every_time() {
        let peer_id = peer_id_from_tag(1);
        let first = peer_name(&peer_id);
        let second = peer_name(&peer_id);
        assert_eq!(first, second);
        assert_eq!(first.len(), WORD_COUNT);
    }

    #[test]
    fn different_peer_ids_yield_different_names() {
        let a = peer_name(&peer_id_from_tag(1));
        let b = peer_name(&peer_id_from_tag(2));
        assert_ne!(a, b, "two different peer ids collided on a 128-word name");
    }

    #[test]
    fn every_word_is_clean_lowercase_or_alphabetic_ascii() {
        let peer_id = peer_id_from_tag(3);
        for word in peer_name(&peer_id) {
            assert!(!word.is_empty());
            assert!(word.chars().all(|c| c.is_ascii_alphabetic()), "unexpected word: {word:?}");
        }
    }

    #[test]
    fn wordlist_is_sorted_and_deduplicated() {
        let words = wordlist();
        assert!(words.len() > 50_000, "expected a large corpus, got {} words", words.len());
        for pair in words.windows(2) {
            assert!(pair[0] < pair[1], "wordlist is not strictly sorted/deduplicated at {pair:?}");
        }
    }

    #[test]
    fn peer_name_string_joins_with_hyphens() {
        let peer_id = peer_id_from_tag(4);
        let words = peer_name(&peer_id);
        assert_eq!(peer_name_string(&peer_id), words.join("-"));
    }
}
