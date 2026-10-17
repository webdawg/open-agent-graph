use std::path::Path;

use oag_core::{ActorId, ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::{AuthContext, GraphService};
use oag_sync::{FederationPolicy, SyncService};

async fn open_graph(data_dir: &Path) -> anyhow::Result<GraphService> {
    std::fs::create_dir_all(data_dir)?;
    let identity = PeerIdentity::load_or_generate(&data_dir.join("identity.key"))?;
    let pool = oag_storage::open_pool(&data_dir.join("oag.sqlite")).await?;
    Ok(GraphService::new(pool, identity))
}

/// One-shot `SyncService` for the `oag peer *` commands — these operate
/// directly on the local SQLite file (WAL mode makes this safe alongside a
/// running `oag serve`, same as `status`/`search`/`key create` already do)
/// and don't need the federation policy from `config.toml`, since a
/// human explicitly running `oag peer add/sync` is itself the authorization
/// to talk to that address.
async fn open_sync(data_dir: &Path) -> anyhow::Result<SyncService> {
    std::fs::create_dir_all(data_dir)?;
    let identity = PeerIdentity::load_or_generate(&data_dir.join("identity.key"))?;
    let peer_id = identity.peer_id();
    let public_key = identity.verifying_key().to_bytes();
    let pool = oag_storage::open_pool(&data_dir.join("oag.sqlite")).await?;
    Ok(SyncService::new(pool, peer_id, public_key, FederationPolicy::Open))
}

pub async fn peer_add(data_dir: &Path, url: &str) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let summary = sync.sync_with_peer(url).await?;
    println!("synced with {url}");
    println!(
        "applied: {}, already_known: {}, forks: {}",
        summary.applied, summary.already_known, summary.forks
    );
    for err in &summary.errors {
        println!("warning: {err}");
    }
    Ok(())
}

pub async fn peer_sync(data_dir: &Path, peer_id_or_url: &str) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;

    let url = if let Ok(peer_id) = peer_id_or_url.parse::<oag_crypto::PeerId>() {
        let mut conn = sync.pool().acquire().await?;
        let addrs = oag_storage::repo::peers::list_addresses(&mut conn, peer_id.as_bytes()).await?;
        addrs
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("no known address for peer {peer_id} — use `oag peer add <URL>` first"))?
    } else {
        peer_id_or_url.to_string()
    };

    if let Some(address_hash) = url.strip_prefix("reticulum:") {
        anyhow::bail!(
            "peer {peer_id_or_url}'s stored address is a Reticulum destination \
             ({address_hash}), which needs a TCP endpoint to actually reach — \
             re-run as `oag peer add-reticulum {address_hash} <host:port>`"
        );
    }

    let summary = sync.sync_with_peer(&url).await?;
    println!("synced with {url}");
    println!(
        "applied: {}, already_known: {}, forks: {}",
        summary.applied, summary.already_known, summary.forks
    );
    for err in &summary.errors {
        println!("warning: {err}");
    }
    Ok(())
}

/// `oag peer fetch` (`spec/31-service-receipts.md`): fetch pinned content
/// directly from another OAG peer over the new `/oag/transfer/v1/fetch`
/// endpoint, with a mutual nonce-bound receipt — distinct from `oag peer
/// sync`'s graph-event replication, and from `/ipfs-site/{cid}`'s
/// browser/bitswap serving (this is peer-to-peer, not browser-to-peer, and
/// fully OAG-owned rather than riding on third-party bitswap).
pub async fn peer_fetch(
    data_dir: &Path,
    address: &str,
    expected_server_peer_id: &str,
    cid: &str,
    out: Option<&std::path::Path>,
) -> anyhow::Result<()> {
    let expected_server_peer_id: oag_crypto::PeerId = expected_server_peer_id
        .parse()
        .map_err(|e| anyhow::anyhow!("invalid expected server peer id: {e}"))?;

    let graph = open_graph(data_dir).await?;
    let identity = graph.identity();

    let client_nonce = oag_crypto::random_bytes_32();
    let request = oag_api::transfer::build_and_sign_request(identity, cid, &client_nonce, oag_api::transfer::now_unix());

    let url = format!("{}/oag/transfer/v1/fetch", address.trim_end_matches('/'));
    let http = reqwest::Client::new();
    let response = http.post(&url).json(&request).send().await?;
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("transfer request failed: {body}");
    }

    let receipt_header = response
        .headers()
        .get("x-oag-transfer-receipt")
        .ok_or_else(|| anyhow::anyhow!("response missing x-oag-transfer-receipt header"))?
        .to_str()
        .map_err(|e| anyhow::anyhow!("invalid receipt header encoding: {e}"))?
        .to_string();
    let receipt: oag_api::transfer::TransferReceipt = serde_json::from_str(&receipt_header)?;
    let content = response.bytes().await?.to_vec();

    let content_hash = oag_crypto::blake3_content_hash(&content);
    if content_hash != receipt.unsigned.content_hash {
        anyhow::bail!(
            "content hash mismatch: received content hashes to {content_hash}, receipt claims {}",
            receipt.unsigned.content_hash
        );
    }

    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs() as i64;
    let verified_server_peer_id = oag_api::transfer::verify_receipt(&receipt, cid, &client_nonce, now)
        .map_err(|e| anyhow::anyhow!("receipt verification failed: {e}"))?;
    if verified_server_peer_id != expected_server_peer_id {
        anyhow::bail!(
            "receipt signed by {verified_server_peer_id}, not the expected {expected_server_peer_id} \
             -- refusing to record a corroborating assertion"
        );
    }

    if let Some(path) = out {
        std::fs::write(path, &content)?;
    }

    oag_api::transfer::record_transfer(
        &graph,
        identity,
        &verified_server_peer_id,
        &identity.peer_id(),
        cid,
        content.len(),
        &receipt.unsigned.client_nonce,
        &receipt.unsigned.server_nonce,
        &content_hash,
        &receipt.unsigned.receipt_hash,
        "client",
        receipt.unsigned.served_at,
        oag_core::ExtractionMethod::Verification,
    )
    .await?;

    println!("verified:       true");
    println!("content_hash:   {content_hash}");
    println!("receipt_hash:   {}", receipt.unsigned.receipt_hash);
    println!("server_peer_id: {verified_server_peer_id}");
    println!("bytes:          {}", content.len());
    if let Some(path) = out {
        println!("written to:     {}", path.display());
    }

    Ok(())
}

pub async fn peer_list(data_dir: &Path) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let mut conn = sync.pool().acquire().await?;
    let peers = oag_storage::repo::peers::list_peers(&mut conn).await?;
    if peers.is_empty() {
        println!("no known peers");
        return Ok(());
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    // Ephemeral peer trust, Phase 1 (spec/24): this observer's own view of
    // each peer's trust, using the default trust-check windows rather than
    // whatever a running `oag serve` was configured with -- this is a
    // read-only snapshot for a human to inspect, not enforcement.
    let trust_config = oag_sync::TrustConfig::default();
    for info in peers {
        let peer_id = oag_crypto::PeerId::from_bytes(info.peer_id);
        let addrs = oag_storage::repo::peers::list_addresses(&mut conn, peer_id.as_bytes()).await?;
        let trust = oag_sync::trust::compute_trust(sync.pool(), &peer_id, now, &trust_config).await?;
        let cluster = oag_storage::repo::peer_cluster::get(&mut conn, peer_id.as_bytes())
            .await?
            .map(|claim| claim.cluster)
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{peer_id}  forked={}  trust={trust:.2}  cluster={cluster}  last_seen={:?}  addresses={:?}",
            info.forked, info.last_seen, addrs
        );
    }
    Ok(())
}

pub async fn peer_remove(data_dir: &Path, peer_id: &str) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let peer_id: oag_crypto::PeerId = peer_id
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid peer id '{peer_id}'"))?;
    let mut conn = sync.pool().acquire().await?;
    oag_storage::repo::peers::remove_peer(&mut conn, peer_id.as_bytes()).await?;
    println!("removed {peer_id}");
    Ok(())
}

pub async fn peer_forks(data_dir: &Path, peer_id: &str) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let peer_id: oag_crypto::PeerId = peer_id
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid peer id '{peer_id}'"))?;
    let mut conn = sync.pool().acquire().await?;
    let forks = oag_storage::repo::peers::list_forks(&mut conn, peer_id.as_bytes()).await?;
    if forks.is_empty() {
        println!("no recorded forks for {peer_id}");
        return Ok(());
    }
    for fork in forks {
        println!(
            "sequence={} event_id_a={} event_id_b={} detected_at={}",
            fork.sequence,
            hex::encode(&fork.event_id_a),
            hex::encode(&fork.event_id_b),
            fork.detected_at
        );
        println!("  event_b_signed_json={}", fork.event_b_signed_json);
    }
    Ok(())
}

pub async fn peer_reticulum_address(data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let identity = PeerIdentity::load_or_generate(&data_dir.join("identity.key"))?;
    let address_hash = oag_reticulum::local_address_hash(&identity);
    println!("reticulum_address: {}", address_hash.to_hex_string());
    Ok(())
}

pub async fn peer_add_reticulum(data_dir: &Path, address_hash: &str, via_tcp: &str) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let identity = PeerIdentity::load_or_generate(&data_dir.join("identity.key"))?;
    let target = oag_reticulum::parse_address_hash(address_hash)
        .map_err(|e| anyhow::anyhow!("invalid reticulum address '{address_hash}': {e:?}"))?;

    let summary = oag_reticulum::sync_with_peer(sync.pool(), &identity, via_tcp, target).await?;
    println!("synced with reticulum:{address_hash} via {via_tcp}");
    println!(
        "applied: {}, already_known: {}, forks: {}",
        summary.applied, summary.already_known, summary.forks
    );
    for err in &summary.errors {
        println!("warning: {err}");
    }
    Ok(())
}

pub async fn status(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    println!("peer_id: {}", graph.identity().peer_id());
    println!("data_dir: {}", data_dir.display());
    let mut conn = graph.pool().acquire().await?;
    let peer_count = oag_storage::repo::peers::list_peers(&mut conn).await?.len();
    println!("known_peers: {peer_count} (see `oag peer list`)");

    let sync = open_sync(data_dir).await?;
    let replication = sync.replication_status().await?;
    let verdict = if replication.meets_target { "OK" } else { "BELOW TARGET" };
    println!(
        "replication: {}/{} target replicas confirmed ({} known peers) — {verdict} (see `oag replication status`)",
        replication.peers_fully_caught_up, replication.target_replication_factor, replication.known_peer_count
    );
    Ok(())
}

pub async fn replication_status(data_dir: &Path) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let status = sync.replication_status().await?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

pub async fn authority_recompute(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let summary = graph.recompute_authority().await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub async fn search(
    data_dir: &Path,
    query: &str,
    limit: i64,
    semantic: bool,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let search_section = crate::config::resolve_search_section(config_path)?;
    // `--semantic` forces it on for this one invocation; otherwise fall back
    // to `[search].semantic_enabled` in config.toml as the standing default.
    if semantic || search_section.semantic_enabled {
        let provider = search_section.to_embedding_provider();
        let results = graph.semantic_search(provider.as_ref(), query, limit).await?;
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        let results = graph.search(query, limit).await?;
        println!("{}", serde_json::to_string_pretty(&results)?);
    }
    Ok(())
}

pub async fn embeddings_recompute(data_dir: &Path, config_path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let provider = crate::config::resolve_search_section(config_path)?.to_embedding_provider();
    let summary = graph.recompute_embeddings(provider.as_ref()).await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub async fn actor_get(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let actor_id = id.parse().map_err(|_| anyhow::anyhow!("invalid actor id '{id}'"))?;
    match graph.get_actor(actor_id).await? {
        Some(actor) => println!("{}", serde_json::to_string_pretty(&actor)?),
        None => println!("not found"),
    }
    Ok(())
}

pub async fn node_get(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let node_id = id.parse().map_err(|_| anyhow::anyhow!("invalid node id '{id}'"))?;
    match graph.get_node(node_id).await? {
        Some(node) => println!("{}", serde_json::to_string_pretty(&node)?),
        None => println!("not found"),
    }
    Ok(())
}

pub async fn assertion_get(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let assertion_id = id.parse().map_err(|_| anyhow::anyhow!("invalid assertion id '{id}'"))?;
    match graph.get_assertion(assertion_id).await? {
        Some(assertion) => {
            let evidence = graph.list_evidence(assertion_id).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "assertion": assertion,
                    "evidence": evidence,
                }))?
            );
        }
        None => println!("not found"),
    }
    Ok(())
}

pub async fn edge_get(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let edge_id = id.parse().map_err(|_| anyhow::anyhow!("invalid edge id '{id}'"))?;
    match graph.get_edge(edge_id).await? {
        Some(edge) => println!("{}", serde_json::to_string_pretty(&edge)?),
        None => println!("not found"),
    }
    Ok(())
}

pub async fn edge_corroboration(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let edge_id = id.parse().map_err(|_| anyhow::anyhow!("invalid edge id '{id}'"))?;
    let corroboration = graph.get_edge_corroboration(edge_id).await?;
    println!("{}", serde_json::to_string_pretty(&corroboration)?);
    Ok(())
}

pub async fn identity_show(data_dir: &Path, config_path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let peer_id = graph.identity().peer_id();
    println!("peer_id:   {peer_id}");
    println!("peer_name: {}", oag_petname::peer_name_string(&peer_id));
    // Long-word subdomain addressing, Phase 1 (spec/30): silent when
    // [addressing].domain isn't set, same "nothing extra when absent"
    // posture as every other opt-in feature's CLI output.
    let addressing = crate::config::resolve_addressing_section(config_path)?;
    if let Some(domain) = addressing.domain {
        println!("address:   {}", crate::addressing::fqdn_for_peer(&peer_id, &domain));
    }
    Ok(())
}

/// Resolves a peer's long-word DNS address via a real TXT lookup
/// (spec/30-long-word-addressing.md). `--domain` takes priority over
/// `[addressing].domain`; errors clearly if neither is given, rather than
/// silently resolving nothing.
pub async fn peer_resolve(
    peer_id: &str,
    domain_flag: Option<String>,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let peer_id: oag_crypto::PeerId =
        peer_id.parse().map_err(|_| anyhow::anyhow!("invalid peer id '{peer_id}'"))?;
    let domain = match domain_flag {
        Some(domain) => domain,
        None => crate::config::resolve_addressing_section(config_path)?
            .domain
            .ok_or_else(|| anyhow::anyhow!("no domain given -- pass --domain or set [addressing].domain in config.toml"))?,
    };
    let values = crate::addressing::resolve_peer_address(&peer_id, &domain).await?;
    for value in values {
        println!("{value}");
    }
    Ok(())
}

pub async fn tensor_show(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let peer_id = graph.identity().peer_id();
    let mut conn = graph.pool().acquire().await?;
    let values = oag_storage::repo::tensor_pads::get(&mut conn, peer_id.as_bytes())
        .await?
        .unwrap_or_else(|| oag_tensor::TensorPad::default().values);
    println!("{}", serde_json::to_string_pretty(&values)?);
    Ok(())
}

pub async fn gravity_show(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let peer_id = graph.identity().peer_id();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let gravity_level = oag_sync::gravity::get_or_generate(graph.pool(), &peer_id, now).await?;
    println!("gravity_level: {gravity_level}");
    Ok(())
}

pub async fn identity_backup(data_dir: &Path, out: &Path) -> anyhow::Result<()> {
    let source = data_dir.join("identity.key");
    if !source.exists() {
        // Ensure an identity exists before backing it up.
        open_graph(data_dir).await?;
    }
    std::fs::copy(&source, out)?;
    println!("backed up identity.key -> {}", out.display());
    println!(
        "warning: this file is your peer's private key. Store it securely; \
         anyone with it can sign events as this peer."
    );
    Ok(())
}

pub async fn identity_restore(data_dir: &Path, backup_path: &Path, force: bool) -> anyhow::Result<()> {
    if !backup_path.exists() {
        anyhow::bail!("backup file '{}' does not exist", backup_path.display());
    }
    std::fs::create_dir_all(data_dir)?;
    let dest = data_dir.join("identity.key");
    if dest.exists() && !force {
        anyhow::bail!(
            "{} already exists -- refusing to overwrite an existing peer identity \
             (this would desync its peer_id from every event already signed under the \
             current key). Pass --force if you're certain.",
            dest.display()
        );
    }

    std::fs::copy(backup_path, &dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600))?;
    }

    let identity = PeerIdentity::load_or_generate(&dest)?;
    println!("restored identity.key -> {}", dest.display());
    println!("peer_id: {}", identity.peer_id());
    Ok(())
}

pub async fn backup(
    data_dir: &Path,
    out: &Path,
    pin: bool,
    config_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    let pool = oag_storage::open_pool(&data_dir.join("oag.sqlite")).await?;
    oag_storage::backup_to(&pool, out).await?;
    println!("backed up database -> {}", out.display());
    println!(
        "warning: full peers replicate graph events, so losing this database usually isn't \
         catastrophic on its own -- but this snapshot is much faster to restore from than a \
         full re-sync, and is your only copy if this is a lone/first peer."
    );
    if pin {
        let pinning_provider = crate::config::resolve_pinning_provider(config_path, data_dir).await?;
        let bytes = tokio::fs::read(out).await?;
        let cid = pinning_provider
            .pin(&bytes)
            .await
            .map_err(|e| anyhow::anyhow!("{e} (is [ipfs].pinning_provider set in config.toml?)"))?;
        println!("pinned to IPFS: {cid}");
        println!(
            "note: this CID is an unverified retrieval pointer only, same as every other \
             pinned_cid in this project (spec/29-ipfs-pinning.md) -- not independently confirmed, \
             never a trust signal."
        );
    }
    Ok(())
}

pub async fn rebuild(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let summary = graph.rebuild_projection().await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

/// Spec section 85's redaction/suppression commands are CLI-only, gated by
/// filesystem access to the data directory -- the same trust boundary
/// `oag backup`/`oag rebuild` already rely on. This synthetic `AuthContext`
/// is never persisted or looked up anywhere; it only needs to carry
/// `Permission::Admin` through `GraphService`'s existing permission checks,
/// the same checks a future REST/MCP surface for these commands would reuse
/// unchanged with a real authenticated actor.
fn cli_admin_auth() -> AuthContext {
    AuthContext {
        actor_id: ActorId::derive(b"oag-cli:local-operator"),
        permissions: vec![Permission::Admin],
    }
}

pub async fn redact_evidence(
    data_dir: &Path,
    evidence_id: &str,
    reason: Option<String>,
    force: bool,
) -> anyhow::Result<()> {
    if !force {
        anyhow::bail!(
            "redaction permanently blanks this evidence's title/excerpt in your local database \
             and cannot be undone by any command in this tool. It also cannot reach copies of \
             this event already replicated to other peers -- an immutable distributed log can't \
             promise that (spec section 85). Pass --force if you're certain."
        );
    }
    let graph = open_graph(data_dir).await?;
    let event_id = evidence_id
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid evidence id '{evidence_id}'"))?;
    graph.redact_evidence(&cli_admin_auth(), event_id, reason).await?;
    println!("redacted evidence {evidence_id}");
    Ok(())
}

pub async fn redact_list(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let auth = cli_admin_auth();
    let redactions = graph.list_redactions(&auth).await?;
    let suppressed_nodes = graph.list_suppressed_nodes(&auth).await?;
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
        "redactions": redactions,
        "suppressed_nodes": suppressed_nodes,
    }))?);
    Ok(())
}

pub async fn redact_suppress_node(data_dir: &Path, node_id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let node_id = node_id.parse().map_err(|_| anyhow::anyhow!("invalid node id '{node_id}'"))?;
    graph.suppress_node_from_search(&cli_admin_auth(), node_id).await?;
    println!("suppressed {node_id} from search results (reversible: oag redact unsuppress-node)");
    Ok(())
}

pub async fn redact_unsuppress_node(data_dir: &Path, node_id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let node_id = node_id.parse().map_err(|_| anyhow::anyhow!("invalid node id '{node_id}'"))?;
    graph.unsuppress_node_from_search(&cli_admin_auth(), node_id).await?;
    println!("unsuppressed {node_id}");
    Ok(())
}

/// spec/27-artifact-versioning.md: "convert perm" is a permanent, irreversible
/// storage mutation outside the event log the moment a real second schema
/// shape ever exists for any artifact type (none does today, for any of the
/// four `oag migrate *` commands) -- gated by --force unconditionally, the
/// same discipline `redact_evidence` above already applies, rather than
/// special-casing today's always-no-op case.
fn require_migrate_force(force: bool) -> anyhow::Result<()> {
    if !force {
        anyhow::bail!(
            "converting an artifact to its current schema version is a permanent, irreversible \
             storage mutation outside the event log. Every normal read already serves \
             current-shape data regardless, via an in-memory shim applied automatically -- this \
             command only matters for skipping that conversion on future reads, by rewriting the \
             stored row now. Pass --force if you're certain."
        );
    }
    Ok(())
}

pub async fn migrate_node(data_dir: &Path, id: &str, force: bool) -> anyhow::Result<()> {
    require_migrate_force(force)?;
    let graph = open_graph(data_dir).await?;
    let node_id = id.parse().map_err(|_| anyhow::anyhow!("invalid node id '{id}'"))?;
    let outcome = graph.migrate_node(&cli_admin_auth(), node_id).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

pub async fn migrate_edge(data_dir: &Path, id: &str, force: bool) -> anyhow::Result<()> {
    require_migrate_force(force)?;
    let graph = open_graph(data_dir).await?;
    let edge_id = id.parse().map_err(|_| anyhow::anyhow!("invalid edge id '{id}'"))?;
    let outcome = graph.migrate_edge(&cli_admin_auth(), edge_id).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

pub async fn migrate_assertion(data_dir: &Path, id: &str, force: bool) -> anyhow::Result<()> {
    require_migrate_force(force)?;
    let graph = open_graph(data_dir).await?;
    let assertion_id = id.parse().map_err(|_| anyhow::anyhow!("invalid assertion id '{id}'"))?;
    let outcome = graph.migrate_assertion(&cli_admin_auth(), assertion_id).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

pub async fn migrate_evidence(data_dir: &Path, id: &str, force: bool) -> anyhow::Result<()> {
    require_migrate_force(force)?;
    let graph = open_graph(data_dir).await?;
    let evidence_id = id.parse().map_err(|_| anyhow::anyhow!("invalid evidence id '{id}'"))?;
    let outcome = graph.migrate_evidence(&cli_admin_auth(), evidence_id).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

pub async fn migrate_actor(data_dir: &Path, id: &str, force: bool) -> anyhow::Result<()> {
    require_migrate_force(force)?;
    let graph = open_graph(data_dir).await?;
    let actor_id = id.parse().map_err(|_| anyhow::anyhow!("invalid actor id '{id}'"))?;
    let outcome = graph.migrate_actor(&cli_admin_auth(), actor_id).await?;
    println!("{}", serde_json::to_string_pretty(&outcome)?);
    Ok(())
}

pub async fn key_create(
    data_dir: &Path,
    actor_type: &str,
    name: Option<String>,
    permissions: Vec<String>,
    identity_uri: Option<String>,
    public_key: Option<String>,
    key_proof: Option<String>,
) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let actor_type = ActorType::parse(actor_type)
        .ok_or_else(|| anyhow::anyhow!("unknown actor type '{actor_type}'"))?;
    let permissions: Vec<Permission> = permissions
        .iter()
        .map(|p| Permission::parse(p).ok_or_else(|| anyhow::anyhow!("unknown permission '{p}'")))
        .collect::<Result<_, _>>()?;

    let public_key_proof = match (public_key, key_proof) {
        (Some(public_key_hex), Some(key_proof_hex)) => {
            let public_key: [u8; 32] = hex::decode(&public_key_hex)
                .map_err(|e| anyhow::anyhow!("invalid --public-key hex: {e}"))?
                .try_into()
                .map_err(|v: Vec<u8>| anyhow::anyhow!("--public-key must be 32 bytes, got {}", v.len()))?;
            let signature: [u8; 64] = hex::decode(&key_proof_hex)
                .map_err(|e| anyhow::anyhow!("invalid --key-proof hex: {e}"))?
                .try_into()
                .map_err(|v: Vec<u8>| anyhow::anyhow!("--key-proof must be 64 bytes, got {}", v.len()))?;
            Some(oag_graph::PublicKeyProof { public_key, signature })
        }
        (None, None) => None,
        _ => unreachable!("clap's requires= already enforces both-or-neither"),
    };

    let actor_id = graph.declare_actor(actor_type, name, identity_uri, public_key_proof).await?;
    let raw_key = graph.create_key(actor_id, permissions).await?;

    println!("actor_id: {}", actor_id.to_hex());
    println!("api_key:  {raw_key}");
    println!("(shown once — only its hash is stored)");
    Ok(())
}

pub async fn key_list(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let keys = graph.list_keys().await?;
    println!("{}", serde_json::to_string_pretty(&keys)?);
    Ok(())
}

pub async fn key_revoke(data_dir: &Path, key_hash: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    graph.revoke_key(key_hash).await?;
    println!("revoked key {key_hash}");
    Ok(())
}

pub async fn doctor(data_dir: &Path, config_path: Option<std::path::PathBuf>) -> anyhow::Result<()> {
    let mut ok = true;

    print!("config... ");
    match &config_path {
        Some(path) => {
            // Reuses the exact function `oag serve` itself calls to resolve
            // config.toml -- no separate, potentially-divergent validation
            // logic. A config that fails to resolve here would fail `oag
            // serve` identically, just later and less conveniently.
            match crate::config::resolve(Some(path.clone()), Some(data_dir.to_path_buf()), None).await {
                Ok(_) => println!("ok ({})", path.display()),
                Err(e) => {
                    println!("FAIL ({e})");
                    ok = false;
                }
            }
        }
        None => println!("none specified, using defaults"),
    }

    print!("data directory writable... ");
    match std::fs::create_dir_all(data_dir) {
        Ok(()) => println!("ok ({})", data_dir.display()),
        Err(e) => {
            println!("FAIL ({e})");
            ok = false;
        }
    }

    print!("peer identity... ");
    let identity = match PeerIdentity::load_or_generate(&data_dir.join("identity.key")) {
        Ok(identity) => {
            println!("ok ({})", identity.peer_id());
            Some(identity)
        }
        Err(e) => {
            println!("FAIL ({e})");
            ok = false;
            None
        }
    };

    print!("database opens + migrations current... ");
    match oag_storage::open_pool(&data_dir.join("oag.sqlite")).await {
        Ok(pool) => {
            println!("ok");
            if let Some(identity) = identity {
                let graph = GraphService::new(pool, identity);
                let has_admin = graph.has_any_actor().await.unwrap_or(false);
                println!(
                    "actors present... {}",
                    if has_admin { "ok" } else { "none yet (fresh peer)" }
                );
            }
        }
        Err(e) => {
            println!("FAIL ({e})");
            ok = false;
        }
    }

    if ok {
        println!("\nhealthy");
        Ok(())
    } else {
        anyhow::bail!("one or more checks failed")
    }
}

pub async fn crawl(
    data_dir: &Path,
    url: &str,
    config_path: Option<std::path::PathBuf>,
    allow_private_networks_flag: bool,
) -> anyhow::Result<()> {
    let url = url::Url::parse(url).map_err(|e| anyhow::anyhow!("invalid URL '{url}': {e}"))?;
    let (crawler_config, llm_extractor, pinning_provider) =
        crate::config::resolve_crawler_config(config_path, allow_private_networks_flag, data_dir).await?;

    let graph = std::sync::Arc::new(open_graph(data_dir).await?);
    let crawler = oag_crawler::CrawlerService::new(graph, crawler_config, llm_extractor, pinning_provider);

    let summary = crawler.crawl(&url).await?;

    println!("crawled: {}", summary.page_url);
    println!("facts asserted: {}", summary.facts_asserted);
    if summary.facts_skipped > 0 {
        println!("facts skipped (failed validation): {}", summary.facts_skipped);
    }
    println!("aliases declared: {}", summary.aliases_declared);
    println!("llms.txt found: {}", summary.llms_txt_found);
    println!("ARD found: {}", summary.ard_found);
    println!("A2A agent card found: {}", summary.a2a_found);
    if summary.llm_candidates_asserted > 0 {
        println!("LLM candidate assertions: {}", summary.llm_candidates_asserted);
    }
    Ok(())
}

/// Retrieve previously-pinned content directly from the configured provider
/// (spec/29-ipfs-pinning.md) — no `GraphService` involved, since this talks
/// to IPFS, not to a peer's graph. `data_dir` is only read by
/// `pinning_provider = "embedded"`, to find its on-disk blockstore — must
/// match the data dir the content was originally pinned under. Writes raw
/// bytes to stdout (not JSON-wrapped) so it can be piped to a file/viewer,
/// matching a real `ipfs cat`'s own behavior.
pub async fn ipfs_cat(cid: &str, config_path: Option<std::path::PathBuf>, data_dir: &Path) -> anyhow::Result<()> {
    let (_, _, pinning_provider) =
        crate::config::resolve_crawler_config(config_path, false, data_dir).await?;
    let content = pinning_provider
        .fetch(cid)
        .await
        .map_err(|e| anyhow::anyhow!("{e} (is [ipfs].pinning_provider set in config.toml?)"))?;
    use std::io::Write;
    std::io::stdout().write_all(&content)?;
    Ok(())
}

/// A whole small static site, generously sized but not unbounded -- same
/// order of magnitude as the crawler's own `max_response_bytes` (10 MiB for
/// one fetched page), see `docs/security.md`'s "Response/request size
/// caps."
const MAX_HOST_PAGE_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_HOST_PAGE_FILE_COUNT: usize = 200;

/// Recursively collect `(relative_path, content)` pairs for every regular
/// file under `dir`, using `/`-separated relative paths regardless of host
/// OS (kubo's UnixFS path semantics, and every browser URL, expect `/`).
/// Hand-rolled rather than pulling in `walkdir` -- a one-off recursive
/// directory walk doesn't justify a new dependency, same posture as
/// `oag-ipfs`'s own hand-rolled HTTP client.
fn collect_directory_files(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_directory_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.push((relative, std::fs::read(&path)?));
        }
    }
    Ok(())
}

/// Pin a local directory as one static site (spec/29-ipfs-pinning.md,
/// "static page hosting" — `USER_INPUT_RECORD.md` Entry 19). Unlike `oag
/// backup --pin` (one file), this builds a single UnixFS directory tree so
/// a browser can resolve `index.html`, `css/style.css`, etc. all under one
/// root CID.
pub async fn ipfs_host_page(
    dir: &Path,
    config_path: Option<std::path::PathBuf>,
    data_dir: &Path,
) -> anyhow::Result<()> {
    anyhow::ensure!(dir.is_dir(), "{} is not a directory", dir.display());

    let mut files = Vec::new();
    collect_directory_files(dir, dir, &mut files)?;
    anyhow::ensure!(!files.is_empty(), "{} contains no files to pin", dir.display());
    anyhow::ensure!(
        files.len() <= MAX_HOST_PAGE_FILE_COUNT,
        "{} contains {} files, over this command's {MAX_HOST_PAGE_FILE_COUNT}-file sanity cap",
        dir.display(),
        files.len()
    );
    let total_bytes: u64 = files.iter().map(|(_, content)| content.len() as u64).sum();
    anyhow::ensure!(
        total_bytes <= MAX_HOST_PAGE_TOTAL_BYTES,
        "{} is {total_bytes} bytes, over this command's {MAX_HOST_PAGE_TOTAL_BYTES}-byte sanity cap",
        dir.display()
    );
    if !files.iter().any(|(path, _)| path == "index.html") {
        println!(
            "warning: no top-level index.html found -- this will pin successfully, but \
             `oag serve`'s /ipfs-site/<cid> bootstrap page expects one at the directory root"
        );
    }

    let pinning_provider = crate::config::resolve_pinning_provider(config_path, data_dir).await?;
    let cid = pinning_provider
        .pin_directory(&files)
        .await
        .map_err(|e| anyhow::anyhow!("{e} (is [ipfs].pinning_provider set in config.toml?)"))?;
    println!("pinned {} file(s) to IPFS as a directory: {cid}", files.len());
    println!("open it via a running `oag serve` (same [ipfs] config) at: /ipfs-site/{cid}");
    println!(
        "note: this CID is an unverified retrieval pointer only, same as every other pinned_cid \
         in this project (spec/29-ipfs-pinning.md) -- not independently confirmed, never a trust \
         signal."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-cli-test-{name}-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn identity_restore_round_trips_the_same_peer_id() {
        let source_dir = temp_dir("restore-source");
        let original = open_graph(&source_dir).await.unwrap();
        let original_peer_id = original.identity().peer_id();

        let backup_path = temp_dir("restore-backup").join("identity.key.bak");
        identity_backup(&source_dir, &backup_path).await.unwrap();

        let fresh_dir = temp_dir("restore-fresh");
        identity_restore(&fresh_dir, &backup_path, false).await.unwrap();
        let restored = open_graph(&fresh_dir).await.unwrap();
        assert_eq!(restored.identity().peer_id(), original_peer_id);
    }

    #[tokio::test]
    async fn identity_restore_refuses_to_overwrite_without_force() {
        let existing_dir = temp_dir("restore-existing");
        open_graph(&existing_dir).await.unwrap(); // creates identity.key

        let other_dir = temp_dir("restore-other-source");
        let backup_path = temp_dir("restore-other-backup").join("identity.key.bak");
        identity_backup(&other_dir, &backup_path).await.unwrap();

        let result = identity_restore(&existing_dir, &backup_path, false).await;
        assert!(result.is_err(), "should refuse to overwrite an existing identity without --force");
    }

    #[tokio::test]
    async fn identity_restore_overwrites_with_force() {
        let existing_dir = temp_dir("restore-force-existing");
        open_graph(&existing_dir).await.unwrap();

        let other_dir = temp_dir("restore-force-source");
        let other_identity = open_graph(&other_dir).await.unwrap();
        let other_peer_id = other_identity.identity().peer_id();
        let backup_path = temp_dir("restore-force-backup").join("identity.key.bak");
        identity_backup(&other_dir, &backup_path).await.unwrap();

        identity_restore(&existing_dir, &backup_path, true).await.unwrap();
        let restored = open_graph(&existing_dir).await.unwrap();
        assert_eq!(restored.identity().peer_id(), other_peer_id);
    }

    #[tokio::test]
    async fn backup_and_rebuild_smoke_test() {
        let data_dir = temp_dir("backup-rebuild-smoke");
        open_graph(&data_dir).await.unwrap();

        let out_path = temp_dir("backup-rebuild-out").join("snapshot.sqlite");
        backup(&data_dir, &out_path, false, None).await.unwrap();
        assert!(out_path.exists());

        rebuild(&data_dir).await.unwrap();
    }

    /// `--pin` with no `[ipfs]` configured (the default) must surface a
    /// clear disabled-provider error, not panic -- same posture as `oag
    /// ipfs cat` with pinning disabled.
    #[tokio::test]
    async fn backup_with_pin_but_no_ipfs_configured_is_a_clear_error() {
        let data_dir = temp_dir("backup-pin-disabled");
        open_graph(&data_dir).await.unwrap();
        let out_path = temp_dir("backup-pin-disabled-out").join("snapshot.sqlite");

        let err = backup(&data_dir, &out_path, true, None).await.unwrap_err();
        assert!(err.to_string().contains("disabled"), "got {err}");
    }

    async fn seed_evidence(data_dir: &Path) -> String {
        let graph = open_graph(data_dir).await.unwrap();
        let actor_id = graph
            .declare_actor(ActorType::Agent, Some("seed".into()), None, None)
            .await
            .unwrap();
        let auth = AuthContext { actor_id, permissions: vec![Permission::GraphAssert] };
        let assertion_id = graph
            .assert(
                &auth,
                oag_graph::AssertInput {
                    subject: "https://example.com/redact-cli-target".into(),
                    subject_type: None,
                    predicate: "instance_of".into(),
                    object: "concept:redact-cli-test".into(),
                    object_type: None,
                    evidence: vec![],
                    actor_confidence: Some(0.9),
                    observed_at: None,
                    extraction_method: None,
                },
            )
            .await
            .unwrap();
        graph
            .add_evidence(
                &auth,
                assertion_id,
                oag_graph::EvidenceInput {
                    evidence_type: Some("documentation".into()),
                    title: Some("A Very Personal Title".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        graph.list_evidence(assertion_id).await.unwrap()[0].id.to_hex()
    }

    #[tokio::test]
    async fn redact_evidence_refuses_without_force() {
        let data_dir = temp_dir("redact-no-force");
        let evidence_id = seed_evidence(&data_dir).await;

        let result = redact_evidence(&data_dir, &evidence_id, None, false).await;
        assert!(result.is_err(), "should refuse to redact without --force");
    }

    #[tokio::test]
    async fn redact_evidence_succeeds_with_force_and_lists_the_reason() {
        let data_dir = temp_dir("redact-with-force");
        let evidence_id = seed_evidence(&data_dir).await;

        redact_evidence(&data_dir, &evidence_id, Some("test reason".to_string()), true).await.unwrap();

        let graph = open_graph(&data_dir).await.unwrap();
        let redactions = graph.list_redactions(&cli_admin_auth()).await.unwrap();
        assert_eq!(redactions.len(), 1);
        assert_eq!(redactions[0].reason.as_deref(), Some("test reason"));
    }

    #[tokio::test]
    async fn redact_suppress_node_shows_up_in_list_suppressed_nodes() {
        let data_dir = temp_dir("redact-suppress-list");
        seed_evidence(&data_dir).await;

        let graph = open_graph(&data_dir).await.unwrap();
        let oag_graph::ResolveOutcome::Found { node, .. } =
            graph.resolve("https://example.com/redact-cli-target").await.unwrap()
        else {
            panic!("expected the seeded subject to resolve");
        };

        redact_suppress_node(&data_dir, &node.id.to_hex()).await.unwrap();

        let suppressed = graph.list_suppressed_nodes(&cli_admin_auth()).await.unwrap();
        assert_eq!(suppressed.len(), 1);
        assert_eq!(suppressed[0].node_id, node.id);

        redact_unsuppress_node(&data_dir, &node.id.to_hex()).await.unwrap();
        assert!(graph.list_suppressed_nodes(&cli_admin_auth()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn doctor_passes_with_no_config_specified() {
        let data_dir = temp_dir("doctor-no-config");
        doctor(&data_dir, None).await.unwrap();
    }

    #[tokio::test]
    async fn doctor_passes_with_a_valid_config_file() {
        let data_dir = temp_dir("doctor-valid-config");
        let config_path = temp_dir("doctor-valid-config-file").join("config.toml");
        std::fs::write(&config_path, "[server]\nlisten = \"127.0.0.1:9999\"\n").unwrap();
        doctor(&data_dir, Some(config_path)).await.unwrap();
    }

    #[tokio::test]
    async fn doctor_fails_with_a_malformed_config_file() {
        let data_dir = temp_dir("doctor-bad-config");
        let config_path = temp_dir("doctor-bad-config-file").join("config.toml");
        std::fs::write(&config_path, "this is not valid toml {{{").unwrap();
        let result = doctor(&data_dir, Some(config_path)).await;
        assert!(result.is_err(), "a malformed config.toml must fail oag doctor");
    }

    #[tokio::test]
    async fn key_revoke_deactivates_the_key_and_survives_rebuild() {
        let data_dir = temp_dir("key-revoke-cli");
        let graph = open_graph(&data_dir).await.unwrap();
        let actor_id =
            graph.declare_actor(ActorType::Agent, Some("revoke-cli-test".into()), None, None).await.unwrap();
        let raw_key = graph.create_key(actor_id, vec![Permission::GraphRead]).await.unwrap();

        // The key authenticates before revocation.
        assert!(graph.authenticate(&raw_key).await.is_ok());

        let keys = key_list_for_test(&data_dir).await;
        assert_eq!(keys.len(), 1);
        assert!(keys[0].revoked_at.is_none());
        let key_hash = keys[0].key_hash.clone();

        key_revoke(&data_dir, &key_hash).await.unwrap();

        // No longer authenticates.
        let graph = open_graph(&data_dir).await.unwrap();
        assert!(matches!(graph.authenticate(&raw_key).await, Err(oag_graph::GraphError::InvalidApiKey)));

        let keys = key_list_for_test(&data_dir).await;
        assert_eq!(keys.len(), 1, "revoked keys stay listed, just flagged");
        assert!(keys[0].revoked_at.is_some());

        // Revocation is itself an event -- it must survive oag rebuild.
        rebuild(&data_dir).await.unwrap();
        let graph = open_graph(&data_dir).await.unwrap();
        assert!(
            matches!(graph.authenticate(&raw_key).await, Err(oag_graph::GraphError::InvalidApiKey)),
            "revocation must survive rebuild, not be resurrected by replaying the original ACTOR_KEY_ADD"
        );
    }

    #[tokio::test]
    async fn key_revoke_unknown_hash_is_not_found() {
        let data_dir = temp_dir("key-revoke-unknown");
        open_graph(&data_dir).await.unwrap();
        let result = key_revoke(&data_dir, &"ab".repeat(32)).await;
        assert!(result.is_err(), "revoking an unknown key hash must error, not silently succeed");
    }

    async fn key_list_for_test(data_dir: &Path) -> Vec<oag_storage::repo::actors::ApiKeyInfo> {
        open_graph(data_dir).await.unwrap().list_keys().await.unwrap()
    }
}
