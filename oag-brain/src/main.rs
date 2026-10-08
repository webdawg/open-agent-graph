mod brain;
mod server;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use oag_crypto::PeerIdentity;
use oag_tensor::TensorPad;
use server::AppState;

/// The evolution layer: programs this node's tensor pad via real scaled
/// dot-product self-attention, run jointly with one other "ant brain."
/// Shares the same `--data-dir` (identity.key + oag.sqlite) as the main
/// `oag serve` process it's paired with, but is never spawned by it.
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "./data")]
    data_dir: PathBuf,

    /// Run the exchange server, bound to this address.
    #[arg(long, conflicts_with = "peer")]
    listen: Option<String>,

    /// One-shot: exchange this node's pad with the brain running at this
    /// URL (e.g. http://127.0.0.1:19801), then exit.
    #[arg(long, conflicts_with = "listen")]
    peer: Option<String>,
}

fn now_ts() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    std::fs::create_dir_all(&args.data_dir)?;
    let identity = PeerIdentity::load_or_generate(&args.data_dir.join("identity.key"))?;
    let self_peer_id = *identity.peer_id().as_bytes();
    let pool = oag_storage::open_pool(&args.data_dir.join("oag.sqlite")).await?;

    if let Some(peer_url) = args.peer {
        let mut conn = pool.acquire().await?;
        let own_pad = oag_storage::repo::tensor_pads::get(&mut conn, &self_peer_id).await?.unwrap_or_default();
        let own_pad = TensorPad::from_values(own_pad).values;
        drop(conn);

        let client = reqwest::Client::new();
        let response: serde_json::Value = client
            .post(format!("{peer_url}/brain/exchange"))
            .json(&serde_json::json!({ "pad": own_pad }))
            .send()
            .await?
            .json()
            .await?;
        let programmed_pad: Vec<f32> = serde_json::from_value(response["pad"].clone())?;
        let programmed_pad = TensorPad::from_values(programmed_pad).values;

        let mut conn = pool.acquire().await?;
        oag_storage::repo::tensor_pads::upsert(&mut conn, &self_peer_id, &programmed_pad, now_ts()).await?;

        println!("exchanged with {peer_url}");
        println!("{}", serde_json::to_string_pretty(&programmed_pad)?);
        return Ok(());
    }

    let listen = args.listen.unwrap_or_else(|| "127.0.0.1:19801".to_string());
    let state = Arc::new(AppState { pool, self_peer_id, brain: brain::Brain::new()? });
    let app = server::build_app(state);
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    println!("oag-brain: listening on http://{listen}/brain/exchange");
    axum::serve(listener, app).await?;
    Ok(())
}
