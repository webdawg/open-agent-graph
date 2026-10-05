# Configuration

**Source**: `crates/oag-cli/src/config.rs`.

`config.toml`, resolved via `figment` (file values, overridable by `OAG_<SECTION>_<KEY>`
environment variables). Every section has a safe default — an empty or absent `config.toml` is a
fully functional single-peer, no-external-service configuration. `oag doctor --config` validates a
config file through the *exact* resolver `oag serve` itself uses.

## Sections

| Section | Covers | Notable defaults |
|---|---|---|
| `[data]` | Data directory | `./data` |
| `[server]` | Listen address, MCP enable | `127.0.0.1:7443` — **loopback-only by default**, not `0.0.0.0`; an operator must explicitly opt into binding a public interface |
| `[search]` | Semantic search / embeddings provider | Disabled (`DisabledProvider`) |
| `[mcp]` | MCP mount toggle | Enabled, mounted at `/mcp` |
| `[network]` | Bootstrap peers, sync interval | No bootstrap peers — replication is passive until `oag peer add` or a peer syncs in |
| `[federation]` | `open` or `allowlist` | `open` |
| `[crawler]` | SSRF defaults, LLM extraction config | Private-network crawling off; LLM extraction off |
| `[reticulum]` | Optional mesh transport | Disabled |

## Why `127.0.0.1` and not `0.0.0.0` by default

Checked specifically, not assumed, during a security-focused audit pass: a secure-by-default
listen address matters as much as any of the explicit resource-exhaustion fixes elsewhere in this
specification, and this one was already correct — confirmed by reading `default_listen()` directly
rather than inferring it from documentation.

## Tests

`crates/oag-cli/src/commands.rs` (`doctor_passes_with_a_valid_config_file`,
`doctor_passes_with_no_config_specified`, `doctor_fails_with_a_malformed_config_file`).
