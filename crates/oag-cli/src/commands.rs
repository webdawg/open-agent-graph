use std::path::Path;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;
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

pub async fn peer_list(data_dir: &Path) -> anyhow::Result<()> {
    let sync = open_sync(data_dir).await?;
    let mut conn = sync.pool().acquire().await?;
    let peers = oag_storage::repo::peers::list_peers(&mut conn).await?;
    if peers.is_empty() {
        println!("no known peers");
        return Ok(());
    }
    for info in peers {
        let peer_id = oag_crypto::PeerId::from_bytes(info.peer_id);
        let addrs = oag_storage::repo::peers::list_addresses(&mut conn, peer_id.as_bytes()).await?;
        println!(
            "{peer_id}  forked={}  last_seen={:?}  addresses={:?}",
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

pub async fn edge_corroboration(data_dir: &Path, id: &str) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let edge_id = id.parse().map_err(|_| anyhow::anyhow!("invalid edge id '{id}'"))?;
    let corroboration = graph.get_edge_corroboration(edge_id).await?;
    println!("{}", serde_json::to_string_pretty(&corroboration)?);
    Ok(())
}

pub async fn identity_show(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let peer_id = graph.identity().peer_id();
    println!("peer_id:   {peer_id}");
    println!("peer_name: {}", oag_petname::peer_name_string(&peer_id));
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

pub async fn backup(data_dir: &Path, out: &Path) -> anyhow::Result<()> {
    let pool = oag_storage::open_pool(&data_dir.join("oag.sqlite")).await?;
    oag_storage::backup_to(&pool, out).await?;
    println!("backed up database -> {}", out.display());
    println!(
        "warning: full peers replicate graph events, so losing this database usually isn't \
         catastrophic on its own -- but this snapshot is much faster to restore from than a \
         full re-sync, and is your only copy if this is a lone/first peer."
    );
    Ok(())
}

pub async fn rebuild(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let summary = graph.rebuild_projection().await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
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

pub async fn doctor(data_dir: &Path) -> anyhow::Result<()> {
    let mut ok = true;

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
    let (crawler_config, llm_extractor) =
        crate::config::resolve_crawler_config(config_path, allow_private_networks_flag)?;

    let graph = std::sync::Arc::new(open_graph(data_dir).await?);
    let crawler = oag_crawler::CrawlerService::new(graph, crawler_config, llm_extractor);

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
        backup(&data_dir, &out_path).await.unwrap();
        assert!(out_path.exists());

        rebuild(&data_dir).await.unwrap();
    }
}
