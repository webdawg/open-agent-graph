use std::path::Path;

use oag_core::{ActorType, Permission};
use oag_crypto::PeerIdentity;
use oag_graph::GraphService;

async fn open_graph(data_dir: &Path) -> anyhow::Result<GraphService> {
    std::fs::create_dir_all(data_dir)?;
    let identity = PeerIdentity::load_or_generate(&data_dir.join("identity.key"))?;
    let pool = oag_storage::open_pool(&data_dir.join("oag.sqlite")).await?;
    Ok(GraphService::new(pool, identity))
}

pub async fn status(data_dir: &Path) -> anyhow::Result<()> {
    let graph = open_graph(data_dir).await?;
    println!("peer_id: {}", graph.identity().peer_id());
    println!("data_dir: {}", data_dir.display());
    println!("replication: none (single-peer milestone)");
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
