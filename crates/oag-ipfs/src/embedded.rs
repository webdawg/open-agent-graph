use std::path::{Path, PathBuf};

use bytes::Bytes;
use futures::StreamExt;
use rust_ipfs::builder::DefaultIpfsBuilder;
use rust_ipfs::unixfs::ll::dir::builder::{BufferingTreeBuilder, TreeOptions};
use rust_ipfs::{Block, Ipfs, IpfsPath, Multiaddr, PeerId};

use crate::error::IpfsError;
use crate::provider::PinningProvider;

/// Name of the file under the IPFS repo directory holding this node's libp2p secret key.
const KEY_FILE: &str = "libp2p.key";

/// Load this node's libp2p secret key from `repo_path`, generating and persisting one on first
/// run.
///
/// `rust-ipfs`'s own builder has `with_keychain_identity`, which would do this against its
/// keystore; this keeps the key in the same directory as the rest of the node's state and in
/// the same 32-byte raw-secret form `oag_crypto::PeerIdentity` already uses
/// (`crates/oag-crypto/src/identity.rs`), so one convention covers both. Deliberately *not*
/// the OAG peer identity: those are two different roles, and sharing a key across them would
/// make the node's dial address an identity assertion about its OAG peer.
///
/// A file of the wrong length is an error rather than a silent regeneration. Losing the key
/// changes the node's dial address, which is the one thing this function exists to keep
/// stable; replacing it quietly would hide exactly the failure the operator needs to see.
fn load_or_create_keypair(repo_path: &Path) -> Result<Vec<u8>, IpfsError> {
    let key_path = repo_path.join(KEY_FILE);
    match std::fs::read(&key_path) {
        Ok(bytes) if bytes.len() == 32 => Ok(bytes),
        Ok(bytes) => Err(IpfsError::Embedded(format!(
            "{} must be 32 bytes, got {}; refusing to start with a different libp2p identity",
            key_path.display(),
            bytes.len()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use rand::rngs::OsRng;
            use rand::RngCore;
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            std::fs::write(&key_path, bytes).map_err(|e| {
                IpfsError::Embedded(format!("failed to persist {}: {e}", key_path.display()))
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| {
                        IpfsError::Embedded(format!(
                            "failed to restrict {} to owner-only: {e}",
                            key_path.display()
                        ))
                    })?;
            }
            Ok(bytes.to_vec())
        }
        Err(e) => Err(IpfsError::Embedded(format!(
            "failed to read {}: {e}",
            key_path.display()
        ))),
    }
}

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
/// Upper bound on a single content fetch.
///
/// Without it a fetch of a CID nobody can serve blocks indefinitely: measured directly, a fresh
/// node asking for a CID it does not hold and cannot reach had not returned after 10 seconds. Every
/// caller of `fetch` is a request path -- the crawler following a URL, a peer serving a transfer --
/// so an unservable CID would otherwise hold a task and a connection open for as long as the
/// requester waited. Same posture as the imagination carrier's `UPSTREAM_TIMEOUT`.
pub const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
        let keypair = load_or_create_keypair(&repo_path)?;
        // Not `DefaultIpfsBuilder::new()`: that generates a fresh keypair on every call, so
        // this node's dial address would change on each `oag serve` start and a browser
        // holding the old bootstrap page could not reach it until it reloaded.
        let ipfs = DefaultIpfsBuilder::with_keypair(keypair)
            .map_err(|e| IpfsError::Embedded(e.to_string()))?
            .set_path(&repo_path)
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

    /// This node's libp2p [`PeerId`] -- the identity half of every dial address it advertises.
    pub fn peer_id(&self) -> PeerId {
        self.ipfs.keypair().public().to_peer_id()
    }

    /// Shut the node down and release its repository lock.
    ///
    /// Dropping the handle is not enough: `rust-ipfs` keeps the repo in the struct, so a
    /// second provider on the same directory fails with "already being used by an IPFS
    /// instance" until the process exits. Needed for a clean in-process restart and for any
    /// test that starts two nodes against one directory.
    pub async fn shutdown(self) {
        self.ipfs.exit_daemon().await;
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

impl EmbeddedIpfsProvider {
    /// [`Self::fetch`] with an explicit bound, so a test can use one short enough to observe.
    ///
    /// The stream is wrapped as a whole rather than per chunk: a peer that trickles bytes slowly
    /// is not the hazard, a peer (or a CID) that never produces any is.
    pub async fn fetch_with_timeout(
        &self,
        cid: &str,
        timeout: std::time::Duration,
    ) -> Result<Vec<u8>, IpfsError> {
        tokio::time::timeout(timeout, self.fetch_unbounded(cid)).await.map_err(|_| {
            IpfsError::Embedded(format!("timed out after {}s fetching {cid}", timeout.as_secs()))
        })?
    }

    async fn fetch_unbounded(&self, cid: &str) -> Result<Vec<u8>, IpfsError> {
        let path: IpfsPath = cid.parse().map_err(|_| IpfsError::EmptyResponse)?;
        let mut stream = self.ipfs.unixfs().cat(path);
        let mut out = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| IpfsError::Embedded(e.to_string()))?;
            out.extend_from_slice(&chunk);
        }
        Ok(out)
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
        self.fetch_with_timeout(cid, FETCH_TIMEOUT).await
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

    /// A fetch of a CID nobody can serve must return an error, not block forever.
    ///
    /// Measured before the bound: a fresh node asking for a CID it does not hold and cannot reach
    /// had not returned after 10 seconds. Every caller of `fetch` is a request path, so an
    /// unservable CID would hold a task and a connection open indefinitely.
    #[tokio::test]
    async fn fetching_a_cid_nobody_can_serve_times_out_instead_of_blocking() {
        let minter_dir = std::env::temp_dir().join(format!(
            "oag-ipfs-fetch-timeout-minter-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&minter_dir).unwrap();
        let minter = EmbeddedIpfsProvider::new(minter_dir, vec![]).await.unwrap();
        let cid = minter.pin(b"mint a real cid, then leave").await.unwrap();
        minter.shutdown().await;

        // A different directory: this node holds neither the CID nor the block, and has no peers.
        let fresh_dir = std::env::temp_dir().join(format!(
            "oag-ipfs-fetch-timeout-fresh-{}",
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&fresh_dir).unwrap();
        let provider = EmbeddedIpfsProvider::new(fresh_dir, vec![]).await.unwrap();

        let started = std::time::Instant::now();
        let error = provider
            .fetch_with_timeout(&cid, std::time::Duration::from_millis(500))
            .await
            .expect_err("a CID nobody can serve must not fetch successfully");
        assert!(
            error.to_string().contains("timed out"),
            "expected a timeout, got {error}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the bound must be honoured, took {:?}",
            started.elapsed()
        );
        provider.shutdown().await;
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
    async fn the_libp2p_identity_survives_a_restart() {
        // `IpfsBuilder::new()` generates a fresh keypair every call, so before this the
        // node's dial address changed on every `oag serve` start and a browser holding the
        // old bootstrap page could not reach it until it reloaded.
        let repo = temp_repo_path("identity");
        let first = EmbeddedIpfsProvider::new(repo.clone(), vec![]).await.unwrap();
        let first_id = first.peer_id();
        assert!(repo.join("libp2p.key").exists(), "the key must be persisted");
        first.shutdown().await;

        let second = EmbeddedIpfsProvider::new(repo.clone(), vec![]).await.unwrap();
        assert_eq!(
            second.peer_id(),
            first_id,
            "a restart must not change the node's dial identity"
        );
    }

    #[tokio::test]
    async fn a_corrupt_identity_file_is_refused_rather_than_regenerated() {
        // Losing the key changes the node's address, which is the one thing persistence
        // exists to keep stable. Regenerating quietly would hide exactly that failure.
        let repo = temp_repo_path("identity-corrupt");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("libp2p.key"), b"too short").unwrap();

        // Matched rather than `unwrap_err`, which would require `EmbeddedIpfsProvider: Debug`.
        let error = match EmbeddedIpfsProvider::new(repo.clone(), vec![]).await {
            Ok(_) => panic!("a corrupt key file must not start"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(message.contains("must be 32 bytes"), "unexpected error: {message}");

        // The file is left for the operator to fix; the node does not overwrite it.
        assert_eq!(std::fs::read(repo.join("libp2p.key")).unwrap(), b"too short");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_persisted_identity_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let repo = temp_repo_path("identity-permissions");
        let _provider = EmbeddedIpfsProvider::new(repo.clone(), vec![]).await.unwrap();
        let mode = std::fs::metadata(repo.join("libp2p.key")).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the libp2p secret must not be group- or world-readable, got {mode:o}"
        );
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
