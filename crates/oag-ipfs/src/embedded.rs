use std::path::PathBuf;

use bytes::Bytes;
use futures::StreamExt;
use rust_ipfs::builder::DefaultIpfsBuilder;
use rust_ipfs::unixfs::ll::dir::builder::{BufferingTreeBuilder, TreeOptions};
use rust_ipfs::{Block, Ipfs, IpfsPath, Multiaddr};

use crate::error::IpfsError;
use crate::provider::PinningProvider;

/// A real IPFS node embedded directly in this process -- no separate daemon,
/// matching this project's "single self-contained binary" posture (unlike
/// `KuboHttpProvider`, which talks to a *separately run* Kubo process over
/// HTTP). Built on `rust-ipfs` (the actively-maintained `dariusc93` fork,
/// not the archived original), composed without its `gateway`/`pinning`
/// (remote pinning-*service*)/`routing` features -- all three pull in
/// `reqwest` for HTTP calls to third-party services, the same "no proxy"
/// principle this project's browser-side IPFS bundle already applies.
///
/// Speaks the same bitswap/libp2p wire protocol as Kubo and the browser's
/// Helia bundle, so a browser loading `/ipfs-site/{cid}` can bitswap-fetch
/// directly from this node once it has a dial address for it (see
/// `dialable_multiaddrs`).
pub struct EmbeddedIpfsProvider {
    ipfs: Ipfs,
}

impl EmbeddedIpfsProvider {
    /// `repo_path` is this node's own on-disk blockstore/keystore directory
    /// (e.g. `<data-dir>/ipfs/`), distinct from OAG's own SQLite database.
    /// `listen_addrs` are the multiaddrs to bind on startup -- typically one
    /// `/ip4/0.0.0.0/tcp/<port-or-0>/ws` entry, the only transport a browser
    /// can dial directly.
    pub async fn new(repo_path: PathBuf, listen_addrs: Vec<Multiaddr>) -> Result<Self, IpfsError> {
        let ipfs = DefaultIpfsBuilder::new()
            .set_path(repo_path)
            .with_default()
            .enable_tcp()
            .enable_websocket()
            .with_mdns()
            .add_listening_addrs(listen_addrs)
            .start()
            .await
            .map_err(|e| IpfsError::Embedded(e.to_string()))?;
        Ok(Self { ipfs })
    }

    /// This node's own real, bound dial addresses (resolved only after
    /// `new` returns -- an ephemeral `/tcp/0/ws` listen address only becomes
    /// a real port once actually bound), each with this node's own
    /// `/p2p/<peer-id>` suffix so a browser can dial it directly without a
    /// separate discovery step. This is what feeds the `/ipfs-site/{cid}`
    /// bootstrap page in place of a hand-configured multiaddr.
    pub async fn real_dialable_multiaddrs(&self) -> Result<Vec<Multiaddr>, IpfsError> {
        let peer_id = self.ipfs.keypair().public().to_peer_id();
        // `start()` returning doesn't guarantee the swarm has processed its
        // own listener-bound events yet -- confirmed live: an immediate
        // call here can see an empty list even with a real listen address
        // configured. A few short retries covers the real-but-brief gap
        // without inventing a fixed, possibly-too-short sleep.
        let mut addrs = Vec::new();
        for _ in 0..20 {
            addrs = self
                .ipfs
                .listening_addresses()
                .await
                .map_err(|e| IpfsError::Embedded(e.to_string()))?;
            if !addrs.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Ok(addrs
            .into_iter()
            .map(|addr| {
                if addr.to_string().contains("/p2p/") {
                    addr
                } else {
                    addr.with(rust_ipfs::Protocol::P2p(peer_id))
                }
            })
            .collect())
    }
}

#[async_trait::async_trait]
impl PinningProvider for EmbeddedIpfsProvider {
    async fn pin(&self, content: &[u8]) -> Result<String, IpfsError> {
        let bytes = Bytes::copy_from_slice(content);
        let mut stream = self.ipfs.add_unixfs(bytes).pin(true);
        let mut last_path = None;
        while let Some(status) = stream.next().await {
            match status {
                rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { path, .. } => {
                    last_path = Some(path);
                }
                rust_ipfs::unixfs::UnixfsStatus::FailedStatus { error, .. } => {
                    return Err(IpfsError::Embedded(error.to_string()));
                }
                rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { .. } => {}
            }
        }
        let path = last_path.ok_or(IpfsError::EmptyResponse)?;
        let cid = path.root().cid().ok_or(IpfsError::EmptyResponse)?;
        Ok(cid.to_string())
    }

    async fn fetch(&self, cid: &str) -> Result<Vec<u8>, IpfsError> {
        let path: IpfsPath = cid.parse().map_err(|_| IpfsError::EmptyResponse)?;
        let mut stream = self.ipfs.unixfs().cat(path);
        let mut out = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| IpfsError::Embedded(e.to_string()))?;
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    async fn pin_directory(&self, files: &[(String, Vec<u8>)]) -> Result<String, IpfsError> {
        let repo = self.ipfs.repo().clone();
        let mut opts = TreeOptions::default();
        // Without this, more than one top-level entry (any directory with
        // more than one file) is rejected outright -- confirmed live, not
        // assumed: the first version of this method omitted it and every
        // multi-file directory failed with "multiple root level entries
        // while configured wrap_with_directory = false".
        opts.wrap_with_directory();
        let mut tree = BufferingTreeBuilder::new(opts);
        for (relative_path, content) in files {
            let bytes = Bytes::copy_from_slice(content);
            let mut stream = self.ipfs.add_unixfs(bytes).pin(false);
            let mut last_path = None;
            while let Some(status) = stream.next().await {
                match status {
                    rust_ipfs::unixfs::UnixfsStatus::CompletedStatus { path, .. } => {
                        last_path = Some(path);
                    }
                    rust_ipfs::unixfs::UnixfsStatus::FailedStatus { error, .. } => {
                        return Err(IpfsError::Embedded(error.to_string()));
                    }
                    rust_ipfs::unixfs::UnixfsStatus::ProgressStatus { .. } => {}
                }
            }
            let path = last_path.ok_or(IpfsError::EmptyResponse)?;
            let cid = *path.root().cid().ok_or(IpfsError::EmptyResponse)?;
            tree.put_link(relative_path, cid, content.len() as u64)
                .map_err(|e| IpfsError::Embedded(e.to_string()))?;
        }

        let mut iter = tree.build();
        let mut to_put = Vec::new();
        let mut last_cid = None;
        while let Some(node) = iter.next_borrowed() {
            let node = node.map_err(|e| IpfsError::Embedded(e.to_string()))?;
            let cid = node.cid.to_owned();
            to_put.push(Block::new(cid, node.block.to_vec()).map_err(|e| IpfsError::Embedded(e.to_string()))?);
            last_cid = Some(cid);
        }
        repo.put_blocks(to_put).await.map_err(|e| IpfsError::Embedded(e.to_string()))?;
        let root_cid = last_cid.ok_or(IpfsError::EmptyResponse)?;
        repo.pin(root_cid)
            .recursive()
            .await
            .map_err(|e| IpfsError::Embedded(e.to_string()))?;
        Ok(root_cid.to_string())
    }

    async fn dialable_multiaddrs(&self) -> Vec<String> {
        self.real_dialable_multiaddrs()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|addr| addr.to_string())
            .collect()
    }

    fn provider_name(&self) -> &'static str {
        "embedded"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oag-ipfs-embedded-test-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn pins_bytes_and_fetches_the_same_content_back() {
        let provider = EmbeddedIpfsProvider::new(temp_repo_path("roundtrip"), vec![])
            .await
            .unwrap();

        let cid = provider.pin(b"hello embedded ipfs").await.unwrap();
        let fetched = provider.fetch(&cid).await.unwrap();

        assert_eq!(fetched, b"hello embedded ipfs");
        assert_eq!(provider.provider_name(), "embedded");
    }

    #[tokio::test]
    async fn pins_a_directory_and_fetches_a_nested_file_by_path() {
        let provider = EmbeddedIpfsProvider::new(temp_repo_path("directory"), vec![])
            .await
            .unwrap();

        let root_cid = provider
            .pin_directory(&[
                ("index.html".to_string(), b"<html>hi</html>".to_vec()),
                ("css/style.css".to_string(), b"body { color: red; }".to_vec()),
            ])
            .await
            .unwrap();

        let index = provider.fetch(&format!("{root_cid}/index.html")).await.unwrap();
        assert_eq!(index, b"<html>hi</html>");

        let css = provider.fetch(&format!("{root_cid}/css/style.css")).await.unwrap();
        assert_eq!(css, b"body { color: red; }");
    }

    #[tokio::test]
    async fn listens_on_a_dialable_websocket_multiaddr() {
        let provider = EmbeddedIpfsProvider::new(
            temp_repo_path("listen"),
            vec!["/ip4/127.0.0.1/tcp/0/ws".parse().unwrap()],
        )
        .await
        .unwrap();

        let addrs = provider.real_dialable_multiaddrs().await.unwrap();

        assert!(
            addrs.iter().any(|a| {
                let s = a.to_string();
                s.contains("/ws") && s.contains("/p2p/")
            }),
            "expected a dialable /ws multiaddr with a /p2p/ peer-id suffix, got: {addrs:?}"
        );

        // The `PinningProvider` trait method (what `oag serve` actually
        // calls) must surface the same addresses as plain strings.
        let trait_addrs = PinningProvider::dialable_multiaddrs(&provider).await;
        assert_eq!(trait_addrs.len(), addrs.len());
    }
}
