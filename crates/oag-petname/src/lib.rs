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

/// How many words make up one peer's DNS-addressing label (spec/30-long-
/// word-addressing.md) — far fewer than [`WORD_COUNT`], since this must fit
/// a single DNS label (RFC 1035: 63 bytes max), not just look nice.
const ADDRESS_WORD_COUNT: usize = 4;

/// Addressing labels only draw from words this short, so the worst case
/// (`ADDRESS_WORD_COUNT` words, each at this max length, joined by
/// `ADDRESS_WORD_COUNT - 1` hyphens) stays safely under DNS's 63-byte
/// label limit by construction, not just "usually": `4*12 + 3 = 51`.
const ADDRESS_WORD_MAX_LEN: usize = 12;

fn address_wordlist() -> &'static [String] {
    static WORDS: OnceLock<Vec<String>> = OnceLock::new();
    WORDS.get_or_init(|| {
        wordlist().iter().filter(|w| w.len() <= ADDRESS_WORD_MAX_LEN).cloned().collect()
    })
}

/// A peer's deterministic DNS-addressing label (spec/30-long-word-
/// addressing.md): [`ADDRESS_WORD_COUNT`] words, hyphen-joined, drawn from
/// [`address_wordlist`] and seeded from `peer_id` — same determinism
/// guarantee as [`peer_name`] (same `peer_id` always yields the same
/// label), but short enough to fit one DNS label, unlike `peer_name`'s own
/// 128 words. This is an addressing *convenience*, not a trust credential:
/// a collision between two different peers' labels is an availability
/// nuisance (they can't both publish under the same name), never a security
/// concern, since anything resolved through it still goes through this
/// peer's own, unrelated signature verification before being trusted.
pub fn address_label(peer_id: &PeerId) -> String {
    let words = address_wordlist();
    let mut rng = ChaCha8Rng::from_seed(*peer_id.as_bytes());
    (0..ADDRESS_WORD_COUNT).map(|_| words[rng.gen_range(0..words.len())].clone()).collect::<Vec<_>>().join("-")
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

    #[test]
    fn address_wordlist_still_has_a_large_candidate_count() {
        // Documents the real entropy margin (spec/30) rather than asserting
        // a hand-computed bit count: at ~15 bits/word even restricted to
        // this shorter candidate set, 4 words comfortably clears the ~24
        // bits needed to distinguish "tens of millions" of peers.
        let words = address_wordlist();
        assert!(words.len() > 10_000, "expected a large candidate set, got {} words", words.len());
    }

    #[test]
    fn address_label_is_always_a_valid_dns_label() {
        for tag in 0..64u8 {
            let peer_id = peer_id_from_tag(tag);
            let label = address_label(&peer_id);
            assert!(!label.is_empty());
            assert!(label.len() <= 63, "label exceeds the DNS 63-byte limit: {label:?} ({} bytes)", label.len());
            assert_eq!(label.split('-').count(), ADDRESS_WORD_COUNT, "expected exactly {ADDRESS_WORD_COUNT} words: {label:?}");
            assert!(
                label.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "label has non-lowercase-alphabetic-or-hyphen characters: {label:?}"
            );
            assert!(!label.starts_with('-') && !label.ends_with('-'), "label starts/ends with a hyphen: {label:?}");
        }
    }

    #[test]
    fn address_label_is_deterministic_and_distinct_per_peer() {
        let peer_id = peer_id_from_tag(5);
        assert_eq!(address_label(&peer_id), address_label(&peer_id));

        let a = address_label(&peer_id_from_tag(6));
        let b = address_label(&peer_id_from_tag(7));
        assert_ne!(a, b, "two different peer ids collided on a 4-word address label");
    }
}
