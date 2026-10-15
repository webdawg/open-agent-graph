// OAG static-site-over-IPFS bootstrap bundle (spec/29-ipfs-pinning.md,
// "static page hosting" -- USER_INPUT_RECORD.md Entry 19/this session).
//
// Builds a Helia node by composing `createHeliaLight` + `withLibp2p` +
// `withBitswap` and DELIBERATELY NEVER calling `withHTTP` (helia's own
// HTTP-trustless-gateway block broker) or including any HTTP delegated-
// routing service -- this is a structural guarantee, not a config flag:
// the HTTP gateway code isn't composed into this node at all, so there is
// nothing for it to fall back to. Content retrieval goes exclusively over
// bitswap/libp2p to peers this node actually connects to, over its own
// peer/content routing (mDNS, bootstrap dial, and the Kademlia DHT -- all
// peer-to-peer, never an HTTP call to a third-party routing/gateway
// service). This is the literal mechanism behind "ipfs mode should not use
// an ipfs proxy."
//
// `oag serve` itself never fetches from IPFS; it only ever serves this
// bundle plus a tiny bootstrap page naming a CID. Everything below runs in
// the browser and does 100% of the real content retrieval.
import { createHeliaLight } from 'helia'
import { withLibp2p } from '@helia/libp2p'
import { withBitswap } from '@helia/bitswap'
import { unixfs } from '@helia/unixfs'
import { CID } from 'multiformats/cid'
import { multiaddr as toMultiaddr } from '@multiformats/multiaddr'
import { webSockets } from '@libp2p/websockets'
import { identify } from '@libp2p/identify'
import { kadDHT } from '@libp2p/kad-dht'
import { ping } from '@libp2p/ping'

async function buildGatewayFreeHelia(directDialMultiaddr) {
  const light = await createHeliaLight()
  const withLp2p = withLibp2p(light, {
    addresses: { listen: [] }, // a browser leaf client never listens, only dials out
    transports: [webSockets()], // the only transport a browser can dial directly
    // The browser's default connection gater refuses to dial BOTH insecure
    // (plain `ws`, non-TLS) addresses and private/local ones -- confirmed
    // live: our own embedded node's address is both (plain `ws` on a local
    // IP), so the default gate rejected it outright with no network
    // activity at all. That default is the right call for a general-
    // purpose dapp talking to arbitrary IPFS peers; it's wrong here, where
    // the server just told this exact browser to dial this exact,
    // same-deployment node. Overriding it is a deliberate, scoped opt-in,
    // not a general "trust everything" change -- this bundle only ever
    // dials the one address the server handed it.
    connectionGater: { denyDialMultiaddr: () => false },
    // Deliberately a full replacement of the default `services` object
    // (not a merge) -- this drops every HTTP-touching default service
    // (delegatedPeerRouting/delegatedContentRouting, which are plain
    // HTTPS calls to a public routing API; autoTLS; the libp2p `http`
    // protocol service; upnp/relay-server/dcutr, all server-side
    // concerns a leaf browser client has no use for). `dht` stays: it is
    // peer-to-peer Kademlia routing, not an HTTP gateway, and is this
    // node's only way to discover providers when no direct dial target
    // is configured below. `ping` is a hard dependency of `dht` (confirmed
    // live: omitting it, the kad-dht service fails to start with "required
    // capability @libp2p/ping... not provided"), not an HTTP concern
    // either way -- it's a basic libp2p liveness-check protocol.
    services: { identify: identify(), dht: kadDHT(), ping: ping() },
  })
  const helia = withBitswap(withLp2p)
  await helia.start()

  if (directDialMultiaddr) {
    try {
      // `dial` needs a real `Multiaddr` object, not the plain string the
      // server hands us -- confirmed live: passing the string directly
      // fails deep inside libp2p with an unhelpful minified error.
      await helia.libp2p.dial(toMultiaddr(directDialMultiaddr))
    } catch (err) {
      renderError(`could not dial the configured local IPFS node (${directDialMultiaddr}): ${err.message}`)
    }
  }
  return helia
}

function renderError(message) {
  const root = document.getElementById('oag-ipfs-root')
  root.textContent = `oag ipfs site: ${message}`
  root.style.cssText = 'font-family: monospace; color: #b00020; padding: 1rem;'
}

/**
 * Walk every file under `rootCid` via `@helia/unixfs`'s `ls`/`cat` (bitswap-
 * only, see above -- no `@helia/verified-fetch`: it does its own public-
 * network provider/content-routing lookup before falling back to bitswap,
 * which stalls for many seconds against a direct, already-connected peer
 * like ours that was never going to show up in a public DHT/IPNI lookup in
 * the first place; `fs.cat(cid)` goes straight to bitswap over connections
 * this node already has), build blob: URLs for each, then rewrite
 * index.html's relative src/href references to those blob URLs before
 * injecting it into the page. This is what makes a *whole site* (CSS/JS/
 * images), not just one page, load with zero HTTP involvement beyond this
 * bootstrap page and this bundle itself -- a plain <link href="style.css">
 * would otherwise resolve against oag serve's own HTTP origin and silently
 * reintroduce the HTTP path for every asset but the first.
 */
const MIME_TYPES = {
  html: 'text/html',
  css: 'text/css',
  js: 'application/javascript',
  json: 'application/json',
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  svg: 'image/svg+xml',
  gif: 'image/gif',
}

function guessMimeType(path) {
  const ext = path.split('.').pop()?.toLowerCase()
  return MIME_TYPES[ext] ?? 'application/octet-stream'
}

/** Concatenate a UnixFS file's byte chunks into one `Blob`. */
async function catToBlob(fs, cid, mimeType) {
  const chunks = []
  for await (const chunk of fs.cat(cid)) {
    chunks.push(chunk)
  }
  return new Blob(chunks, { type: mimeType })
}

async function loadSite(rootCid, directDialMultiaddr) {
  const helia = await buildGatewayFreeHelia(directDialMultiaddr)
  const fs = unixfs(helia)

  const cid = CID.parse(rootCid)
  // `{ relative_path, cid }` pairs, collected directly from the UnixFS
  // directory listing -- each entry already carries its own exact CID, so
  // fetching below goes straight to `fs.cat`, never through a separate
  // path-resolution step.
  const entries = []
  for await (const entry of fs.ls(cid)) {
    await collectEntries(fs, entry, '', entries)
  }

  const blobUrls = new Map()
  let indexBlob = null
  for (const entry of entries) {
    const blob = await catToBlob(fs, entry.cid, guessMimeType(entry.path))
    if (entry.path === 'index.html') {
      indexBlob = blob
    } else {
      blobUrls.set(entry.path, URL.createObjectURL(blob))
    }
  }

  if (!indexBlob) {
    renderError(`index.html not found under ${rootCid}`)
    return
  }
  const html = await indexBlob.text()
  const doc = new DOMParser().parseFromString(html, 'text/html')
  for (const el of doc.querySelectorAll('[src], [href]')) {
    const attr = el.hasAttribute('src') ? 'src' : 'href'
    const value = el.getAttribute(attr)
    // Only rewrite a reference that resolves to a file we actually pinned
    // under this same directory -- an absolute URL or something outside
    // the pinned tree (e.g. an external CDN script) is left alone.
    const relative = value.replace(/^\.?\/+/, '')
    if (blobUrls.has(relative)) {
      el.setAttribute(attr, blobUrls.get(relative))
    }
  }

  document.open()
  document.write(doc.documentElement.outerHTML)
  document.close()
}

async function collectEntries(fs, entry, prefix, out) {
  const relative = prefix ? `${prefix}/${entry.name}` : entry.name
  // `fs.ls()`'s own yielded entries carry only `{cid, name, path, size}` --
  // no `type` field, confirmed live (despite `ipfs-unixfs-exporter`'s own
  // type declarations suggesting otherwise; that shape applies to its own
  // `exporter()`/`stat()` results, not to what `ls()` re-yields from
  // `entries()`). `fs.stat(cid)` re-runs the exporter directly on this
  // entry's own CID and reliably reports `type`.
  const info = await fs.stat(entry.cid)
  if (info.type === 'directory') {
    for await (const child of fs.ls(entry.cid)) {
      await collectEntries(fs, child, relative, out)
    }
  } else {
    out.push({ path: relative, cid: entry.cid })
  }
}

const cid = window.__OAG_IPFS_CID__
const multiaddr = window.__OAG_IPFS_MULTIADDR__ || null
if (!cid) {
  renderError('no CID configured on this page')
} else {
  loadSite(cid, multiaddr).catch((err) => renderError(err.message))
}
