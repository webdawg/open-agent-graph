//! The gravity layer (spec/25): a small, truly-random, per-node "how much
//! gravity is here" value that very slightly throttles that node's own
//! processing speed. Resolves the open question left in spec/22's
//! original capture in favor of the "gravitational time dilation"
//! reading: gravity slows a node's own clock, not its tensor pad's
//! magnitude.
//!
//! Deliberately a *strong* RNG, not the deterministic ChaCha8-with-a-fixed-
//! seed pattern `oag-tensor`/`oag-brain`/`oag-petname` use elsewhere in
//! this project for *reproducibility* — gravity wants the opposite
//! property, real per-node unpredictability. `OsRng` draws from the OS's
//! own CSPRNG (on Linux, ultimately seeded from hardware entropy) — a
//! software stand-in for a real hardware measurement, which is future
//! work (see spec/25).

use oag_crypto::PeerId;
use oag_storage::repo::node_gravity as node_gravity_repo;
use oag_storage::SqlitePool;
use rand::rngs::OsRng;
use rand::Rng;

/// The full range a rolled gravity level can fall in.
pub const GRAVITY_LEVEL_RANGE: std::ops::Range<f32> = 0.0..1.0;

/// "Very very slightly": the maximum possible delay a gravity level of
/// `1.0` could ever cause, applied once per peer address a node processes
/// in its own gossip loop ("one at a time" — spec/22's original capture).
/// Half a millisecond at most; most nodes will see a small fraction of
/// this.
pub const MAX_DELAY_MICROS: u64 = 500;

pub fn roll_gravity_level() -> f32 {
    OsRng.gen_range(GRAVITY_LEVEL_RANGE)
}

/// Looks up this peer's persisted gravity level, rolling and persisting a
/// fresh one on first use. Stable across restarts for a permanent
/// identity; a fresh roll every time for an ephemeral one, same as its
/// `peer_id` (spec/24).
pub async fn get_or_generate(
    pool: &SqlitePool,
    peer_id: &PeerId,
    now: i64,
) -> Result<f32, oag_storage::StorageError> {
    let mut conn = pool.acquire().await?;
    if let Some(level) = node_gravity_repo::get(&mut conn, peer_id.as_bytes()).await? {
        return Ok(level);
    }
    let rolled = roll_gravity_level();
    node_gravity_repo::insert_if_missing(&mut conn, peer_id.as_bytes(), rolled, now).await?;
    // Another task could have raced this insert -- re-read rather than
    // trust the value just rolled, so every caller converges on whichever
    // value actually got persisted first.
    Ok(node_gravity_repo::get(&mut conn, peer_id.as_bytes()).await?.unwrap_or(rolled))
}

/// Maps a gravity level to the tiny delay it causes -- a pure function so
/// the mapping itself is exactly hand-checkable, independent of actually
/// sleeping.
pub fn delay_for(gravity_level: f32) -> std::time::Duration {
    let micros = (gravity_level.clamp(0.0, 1.0) * MAX_DELAY_MICROS as f32) as u64;
    std::time::Duration::from_micros(micros)
}

/// Applies this node's own very slight processing slowdown.
pub async fn apply_slowdown(gravity_level: f32) {
    let delay = delay_for(gravity_level);
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolled_levels_land_in_range_and_are_not_all_identical() {
        let levels: Vec<f32> = (0..20).map(|_| roll_gravity_level()).collect();
        for &level in &levels {
            assert!(GRAVITY_LEVEL_RANGE.contains(&level), "{level} out of range");
        }
        // "different everywhere by some small random amount" -- a real
        // strong RNG must not produce the same value twice in 20 rolls.
        let distinct = levels.iter().map(|l| l.to_bits()).collect::<std::collections::HashSet<_>>().len();
        assert!(distinct > 1, "expected real randomness across rolls, got {levels:?}");
    }

    #[test]
    fn delay_scales_linearly_and_stays_within_the_very_slight_bound() {
        assert_eq!(delay_for(0.0), std::time::Duration::ZERO);
        assert_eq!(delay_for(1.0), std::time::Duration::from_micros(MAX_DELAY_MICROS));
        assert_eq!(delay_for(0.5), std::time::Duration::from_micros(MAX_DELAY_MICROS / 2));
    }

    #[test]
    fn delay_is_clamped_for_out_of_range_input() {
        assert_eq!(delay_for(-1.0), std::time::Duration::ZERO);
        assert_eq!(delay_for(5.0), std::time::Duration::from_micros(MAX_DELAY_MICROS));
    }

    #[tokio::test]
    async fn get_or_generate_persists_and_is_stable_across_calls() {
        let dir = std::env::temp_dir().join(format!(
            "oag-sync-gravity-test-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = oag_storage::pool::open_pool(&dir.join("oag.sqlite")).await.unwrap();
        let peer_id = oag_crypto::PeerIdentity::generate().peer_id();

        let first = get_or_generate(&pool, &peer_id, 1_000).await.unwrap();
        let second = get_or_generate(&pool, &peer_id, 2_000).await.unwrap();
        assert_eq!(first, second, "gravity level must not change across repeated lookups");
    }
}
