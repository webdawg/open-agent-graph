//! Long-word subdomain addressing, Phase 1 (spec/30-long-word-addressing.md):
//! deriving the DNS name a peer would publish itself under, and resolving
//! one via a real TXT lookup. Deliberately not wired into `oag-sync`'s
//! automatic discovery/gossip path -- an explicit, operator-triggered CLI
//! tool for this phase, same posture as `oag ipfs cat`/`oag migrate`.

use oag_crypto::PeerId;

/// The FQDN a peer would publish itself under: its deterministic 4-word
/// address label (`oag_petname::address_label`), under `domain`. A trailing
/// dot on `domain` (the absolute-name DNS convention) is tolerated and
/// normalized away rather than producing a doubled `..`.
pub fn fqdn_for_peer(peer_id: &PeerId, domain: &str) -> String {
    format!("{}.{}", oag_petname::address_label(peer_id), domain.trim_end_matches('.'))
}

/// Resolves `peer_id`'s FQDN under `domain` via a real TXT lookup, returning
/// every TXT record's string value (an operator may publish more than one,
/// e.g. mid-migration to a new address). A clean, typed-message error --
/// not a raw resolver error dump -- when nothing is published there.
pub async fn resolve_peer_address(peer_id: &PeerId, domain: &str) -> anyhow::Result<Vec<String>> {
    let fqdn = fqdn_for_peer(peer_id, domain);
    let resolver = hickory_resolver::Resolver::builder_tokio()
        .map_err(|e| anyhow::anyhow!("could not build a DNS resolver: {e}"))?
        .build()
        .map_err(|e| anyhow::anyhow!("could not build a DNS resolver: {e}"))?;
    let lookup = resolver.txt_lookup(fqdn.clone()).await.map_err(|e| {
        anyhow::anyhow!(
            "no TXT record found at {fqdn} ({e}) -- has this peer's operator published one there?"
        )
    })?;
    let values: Vec<String> = lookup
        .answers()
        .iter()
        .filter_map(|record| match &record.data {
            hickory_resolver::proto::rr::RData::TXT(txt) => Some(txt.to_string()),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        anyhow::bail!("{fqdn} resolved but carried no TXT record data");
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer_id_from_tag(tag: u8) -> PeerId {
        PeerId::from_bytes([tag; 32])
    }

    #[test]
    fn fqdn_joins_label_and_domain() {
        let peer_id = peer_id_from_tag(1);
        let label = oag_petname::address_label(&peer_id);
        assert_eq!(fqdn_for_peer(&peer_id, "example.com"), format!("{label}.example.com"));
    }

    #[test]
    fn fqdn_normalizes_a_trailing_dot_on_the_domain() {
        let peer_id = peer_id_from_tag(2);
        let label = oag_petname::address_label(&peer_id);
        assert_eq!(fqdn_for_peer(&peer_id, "example.com."), format!("{label}.example.com"));
    }

    /// Real network access against real, already-existing public DNS (not
    /// this project's own infrastructure -- no real domain is configured
    /// for OAG itself) to prove the resolver plumbing actually works, not
    /// just that it compiles. `#[ignore]`d by default since it needs real
    /// network access; run explicitly with `--ignored`.
    #[tokio::test]
    #[ignore = "needs real network access to public DNS"]
    async fn resolves_a_real_txt_record_on_a_well_known_domain() {
        let resolver = hickory_resolver::Resolver::builder_tokio().unwrap().build().unwrap();
        let lookup = resolver.txt_lookup("google.com").await.unwrap();
        assert!(!lookup.answers().is_empty(), "expected at least one TXT record on google.com");
    }

    #[tokio::test]
    #[ignore = "needs real network access to public DNS"]
    async fn missing_txt_record_is_a_clean_error_not_a_panic() {
        let peer_id = peer_id_from_tag(3);
        let result = resolve_peer_address(
            &peer_id,
            "this-subdomain-definitely-does-not-exist-for-oag.example.com",
        )
        .await;
        assert!(result.is_err(), "expected a clean error for a nonexistent name, got {result:?}");
    }
}
