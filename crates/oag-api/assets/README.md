# `ipfs-site-bundle.js`

Vendored at development time, not built as part of `cargo build` — this project's whole posture
is "single self-contained binary, no required external service," and pulling a bundler/Node
toolchain into the Rust build would contradict that. `crates/oag-api/src/ipfs_site.rs` embeds the
built `ipfs-site-bundle.js` via `include_bytes!` and serves it as-is.

To rebuild after editing `ipfs-site-bundle.src.js`:

```bash
mkdir -p /tmp/ipfs-bundle-build && cd /tmp/ipfs-bundle-build
npm init -y
npm install --save-exact \
  helia@7.1.15 \
  @helia/bitswap@4.0.19 \
  @helia/unixfs@8.0.7 \
  @helia/libp2p \
  @libp2p/websockets@10.1.22 \
  @libp2p/identify \
  @libp2p/kad-dht \
  @libp2p/ping \
  @multiformats/multiaddr \
  multiformats@14.0.5 \
  esbuild@0.28.2
cp <repo>/crates/oag-api/assets/ipfs-site-bundle.src.js .
npx esbuild ipfs-site-bundle.src.js --bundle --format=esm --minify --target=es2022 \
  --outfile=ipfs-site-bundle.js
cp ipfs-site-bundle.js <repo>/crates/oag-api/assets/ipfs-site-bundle.js
```

The explicitly-versioned packages were pinned exactly (no ranges), each at least two weeks old as
of when they were picked (2026-10-15), same version-pinning discipline as everywhere else in this
project. The rest were left unpinned to whatever version the dependency tree resolves — only pin
them explicitly if a rebuild needs to reproduce an exact past bundle byte-for-byte.

## Why this specific API shape

See the comment block at the top of `ipfs-site-bundle.src.js` for the full reasoning; this is a
summary, confirmed correct by live end-to-end testing (a real headless browser, a real embedded
`rust-ipfs` node on the Rust side, zero HTTP gateway/proxy anywhere):

- The Helia node is built by composing `createHeliaLight()` + `withLibp2p()` + `withBitswap()`,
  **never** `withHTTP()` — a structural omission, not a config flag, so the HTTP-trustless-gateway
  block broker code never gets composed into this node (and tree-shakes out of the bundle).
- `withLibp2p`'s `services` option is fully overridden (its default includes
  `delegatedPeerRouting`/`delegatedContentRouting`, both plain HTTPS calls to a public routing
  API, plus `autoTLS`/`http`/`upnp`/relay-server services irrelevant to a browser leaf client) down
  to `identify` + `dht` + `ping` (`ping` is a hard runtime dependency of `dht`, confirmed live).
  `dht` stays: Kademlia peer/content routing is peer-to-peer, not an HTTP gateway, and is this
  node's only discovery path when no direct dial multiaddr is configured.
- The browser's **default connection gater** refuses to dial both insecure (plain `ws`) and
  private/local addresses — confirmed live, this silently no-ops the dial to our own embedded node
  (plain `ws` on a local IP) with zero network activity. Overridden with
  `connectionGater: { denyDialMultiaddr: () => false }` — a deliberate, scoped opt-in (this bundle
  only ever dials the one address the server handed it), not a general "trust everything" change.
- **No `@helia/verified-fetch`.** It does its own public-network provider/content-routing lookup
  (DHT/IPNI queries against the whole public network) before ever trying bitswap — confirmed live,
  this stalls for many seconds against a direct, already-connected peer that was never going to
  show up in a public lookup in the first place. `@helia/unixfs`'s `ls`/`stat`/`cat` go straight to
  bitswap over connections this node already has, with no separate routing step.
- `fs.ls()`'s own yielded entries carry only `{cid, name, path, size}` — no `type` field, despite
  `ipfs-unixfs-exporter`'s own type declarations suggesting otherwise (confirmed live; that shape
  describes its own `exporter()`/`stat()` results, not what `ls()` re-yields from `entries()`).
  Distinguishing a file from a subdirectory during the directory walk uses `fs.stat(cid)` instead,
  which reliably reports `type`.
- `multiaddr()` from `@multiformats/multiaddr` must wrap the plain multiaddr string the server
  injects into the page before `libp2p.dial()` will accept it — passing the string directly fails
  deep inside libp2p with an unhelpful minified error (confirmed live).
