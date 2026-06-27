# AOL2 Anchor Crate

This crate is the AOL2 anchor-layer integration boundary. Its job is to talk to Kaspa cleanly and keep Layer 1 concerns separate from overlay transport, truth validation, and desktop orchestration.

## Scope

The anchor crate is responsible for:

- live Kaspa node connectivity over wRPC;
- collecting incoming Kaspa block hashes for anchor-following workflows;
- creating and importing Kaspa wallets through the Rust wallet core;
- sending Kaspa transactions with or without payload bytes;
- consuming simply-kaspa-indexer as a companion index/query surface;
- exposing a stable, modular API to the rest of the workspace.

The anchor crate is not responsible for:

- peer discovery or session routing;
- truth-engine proof validation;
- desktop UI state;
- AOL2 checkpoint policy decisions outside the anchoring boundary.

## Module Layout

- `src/node.rs`: live Kaspa wRPC node client, block-DAG inspection, and incoming block-hash polling stream.
- `src/wallet.rs`: wallet-core-backed wallet lifecycle, address generation, and transaction sending.
- `src/indexer.rs`: simply-kaspa-indexer companion client for health and metrics snapshots.
- `src/service.rs`: high-level facade that composes node, wallet, and indexer services.
- `src/config.rs`: deployment configuration and network selection types.
- `src/types.rs`: crate-level request and response types.
- `src/error.rs`: anchor-specific error surface.
- `tests/config_contract.rs`: deterministic configuration and network-contract coverage.
- `tests/wallet_lifecycle.rs`: isolated wallet create/open/address/import/archive integration coverage.
- `tests/live_smoke.rs`: ignored-by-default live smoke tests for real node polling, indexer verification, and funded send execution.
- `indexer/simply-kaspa-indexer`: bundled local checkout of the simply-kaspa-indexer upstream used by live indexer tests.

## Implemented Now

- live Kaspa wRPC client bootstrap with resolver-or-URL configuration;
- `get_block_dag_info` access and polling for incoming block hashes via `get_blocks`;
- wallet creation with generated 24-word mnemonic;
- wallet import from mnemonic;
- wallet import from exported wallet bytes;
- wallet open flow;
- receive-address generation for an account;
- transaction sending with optional payload bytes;
- simply-kaspa-indexer health and metrics fetches;
- anchor facade that can seed block-following from indexer state when available.

## Current Constraints

- incoming block hashes are implemented as polling over `get_blocks`, not as a first-pass notification subscriber;
- simply-kaspa-indexer is currently consumed through its public REST health and metrics surface, because that is the stable API it exposes today;
- the indexer is used as a checkpoint and observability companion, not as the primary live block transport;
- wallet send helpers currently target the standard account send path and assume the caller manages the intended AOL2 payload semantics.

## Roadmap

### Immediate

- add anchor-layer tests that exercise configuration parsing and pure helper logic;
- wire the anchor crate into the desktop and sidecar orchestration path;
- persist and restore block-follow cursors instead of requiring the caller to supply them;
- expose fee-rate configuration and estimation on the wallet send path.

### Next

- add notification-based block following on top of Kaspa RPC subscriptions;
- expand indexer integration if simply-kaspa-indexer exposes richer stable endpoints or if direct database integration becomes a requirement;
- add checkpoint, delegation-record, and fraud-proof publication helpers specific to the chosen AOL2 deployment profile;
- bind desktop identity and anchor wallet/account material into one explicit anchor-identity flow.

### Later

- define the concrete AOL2 Kaspa deployment profile document that fixes finality handling, anchoring payload conventions, and checkpoint publication format;
- add end-to-end tests against a controlled Kaspa node and indexer environment;
- surface anchor operations through Tauri commands once the API shape stabilizes.

## Requirements Checklist

- [x] Clear separation between node, wallet, and indexer concerns.
- [x] Live Kaspa node integration rather than placeholder structs.
- [x] Wallet create and import support.
- [x] Transaction send support with optional payload bytes.
- [x] simply-kaspa-indexer companion integration.
- [ ] Desktop wiring.
- [ ] AOL2-specific checkpoint publication flow.
- [ ] Identity binding between anchor wallet/account material and the runtime p2p identity.

## Validation Expectations

Whenever this crate changes, the minimum validation loop should be:

1. `cargo check -p anchor`
2. targeted tests for any new pure helper or serialization logic
3. if node or wallet logic changed, a live smoke test against a Kaspa node URL and, when relevant, a simply-kaspa-indexer instance

## Test Harness

Default integration coverage runs without external services:

- `cargo test -p anchor --tests`

`cargo test -p anchor --tests` runs the stable integration binaries, including `wallet_lifecycle`, but it does not run `tests/live_smoke.rs` because those cases are marked `#[ignore]` and require explicit selection.

If you want the crate to prompt for live-test settings and then run the stable plus ignored live slices in order, use:

- `cargo run -p anchor --bin anchor-live-smoke`

Live smoke tests are integration-gated and ignored by default:

- `ANCHOR_LIVE_TEST=1 cargo test -p anchor --test live_smoke live_node_block_polling_via_real_kaspa_node -- --ignored --nocapture`
- `ANCHOR_LIVE_TEST=1 cargo test -p anchor --test live_smoke live_indexer_health_and_anchor_best_cursor_against_real_simply_kaspa_indexer -- --ignored --nocapture`
- `ANCHOR_LIVE_TEST=1 cargo test -p anchor --test live_smoke live_funded_send_flow_against_real_kaspa_account -- --ignored --nocapture`

The live-test harness defaults to `testnet-10`. The interactive runner asks which network to use, asks whether you have a specific wRPC node endpoint or want to fall back to the public resolver, and keeps one persistent test wallet per network under `~/.mssq-anchor-tests/<network>` unless `ANCHOR_TEST_WALLET_STORAGE_DIR` is set explicitly.

The indexer smoke now uses the bundled checkout at `AOL2/anchor/indexer/simply-kaspa-indexer` by default and starts it on `http://127.0.0.1:8500` when `ANCHOR_TEST_INDEXER_URL` is not set. For the bundled path it needs a PostgreSQL backend; provide `ANCHOR_TEST_INDEXER_DATABASE_URL` if you already have one, or let the test spin up a local `postgres:16-alpine` container when Docker is available. When you use `cargo run -p anchor --bin anchor-live-smoke` and the bundled indexer path needs Docker but Docker is missing, the runner now asks whether it should install Docker at that moment.

The live send smoke now auto-creates a reusable local test wallet, persists its state under `ANCHOR_TEST_WALLET_STORAGE_DIR` or the default network-specific home directory, derives a fresh internal destination address up front, and then waits for the wallet account itself to become spendable for the configured send. It no longer asks for an `Enter` confirmation, and it no longer treats the originally printed funding address as the wallet's total balance source. The harness now keeps the wallet on the same resolver-backed or explicit wRPC client instance for the whole run, waits for that live client to report its connected endpoint and peer metadata over wallet-core and `RpcCtl` events, and refreshes account balance over that same live wallet connection. The funding target is still reported in Kaspa using the send amount plus a fee floor: `0.00002 KAS` for plain transactions by default, or `0.25 KAS` when a payload is present, with `ANCHOR_TEST_SEND_FEE_FLOOR_SOMPI` available as an override. While waiting, the harness now subscribes to wallet-core events, reacts to account activation, sync-state, backend-status, and balance updates, and only re-runs spendability estimation when those events indicate the account state changed. `ANCHOR_TEST_FUNDING_POLL_INTERVAL_SECS` remains available, but it now controls the idle heartbeat interval for status logging while the harness is waiting on wallet events rather than a balance polling cadence. Optional tuning env vars are `ANCHOR_TEST_SEND_AMOUNT_SOMPI`, `ANCHOR_TEST_SEND_PRIORITY_FEE_SOMPI`, `ANCHOR_TEST_SEND_FEE_FLOOR_SOMPI`, `ANCHOR_TEST_FUNDING_POLL_INTERVAL_SECS`, `ANCHOR_TEST_ACCOUNT_REFRESH_ATTEMPT_TIMEOUT_SECS`, and `ANCHOR_TEST_SEND_TIMEOUT_SECS`. If `ANCHOR_TEST_SEND_TIMEOUT_SECS` is unset, the live send path does not impose a timeout.

Live send tests still target real funds and should be run deliberately. If you override wallet-core local storage with `ANCHOR_TEST_WALLET_STORAGE_DIR`, run the live-smoke binary with `--test-threads=1` to avoid process-global storage-folder races inside `kaspa-wallet-core`. Both the node and wallet smokes now use the same resolver-backed `KaspaRpcClient` connection pattern as the official Kaspa examples. If the public resolver or its returned testnet endpoints are slow or unhealthy from your host, prefer an explicit `ANCHOR_TEST_NODE_URL=wrpc://host:17110` so the smokes can bypass resolver instability.