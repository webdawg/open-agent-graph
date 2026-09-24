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

pub async fn status(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    println!("peer_id: {}", graph.identity().peer_id());
    println!("data_dir: {}", data_dir.display());
    let mut conn = graph.pool().acquire().await?;
    let peer_count = oag_storage::repo::peers::list_peers(&mut conn).await?.len();
    println!("known_peers: {peer_count} (see `oag peer list`)");
    Ok(())
}

pub async fn search(data_dir: &Path, query: &str, limit: i64) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let results = graph.search(query, limit).await?;
    println!("{}", serde_json::to_string_pretty(&results)?);
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

pub async fn identity_show(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    println!("peer_id: {}", graph.identity().peer_id());
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

pub async fn key_create(
    data_dir: &Path,
    actor_type: &str,
    name: Option<String>,
    permissions: Vec<String>,
) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    let actor_type = ActorType::parse(actor_type)
        .ok_or_else(|| anyhow::anyhow!("unknown actor type '{actor_type}'"))?;
    let permissions: Vec<Permission> = permissions
        .iter()
        .map(|p| Permission::parse(p).ok_or_else(|| anyhow::anyhow!("unknown permission '{p}'")))
        .collect::<Result<_, _>>()?;

    let actor_id = graph.declare_actor(actor_type, name, None).await?;
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
