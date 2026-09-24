use std::sync::Arc;
use std::time::{Duration, Instant};

use oag_core::{ActorType, AssertionId, AssertionStatus, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::{AssertInput, AuthContext, GraphService};
use oag_storage::pool::open_pool;
use oag_sync::{router, FederationPolicy, SyncService};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};

const NUM_PEERS: usize = 50;
const CHAOS_ROUNDS: usize = 40;
const KILL_FLOOR: usize = 25;
const CHAOS_SYNC_BATCH: usize = 8;
/// Full mesh sweeps after the chaos phase: every alive peer explicitly
/// pulls from every other alive peer, guaranteeing convergence
/// structurally (each peer gets every other peer's events straight from
/// the source, not relying on transitive random-gossip propagation) rather
/// than probabilistically. Each sweep both catches up on content and
/// retries anything an earlier sweep hit transient SQLite contention on
/// (see `full_mesh_sync_sweep`'s doc comment) — three sweeps gives that
/// retry margin without meaningfully extending runtime, since a sweep
/// where a peer is already caught up on an origin skips re-fetching it.
const HEALING_SWEEPS: usize = 3;

struct ChaosPeer {
    graph: Arc<GraphService>,
    sync: SyncService,
    auth: AuthContext,
    addr: String,
}

async fn spawn_peer(name: &str) -> ChaosPeer {
    let dir = std::env::temp_dir().join(format!(
        "oag-chaos-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pool = open_pool(&dir.join("oag.sqlite")).await.unwrap();

    let identity = PeerIdentity::generate();
    let peer_id = identity.peer_id();
    let public_key = identity.verifying_key().to_bytes();

    let graph = Arc::new(GraphService::new(pool.clone(), identity));
    let actor_id = graph
        .declare_actor(ActorType::Service, Some(format!("{name}-actor")), None)
        .await
        .unwrap();
    let auth = AuthContext {
        actor_id,
        permissions: vec![Permission::GraphAssert],
    };

    let sync = SyncService::new(pool, peer_id, public_key, FederationPolicy::Open);

    let app = router(sync.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    ChaosPeer {
        graph,
        sync,
        auth,
        addr: format!("http://{addr}"),
    }
}

/// One round of gossip: `batch_size` random (source, target) pairs, both
/// alive and in the same partition group, synced concurrently. This is the
/// harness's stand-in for each peer's own wall-clock gossip loop — driven
/// explicitly so the chaos sequence stays deterministic under a seeded RNG.
async fn do_sync_round(
    peers: &[ChaosPeer],
    alive: &[bool],
    group: &[usize],
    batch_size: usize,
    rng: &mut StdRng,
) {
    let alive_indices: Vec<usize> = (0..peers.len()).filter(|&i| alive[i]).collect();
    if alive_indices.len() < 2 {
        return;
    }
    let mut set = tokio::task::JoinSet::new();
    for _ in 0..batch_size {
        let src = *alive_indices.choose(rng).unwrap();
        let candidates: Vec<usize> = alive_indices
            .iter()
            .copied()
            .filter(|&i| i != src && group[i] == group[src])
            .collect();
        let Some(&dst) = candidates.choose(rng) else { continue };
        let sync = peers[src].sync.clone();
        let addr = peers[dst].addr.clone();
        set.spawn(async move {
            let _ = tokio::time::timeout(Duration::from_secs(5), sync.sync_with_peer(&addr)).await;
        });
    }
    while set.join_next().await.is_some() {}
}

/// Every alive peer pulls from every other alive peer, once. Unlike
/// [`do_sync_round`]'s random sampling, this gives a structural convergence
/// guarantee in a single sweep: each peer receives every other peer's
/// events straight from the source, with no dependence on transitive
/// propagation or how many random rounds happened to be enough.
///
/// Concurrency is bounded by a semaphore rather than firing all N*(N-1)
/// pairs at once: with 50 peers that's 2450 simultaneous multi-round-trip
/// HTTP chains against 50 in-process Axum servers sharing one Tokio
/// runtime, which — empirically — doesn't deadlock but does starve every
/// single task past any reasonable per-call timeout. Bounding concurrency
/// is what real deployments get for free from independently-timed gossip
/// loops; a synchronous test harness has to do it explicitly.
///
/// Even bounded, 32-way concurrent writers against one peer's single-writer
/// SQLite file routinely exceeds its 5s `busy_timeout` under this load —
/// expected, harmless (the failed range is simply retried on the next
/// sweep or the next chaos round's sync), and returned via `SyncSummary`
/// rather than panicking, so it's counted here rather than treated as a
/// hard failure.
#[derive(Default)]
struct SweepStats {
    transient_errors: usize,
    timeouts: usize,
}

enum SyncOutcome {
    Completed(usize),
    SyncErr,
    TimedOut,
}

async fn full_mesh_sync_sweep(peers: &[ChaosPeer], alive: &[bool]) -> SweepStats {
    const MAX_CONCURRENT_SYNCS: usize = 32;
    let alive_indices: Vec<usize> = (0..peers.len()).filter(|&i| alive[i]).collect();
    let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_SYNCS));
    let mut set = tokio::task::JoinSet::new();
    for &src in &alive_indices {
        for &dst in &alive_indices {
            if src == dst {
                continue;
            }
            let sync = peers[src].sync.clone();
            let addr = peers[dst].addr.clone();
            let semaphore = semaphore.clone();
            set.spawn(async move {
                let _permit = semaphore.acquire_owned().await.unwrap();
                match tokio::time::timeout(Duration::from_secs(10), sync.sync_with_peer(&addr)).await {
                    Ok(Ok(summary)) => SyncOutcome::Completed(summary.errors.len()),
                    Ok(Err(_)) => SyncOutcome::SyncErr,
                    Err(_) => SyncOutcome::TimedOut,
                }
            });
        }
    }
    let mut stats = SweepStats::default();
    while let Some(result) = set.join_next().await {
        match result.expect("sync task panicked") {
            SyncOutcome::Completed(n) => stats.transient_errors += n,
            SyncOutcome::SyncErr => stats.transient_errors += 1,
            SyncOutcome::TimedOut => stats.timeouts += 1,
        }
    }
    stats
}

/// Spec section 105 (Chaos Testing) / invariant 9 (section 106): spin up
/// many real peers, randomly create events / kill / restart / partition /
/// heal, and confirm every peer converges on an identical accepted event
/// set once communication stabilizes.
///
/// Peer "death" and "partition" are simulated at the orchestration level,
/// not the network level — every peer's Axum server runs for the whole
/// test; a "dead" or "partitioned-away" peer is simply never chosen as a
/// sync source or target while it's down. This is the same simplification
/// already used by this crate's `relay_without_origin_dependency` test in
/// `src/tests.rs`, whose peer-death comment is literally "A is never
/// contacted again from this point on" — it proves the replication
/// protocol's convergence properties under topology churn, not TCP-level
/// fault tolerance.
#[ignore = "slow: spins up 50 real peers; run explicitly with \
            `cargo test -p oag-sync --test distributed_convergence -- --ignored --nocapture`"]
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fifty_peers_converge_after_chaos() {
    let start = Instant::now();
    let mut rng = StdRng::seed_from_u64(42);

    let mut peers = Vec::with_capacity(NUM_PEERS);
    for i in 0..NUM_PEERS {
        peers.push(spawn_peer(&format!("p{i}")).await);
    }

    let mut alive = vec![true; NUM_PEERS];
    let mut group = vec![0usize; NUM_PEERS];
    let mut ledger: Vec<(usize, AssertionId)> = Vec::new();

    let mut created = 0u32;
    let mut kills = 0u32;
    let mut restarts = 0u32;
    let mut partitions = 0u32;
    let mut heals = 0u32;

    for round in 0..CHAOS_ROUNDS {
        let alive_indices: Vec<usize> = (0..NUM_PEERS).filter(|&i| alive[i]).collect();
        let roll = rng.gen_range(0..100);
        match roll {
            0..=39 => {
                // Create a random assertion on a random alive peer.
                if let Some(&creator) = alive_indices.choose(&mut rng) {
                    let subject = format!("https://example.com/chaos/r{round}-p{creator}-{created}");
                    let object = if rng.gen_bool(0.3) {
                        "concept:shared-topic".to_string()
                    } else {
                        format!("concept:topic-{}", rng.gen_range(0..10))
                    };
                    let assertion_id = peers[creator]
                        .graph
                        .assert(
                            &peers[creator].auth,
                            AssertInput {
                                subject,
                                subject_type: Some("repository".into()),
                                predicate: "related_to".into(),
                                object,
                                object_type: None,
                                evidence: vec![],
                                actor_confidence: Some(0.8),
                                observed_at: None,
                                extraction_method: None,
                            },
                        )
                        .await
                        .unwrap();
                    ledger.push((creator, assertion_id));
                    created += 1;
                }
            }
            40..=54 => {
                // Kill a random alive peer, keeping at least KILL_FLOOR alive.
                if alive_indices.len() > KILL_FLOOR {
                    if let Some(&victim) = alive_indices.choose(&mut rng) {
                        alive[victim] = false;
                        kills += 1;
                    }
                }
            }
            55..=69 => {
                // Restart a random dead peer.
                let dead_indices: Vec<usize> = (0..NUM_PEERS).filter(|&i| !alive[i]).collect();
                if let Some(&revived) = dead_indices.choose(&mut rng) {
                    alive[revived] = true;
                    restarts += 1;
                }
            }
            70..=79 => {
                // Split alive peers into two random partition groups.
                let mut shuffled = alive_indices.clone();
                shuffled.shuffle(&mut rng);
                let (left, right) = shuffled.split_at(shuffled.len() / 2);
                for &i in left {
                    group[i] = 0;
                }
                for &i in right {
                    group[i] = 1;
                }
                partitions += 1;
            }
            80..=89 => {
                // Heal: everyone back into one group.
                for g in group.iter_mut() {
                    *g = 0;
                }
                heals += 1;
            }
            _ => {}
        }

        do_sync_round(&peers, &alive, &group, CHAOS_SYNC_BATCH, &mut rng).await;
    }

    // End of chaos: force everyone healthy and unpartitioned — spec's bar
    // is "all *healthy* peers converge," so make "healthy" unambiguous.
    let alive = vec![true; NUM_PEERS];

    let mut healing_transient_errors = 0usize;
    let mut healing_timeouts = 0usize;
    for _ in 0..HEALING_SWEEPS {
        let stats = full_mesh_sync_sweep(&peers, &alive).await;
        healing_transient_errors += stats.transient_errors;
        healing_timeouts += stats.timeouts;
    }

    // --- Convergence assertions ---

    let reference_heads = peers[0].sync.local_heads().await.unwrap();
    let mut divergent = Vec::new();
    for (i, peer) in peers.iter().enumerate() {
        let heads = peer.sync.local_heads().await.unwrap();
        if heads != reference_heads {
            divergent.push((i, heads));
        }
    }
    assert!(
        divergent.is_empty(),
        "{} of {NUM_PEERS} peers diverged from the reference heads {reference_heads:?}: {divergent:?}",
        divergent.len()
    );

    // Spot-check every created assertion is present and Active on a sample
    // of independently-chosen peers.
    let mut sample_peer_indices: Vec<usize> = (0..NUM_PEERS).collect();
    sample_peer_indices.shuffle(&mut rng);
    sample_peer_indices.truncate(3);

    for &peer_idx in &sample_peer_indices {
        for &(_, assertion_id) in &ledger {
            let assertion = peers[peer_idx]
                .graph
                .get_assertion(assertion_id)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("peer {peer_idx} missing assertion {assertion_id}"));
            assert_eq!(assertion.status, AssertionStatus::Active);
        }
    }

    // This harness never constructs a genuine conflicting-signature
    // scenario, so any recorded fork indicates a real correctness bug.
    let mut total_forks = 0usize;
    for peer in &peers {
        let mut conn = peer.sync.pool().acquire().await.unwrap();
        let known = oag_storage::repo::peers::list_peers(&mut conn).await.unwrap();
        for info in known {
            total_forks += oag_storage::repo::peers::list_forks(&mut conn, &info.peer_id)
                .await
                .unwrap()
                .len();
        }
    }
    assert_eq!(total_forks, 0, "chaos harness never constructs real forks; found {total_forks}");

    let total_events: u64 = reference_heads.values().sum();
    println!(
        "chaos convergence: {NUM_PEERS} peers, {CHAOS_ROUNDS} chaos rounds + {HEALING_SWEEPS} full-mesh healing sweeps, \
         {created} assertions created, {kills} kills, {restarts} restarts, {partitions} partitions, {heals} heals, \
         {total_events} total events converged, ledger checked on {} peers, \
         {healing_transient_errors} transient SQLite-contention errors and {healing_timeouts} timeouts during \
         healing (all recovered by a later sweep — none blocked final convergence), elapsed {:?}",
        sample_peer_indices.len(),
        start.elapsed()
    );
}
