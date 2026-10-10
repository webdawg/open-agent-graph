//! Trust scoring for ephemeral peer identity (Phase 1). Deliberately one
//! pure function with no notion of *why* a peer looks bad — "trust is
//! reliance, not paranoia" (`USER_INPUT_RECORD.md` Entry 4): a peer made
//! unreliable by buggy hardware is scored exactly like a malicious one,
//! from the same observed signals, with no intent-guessing branch.
//!
//! Trust is **per-observer**, computed locally from this peer's own
//! `peer_presence` row for the peer being scored — never a globally agreed
//! number. A score of exactly `0.0` is the only hard floor (a forked peer,
//! or one this peer has never heard a heartbeat from at all); everything
//! above that is a continuum, not a second cliff — see `federation.rs`'s
//! "degrade, don't drop" enforcement built on top of this.

use oag_crypto::PeerId;
use oag_storage::models::PeerPresenceRow;
use oag_storage::repo::{peer_presence as peer_presence_repo, peers as peers_repo};
use oag_storage::SqlitePool;

/// How many missed heartbeat intervals before a peer counts as "gone quiet"
/// rather than merely between beats — generous on purpose (the "frozen in
/// ice" latency-tolerance framing already in `future_ephemeral_peer_trust.md`
/// means a slow network, not silence, should be the default assumption).
const STALE_MULTIPLIER: i64 = 3;

/// The trust-rebuild curve's starting point for a peer whose first
/// heartbeat at a given address arrived within that address's
/// "announced restart" grace window (see `peer_presence.rs`'s
/// `address_going_offline` table) — gentler than a totally cold start, but
/// still well short of full trust, since the correlation is a heuristic,
/// not proof.
const ANNOUNCED_RESTART_STARTING_TRUST: f32 = 0.3;

/// The threshold `oag serve` passes to [`crate::SyncService::with_minimum_trust`]
/// when ephemeral mode is on. Any positive trust (i.e. "has sent at least
/// one heartbeat and isn't forked") clears it — this, not a user-tunable
/// config value, is what makes federation enforcement a two-tier floor
/// rather than a second cliff: only an exact `0.0` score (forked, or never
/// seen at all) is ever hard-rejected.
pub const EPHEMERAL_MINIMUM_TRUST: f32 = f32::MIN_POSITIVE;

#[derive(Debug, Clone, Copy)]
pub struct TrustConfig {
    pub heartbeat_interval_seconds: i64,
    pub trust_rebuild_seconds: i64,
    pub announced_restart_grace_seconds: i64,
}

impl Default for TrustConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval_seconds: 30,
            trust_rebuild_seconds: 3600,
            announced_restart_grace_seconds: 300,
        }
    }
}

fn rebuild_fraction(session_started_at: i64, as_of: i64, cfg: &TrustConfig, announced_restart: bool) -> f32 {
    let continuous_seconds = (as_of - session_started_at).max(0) as f32;
    let starting_point = if announced_restart { ANNOUNCED_RESTART_STARTING_TRUST } else { 0.0 };
    let rebuilt_fraction = (continuous_seconds / cfg.trust_rebuild_seconds as f32).clamp(0.0, 1.0);
    starting_point + rebuilt_fraction * (1.0 - starting_point)
}

/// Compute this observer's own trust score for one peer, in `[0.0, 1.0]`.
///
/// - `presence`: this peer's `peer_presence` row for the scored peer, if
///   any heartbeat has ever been recorded for its current identity lifetime.
/// - `forked`: the existing, unrelated `peers.forked` signal — a hard floor,
///   reused exactly as-is rather than duplicated.
/// - `announced_restart`: true if the scored peer's *first* heartbeat
///   arrived from an address that recently announced `GoingOffline` (see
///   `peer_presence::get_address_going_offline`).
pub fn trust_score(
    presence: Option<&PeerPresenceRow>,
    forked: bool,
    announced_restart: bool,
    now: i64,
    cfg: &TrustConfig,
) -> f32 {
    if forked {
        return 0.0;
    }
    let Some(presence) = presence else {
        return 0.0;
    };

    let silence = now - presence.last_heartbeat_at;
    let stale_after = cfg.heartbeat_interval_seconds * STALE_MULTIPLIER;

    if silence <= stale_after {
        return rebuild_fraction(presence.session_started_at, now, cfg, announced_restart).clamp(0.0, 1.0);
    }

    // Gone quiet: decay the trust it had *at the moment it went silent*
    // patiently toward zero, rather than cliff-dropping to zero the instant
    // it crosses the stale threshold.
    let trust_at_silence =
        rebuild_fraction(presence.session_started_at, presence.last_heartbeat_at, cfg, announced_restart);
    let decay_elapsed = (silence - stale_after) as f32;
    let decay_fraction = (decay_elapsed / cfg.trust_rebuild_seconds as f32).clamp(0.0, 1.0);
    (trust_at_silence * (1.0 - decay_fraction)).clamp(0.0, 1.0)
}

/// Orchestrates `trust_score` against real storage: looks up the existing
/// `forked` flag, this peer_id's `peer_presence` row, and whether its
/// current session correlates with a recent `GoingOffline` announcement at
/// any address this peer knows for it (the "announced restart" grace —
/// see `peer_presence.rs`'s `address_going_offline` table and
/// `USER_INPUT_RECORD.md` Entry 4's "voting right" reasoning). Trust is
/// always computed *by* the caller, *for* one peer, from only what the
/// caller itself has observed — never a value fetched from anywhere else.
pub async fn compute_trust(
    pool: &SqlitePool,
    peer_id: &PeerId,
    now: i64,
    cfg: &TrustConfig,
) -> Result<f32, oag_storage::StorageError> {
    let mut conn = pool.acquire().await?;
    let forked = peers_repo::get_peer(&mut conn, peer_id.as_bytes()).await?.map(|p| p.forked).unwrap_or(false);
    let presence = peer_presence_repo::get(&mut conn, peer_id.as_bytes()).await?;

    let announced_restart = match &presence {
        Some(p) => {
            let mut found = false;
            for addr in peers_repo::list_addresses(&mut conn, peer_id.as_bytes()).await? {
                if let Some(row) = peer_presence_repo::get_address_going_offline(&mut conn, &addr).await? {
                    let within_grace = now - row.announced_at <= cfg.announced_restart_grace_seconds;
                    let predates_this_session = row.announced_at <= p.session_started_at;
                    if within_grace && predates_this_session {
                        found = true;
                        break;
                    }
                }
            }
            found
        }
        None => false,
    };

    Ok(trust_score(presence.as_ref(), forked, announced_restart, now, cfg))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(session_started_at: i64, last_heartbeat_at: i64) -> PeerPresenceRow {
        PeerPresenceRow {
            peer_id: vec![0u8; 32],
            session_started_at,
            last_heartbeat_at,
            status: "online".to_string(),
        }
    }

    #[test]
    fn forked_peer_is_always_zero_regardless_of_presence() {
        let cfg = TrustConfig::default();
        let p = presence(0, 10_000);
        assert_eq!(trust_score(Some(&p), true, false, 10_000, &cfg), 0.0);
    }

    #[test]
    fn never_seen_peer_is_zero() {
        let cfg = TrustConfig::default();
        assert_eq!(trust_score(None, false, false, 10_000, &cfg), 0.0);
    }

    #[test]
    fn cold_start_begins_at_zero_and_rises_toward_one_over_the_rebuild_window() {
        let cfg = TrustConfig::default();
        let p = presence(0, 0);
        assert_eq!(trust_score(Some(&p), false, false, 0, &cfg), 0.0);

        let halfway = trust_score(Some(&presence(0, cfg.trust_rebuild_seconds / 2)), false, false, cfg.trust_rebuild_seconds / 2, &cfg);
        assert!((halfway - 0.5).abs() < 0.01, "expected ~0.5 at the halfway point, got {halfway}");

        let full = trust_score(Some(&presence(0, cfg.trust_rebuild_seconds)), false, false, cfg.trust_rebuild_seconds, &cfg);
        assert!((full - 1.0).abs() < 0.001, "expected 1.0 once the full rebuild window has elapsed, got {full}");
    }

    #[test]
    fn announced_restart_starts_above_zero_but_below_full_trust() {
        let cfg = TrustConfig::default();
        let p = presence(0, 0);
        let score = trust_score(Some(&p), false, true, 0, &cfg);
        assert_eq!(score, ANNOUNCED_RESTART_STARTING_TRUST);
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn trust_decays_patiently_after_going_quiet_rather_than_cliff_dropping() {
        let cfg = TrustConfig::default();
        // Built up to full trust, then went silent exactly at t=rebuild_seconds.
        let p = presence(0, cfg.trust_rebuild_seconds);
        let stale_after = cfg.heartbeat_interval_seconds * STALE_MULTIPLIER;

        // Right at the edge of staleness: still full trust, no cliff yet.
        let at_edge = trust_score(Some(&p), false, false, cfg.trust_rebuild_seconds + stale_after, &cfg);
        assert!((at_edge - 1.0).abs() < 0.001, "expected still-full trust right at the stale edge, got {at_edge}");

        // Well past staleness: decayed, but not instantly to zero.
        let decaying = trust_score(
            Some(&p),
            false,
            false,
            cfg.trust_rebuild_seconds + stale_after + cfg.trust_rebuild_seconds / 2,
            &cfg,
        );
        assert!(decaying > 0.0 && decaying < 1.0, "expected a partial decay, got {decaying}");

        // Long after: fully decayed back to zero.
        let fully_decayed = trust_score(
            Some(&p),
            false,
            false,
            cfg.trust_rebuild_seconds + stale_after + cfg.trust_rebuild_seconds + 1,
            &cfg,
        );
        assert_eq!(fully_decayed, 0.0);
    }
}
