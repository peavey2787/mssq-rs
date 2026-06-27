mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anchor::{CreateWalletRequest, KaspaAnchorService, KaspaIndexerClient, KaspaIndexerConfig, KaspaNodeClient, KaspaWalletBalanceSnapshot, KaspaWalletConfig, KaspaWalletEvent, KaspaWalletService, OpenWalletRequest, WalletStorageMode};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, timeout};

const LIVE_SEND_STATE_FILE: &str = "anchor-live-send-wallet.json";
const LIVE_SEND_TITLE: &str = "Anchor Live Smoke";
const LIVE_SEND_ACCOUNT_NAME: &str = "live-smoke-account";
const DEFAULT_SEND_AMOUNT_SOMPI: u64 = 10_000;
const DEFAULT_MINIMUM_RELAY_FEE_SOMPI: u64 = 2_000;
const DEFAULT_PAYLOAD_FEE_FLOOR_SOMPI: u64 = 25_000_000;
const DEFAULT_FUNDING_POLL_INTERVAL_SECS: u64 = 3;
const SOMPI_PER_KASPA: u64 = 100_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LiveSendWalletState {
    wallet_secret: String,
    filename: String,
    funding_address: String,
}

struct LiveSendWallet {
    account_id: anchor::AccountId,
    state: LiveSendWalletState,
}

#[derive(Clone, Debug)]
struct LiveBackendSelection {
    wallet_config: KaspaWalletConfig,
    description: String,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ANCHOR_LIVE_TEST=1 and live Kaspa network access"]
async fn live_node_block_polling_via_real_kaspa_node() {
    common::require_live_enabled("live_node_block_polling_via_real_kaspa_node");

    let client = KaspaNodeClient::new(common::node_config()).expect("should construct live node client");

    let dag = timeout(Duration::from_secs(30), client.block_dag_info())
        .await
        .expect("block DAG info request should complete before timeout")
        .expect("block DAG info request should succeed against live node");

    assert!(!dag.tip_hashes.is_empty(), "live node should report at least one tip hash");

    let cursor = timeout(Duration::from_secs(30), client.recommended_cursor())
        .await
        .expect("recommended cursor request should complete before timeout")
        .expect("recommended cursor request should succeed against live node");

    assert!(cursor.low_hash.is_some(), "recommended cursor should contain a low hash");

    let batch = timeout(Duration::from_secs(30), client.get_block_hashes_since(&cursor))
        .await
        .expect("block hash polling request should complete before timeout")
        .expect("block hash polling should succeed against live node");

    assert!(batch.low_hash.is_some(), "poll result should retain its source cursor");
    assert!(batch.next_cursor.is_some(), "poll result should produce a next cursor");

    let mut stream = client.stream_incoming_block_hashes(cursor);
    let streamed = timeout(Duration::from_secs(35), stream.next())
        .await
        .expect("stream should yield its first polling result before timeout")
        .expect("stream should yield a polling item")
        .expect("streamed block polling should succeed against live node");

    assert!(streamed.low_hash.is_some(), "streamed poll result should retain its cursor context");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ANCHOR_LIVE_TEST=1 and the bundled simply-kaspa-indexer checkout plus either Docker or ANCHOR_TEST_INDEXER_DATABASE_URL"]
async fn live_indexer_health_and_anchor_best_cursor_against_real_simply_kaspa_indexer() {
    common::require_live_enabled("live_indexer_health_and_anchor_best_cursor_against_real_simply_kaspa_indexer");

    let local_indexer = common::ensure_live_indexer();
    let indexer_url = local_indexer.base_url().to_string();
    let indexer = KaspaIndexerClient::new(KaspaIndexerConfig {
        base_url: indexer_url.clone(),
    });

    let health = timeout(Duration::from_secs(20), indexer.health())
        .await
        .expect("indexer health request should complete before timeout")
        .expect("indexer health request should succeed against live indexer");

    assert!(health.payload.is_object(), "indexer health payload should be JSON object data");
    assert!(
        health.payload.get("status").is_some() || health.payload.get("indexer").is_some(),
        "indexer health payload should expose health status fields"
    );

    let snapshot = timeout(Duration::from_secs(20), indexer.metrics())
        .await
        .expect("indexer metrics request should complete before timeout")
        .expect("indexer metrics request should succeed against live indexer");

    assert!(snapshot.metrics.is_object(), "indexer metrics payload should be JSON object data");
    assert!(
        snapshot.metrics.get("components").is_some() || snapshot.metrics.get("checkpoint").is_some(),
        "indexer metrics payload should expose component or checkpoint state"
    );

    let service = KaspaAnchorService::new(common::anchor_config_with_indexer(WalletStorageMode::Resident, indexer_url))
        .expect("should construct anchor service with live indexer config");

    let cursor = timeout(Duration::from_secs(30), service.best_cursor())
        .await
        .expect("anchor best-cursor request should complete before timeout")
        .expect("anchor best-cursor request should succeed with live indexer");

    assert!(cursor.low_hash.is_some(), "anchor best cursor should resolve to a hash");

    let batch = timeout(Duration::from_secs(30), service.poll_incoming_block_hashes(Some(cursor)))
        .await
        .expect("anchor poll request should complete before timeout")
        .expect("anchor poll request should succeed with live indexer and node");

    assert!(batch.next_cursor.is_some(), "anchor poll should produce a next cursor");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires ANCHOR_LIVE_TEST=1 and manual funding of the generated reusable live test wallet"]
async fn live_funded_send_flow_against_real_kaspa_account() {
    common::require_live_enabled("live_funded_send_flow_against_real_kaspa_account");

    let storage_dir = install_live_send_storage_dir();
    let backend_selection = resolve_live_wallet_backend().await;

    let service = KaspaWalletService::new(backend_selection.wallet_config)
        .expect("should construct live-send wallet service");
    let amount_sompi = optional_u64_env("ANCHOR_TEST_SEND_AMOUNT_SOMPI").unwrap_or(DEFAULT_SEND_AMOUNT_SOMPI);
    let priority_fee_sompi = optional_u64_env("ANCHOR_TEST_SEND_PRIORITY_FEE_SOMPI").unwrap_or(0);
    let payload = common::optional_hex_env("ANCHOR_TEST_SEND_PAYLOAD_HEX");
    let live_wallet = ensure_live_send_wallet(&service, &storage_dir).await;

    let destination = service
        .create_receive_address(live_wallet.account_id.clone())
        .await
        .expect("should derive a fresh internal destination address for the live send smoke");

    println!(
        "wallet refresh backend selected for {}: {}",
        live_wallet.state.filename,
        backend_selection.description
    );

    let estimated_summary = wait_for_live_wallet_funding(
        &service,
        &live_wallet,
        &destination.address,
        amount_sompi,
        priority_fee_sompi,
        payload.as_deref(),
    )
    .await;

    println!(
        "wallet spendability check passed for {}; estimated fees {} KAS, estimated total {} KAS; submitting live send",
        destination.address,
        format_kaspa_amount(estimated_summary.aggregated_fees),
        format_kaspa_amount(amount_sompi.saturating_add(estimated_summary.aggregated_fees))
    );

    let send_future = service.send_to_address(
        live_wallet.account_id.clone(),
        live_wallet.state.wallet_secret.clone(),
        None,
        &destination.address,
        amount_sompi,
        priority_fee_sompi,
        payload.clone(),
    );

    let result = if let Some(timeout_secs) = optional_u64_env("ANCHOR_TEST_SEND_TIMEOUT_SECS") {
        timeout(Duration::from_secs(timeout_secs), send_future)
            .await
            .unwrap_or_else(|_| panic!("funded send should complete before the configured {timeout_secs}s timeout"))
            .expect("funded send should succeed against live Kaspa network")
    } else {
        send_future
            .await
            .expect("funded send should succeed against live Kaspa network")
    };

    assert!(
        !result.transaction_ids.is_empty(),
        "funded send should return at least one submitted transaction id"
    );

    println!(
        "live send submitted to {} with tx ids {:?}; amount {} KAS, aggregated fees {} KAS, total {} KAS",
        destination.address,
        result.transaction_ids,
        format_kaspa_amount(amount_sompi),
        format_kaspa_amount(result.generator_summary.aggregated_fees),
        format_kaspa_amount(amount_sompi.saturating_add(result.generator_summary.aggregated_fees))
    );
}

fn live_send_storage_dir() -> PathBuf {
    common::persistent_live_wallet_storage_dir()
}

fn install_live_send_storage_dir() -> PathBuf {
    let storage_dir = live_send_storage_dir();
    fs::create_dir_all(&storage_dir)
        .unwrap_or_else(|err| panic!("should create live-send wallet storage directory {}: {err}", storage_dir.display()));
    common::install_storage_dir(&storage_dir);
    storage_dir
}

fn live_send_state_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join(LIVE_SEND_STATE_FILE)
}

fn optional_u64_env(name: &str) -> Option<u64> {
    common::env_var(name).map(|value| {
        value
            .parse::<u64>()
            .unwrap_or_else(|err| panic!("{name} should parse as an unsigned integer: {err}"))
    })
}

fn load_live_send_wallet_state(state_path: &Path) -> Option<LiveSendWalletState> {
    let bytes = fs::read(state_path).ok()?;
    serde_json::from_slice(&bytes)
        .map_err(|err| panic!("should parse live-send wallet state {}: {err}", state_path.display()))
        .ok()
}

fn write_live_send_wallet_state(state_path: &Path, state: &LiveSendWalletState) {
    let bytes = serde_json::to_vec_pretty(state)
        .unwrap_or_else(|err| panic!("should serialize live-send wallet state: {err}"));

    fs::write(state_path, bytes)
        .unwrap_or_else(|err| panic!("should write live-send wallet state {}: {err}", state_path.display()));
}

fn generate_wallet_secret() -> String {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the unix epoch")
        .as_nanos();
    let digest = blake3::hash(format!("anchor-live-send:{}:{unique}", std::process::id()).as_bytes());
    format!("anchor-live-send-{}", digest.to_hex())
}

fn generate_wallet_filename() -> String {
    let unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after the unix epoch")
        .as_secs();
    format!("anchor-live-send-{unix_secs}")
}

async fn open_live_send_wallet(service: &KaspaWalletService, state: &LiveSendWalletState) -> Option<anchor::AccountId> {
    let opened = timeout(
        Duration::from_secs(30),
        service.open_wallet(OpenWalletRequest {
            wallet_secret: state.wallet_secret.clone(),
            filename: Some(state.filename.clone()),
            account_descriptors: true,
            include_legacy_accounts: true,
        }),
    )
    .await
    .unwrap_or_else(|_| panic!("live-send wallet open should complete before timeout"))
    .unwrap_or_else(|err| panic!("stored live-send wallet should open successfully: {err}"))?;

    opened.first().map(|descriptor| descriptor.account_id.clone())
}

async fn create_live_send_wallet(service: &KaspaWalletService, state_path: &Path) -> LiveSendWallet {
    let wallet_secret = generate_wallet_secret();
    let filename = generate_wallet_filename();
    let created = service
        .create_wallet(CreateWalletRequest {
            wallet_secret: wallet_secret.clone(),
            title: Some(LIVE_SEND_TITLE.to_string()),
            filename: Some(filename.clone()),
            overwrite: false,
            account_name: Some(LIVE_SEND_ACCOUNT_NAME.to_string()),
            payment_secret: None,
        })
        .await
        .expect("should create a reusable live-send wallet");

    let funding_address = service
        .create_receive_address(created.account_descriptor.account_id.clone())
        .await
        .expect("should derive the funding address for the live-send wallet")
        .address;

    let state = LiveSendWalletState {
        wallet_secret,
        filename,
        funding_address,
    };
    write_live_send_wallet_state(state_path, &state);

    println!(
        "created reusable live-smoke wallet {} at {}",
        state.filename,
        state_path.display()
    );

    LiveSendWallet {
        account_id: created.account_descriptor.account_id,
        state,
    }
}

async fn ensure_live_send_wallet(service: &KaspaWalletService, storage_dir: &Path) -> LiveSendWallet {
    let state_path = live_send_state_path(storage_dir);

    if let Some(state) = load_live_send_wallet_state(&state_path) {
        if let Some(account_id) = open_live_send_wallet(service, &state).await {
            println!(
                "reusing live-smoke wallet {} from {}",
                state.filename,
                state_path.display()
            );
            return LiveSendWallet { account_id, state };
        }
    }

    create_live_send_wallet(service, &state_path).await
}

fn default_fee_floor_sompi(payload: Option<&[u8]>) -> u64 {
    match payload {
        Some(bytes) if !bytes.is_empty() => DEFAULT_PAYLOAD_FEE_FLOOR_SOMPI,
        _ => DEFAULT_MINIMUM_RELAY_FEE_SOMPI,
    }
}

fn funding_poll_interval() -> Duration {
    Duration::from_secs(optional_u64_env("ANCHOR_TEST_FUNDING_POLL_INTERVAL_SECS").unwrap_or(DEFAULT_FUNDING_POLL_INTERVAL_SECS))
}

fn account_balance_lookup_timeout() -> Duration {
    Duration::from_secs(optional_u64_env("ANCHOR_TEST_INITIAL_BALANCE_LOOKUP_TIMEOUT_SECS").unwrap_or(10))
}

enum SpendabilityCheck {
    Ready(anchor::GeneratorSummary),
    Waiting(String),
}

async fn wait_for_live_wallet_funding(
    service: &KaspaWalletService,
    live_wallet: &LiveSendWallet,
    destination_address: &str,
    amount_sompi: u64,
    priority_fee_sompi: u64,
    payload: Option<&[u8]>,
) -> anchor::GeneratorSummary {
    let fee_floor_sompi = optional_u64_env("ANCHOR_TEST_SEND_FEE_FLOOR_SOMPI").unwrap_or_else(|| default_fee_floor_sompi(payload));
    let payload_bytes = payload.map_or(0, |bytes| bytes.len());
    let configured_minimum_total = amount_sompi
        .saturating_add(priority_fee_sompi)
        .saturating_add(fee_floor_sompi);
    let mut last_status = String::new();
    let mut last_balance = None;
    let events = service.subscribe_events();
    let activation_service = service.clone();
    let activation_account_id = live_wallet.account_id.clone();
    let activation = activation_service.activate_account(activation_account_id);
    tokio::pin!(activation);
    let mut activation_complete = false;

    println!(
        "watching reusable live-smoke wallet {} for spendability; fund {} with roughly at least {} KAS if needed (send {} + relay/payload fee floor {} + priority fee {}{})",
        live_wallet.state.filename,
        live_wallet.state.funding_address,
        format_kaspa_amount(configured_minimum_total),
        format_kaspa_amount(amount_sompi),
        format_kaspa_amount(fee_floor_sompi),
        format_kaspa_amount(priority_fee_sompi),
        if payload_bytes == 0 {
            String::new()
        } else {
            format!(", payload {} bytes", payload_bytes)
        }
    );

    loop {
        tokio::select! {
            activation_result = &mut activation, if !activation_complete => {
                activation_result
                    .expect("should activate the persistent live-send account before waiting for spendability events");
                activation_complete = true;
                log_status_once(
                    &mut last_status,
                    format!(
                        "persistent live-smoke wallet account {} is active; waiting for wallet connection and balance events",
                        live_wallet.state.filename,
                    ),
                );
            }
            event_result = events.recv() => {
                let event = event_result
                    .expect("should receive wallet events while waiting for live-send funding");

                if let Some(event_status) = live_wallet_event_status(&event, live_wallet, &mut last_balance) {
                    log_status_once(&mut last_status, event_status);
                }

                if activation_complete && event_requires_spendability_probe(&event, &live_wallet.account_id) {
                    if event_refreshes_account_balance_over_wallet_connection(&event, &live_wallet.account_id) {
                        refresh_live_wallet_balance_over_wallet_connection(
                            service,
                            live_wallet,
                            &mut last_status,
                            &mut last_balance,
                        )
                        .await;
                    }

                    if let Some(summary) = log_spendability_probe(
                        &mut last_status,
                        amount_sompi,
                        probe_live_wallet_spendability(
                            service,
                            live_wallet,
                            destination_address,
                            amount_sompi,
                            priority_fee_sompi,
                            payload,
                            configured_minimum_total,
                            last_balance.as_ref(),
                        )
                        .await,
                    ) {
                        return summary;
                    }
                }
            }
            _ = sleep(funding_poll_interval()) => {
                log_status_once(
                    &mut last_status,
                    format!(
                        "waiting for wallet {} events at {}; account activation {} and last observed account balance {}",
                        live_wallet.state.filename,
                        live_wallet.state.funding_address,
                        if activation_complete { "completed" } else { "still in progress" },
                        format_optional_wallet_balance(last_balance.as_ref())
                    ),
                );
            }
        }
    }
}

async fn probe_live_wallet_spendability(
    service: &KaspaWalletService,
    live_wallet: &LiveSendWallet,
    destination_address: &str,
    amount_sompi: u64,
    priority_fee_sompi: u64,
    payload: Option<&[u8]>,
    configured_minimum_total: u64,
    last_balance: Option<&KaspaWalletBalanceSnapshot>,
) -> SpendabilityCheck {
    let refresh_attempt_timeout = Duration::from_secs(
        optional_u64_env("ANCHOR_TEST_ACCOUNT_REFRESH_ATTEMPT_TIMEOUT_SECS").unwrap_or(30),
    );
    let estimate_result = timeout(
        refresh_attempt_timeout,
        service.estimate_send_to_address(
            live_wallet.account_id.clone(),
            destination_address,
            amount_sompi,
            priority_fee_sompi,
            payload.map(|bytes| bytes.to_vec()),
        ),
    )
    .await;

    match estimate_result {
        Ok(Ok(summary)) => SpendabilityCheck::Ready(summary),
        Ok(Err(err)) => {
            let err_text = err.to_string();
            let reason = if err_text.contains("Insufficient funds") {
                "waiting for additional spendable funds"
            } else {
                "waiting for wallet spendability refresh"
            };

            SpendabilityCheck::Waiting(format!(
                "{reason} for persistent live-smoke wallet {} at {}: rough target {} KAS, observed {} ({err_text})",
                live_wallet.state.filename,
                live_wallet.state.funding_address,
                format_kaspa_amount(configured_minimum_total),
                format_optional_wallet_balance(last_balance)
            ))
        }
        Err(_) => SpendabilityCheck::Waiting(format!(
            "wallet spendability refresh is still in progress for {} at {}; last observed account balance {}",
            live_wallet.state.filename,
            live_wallet.state.funding_address,
            format_optional_wallet_balance(last_balance)
        )),
    }
}

async fn refresh_live_wallet_balance_over_wallet_connection(
    service: &KaspaWalletService,
    live_wallet: &LiveSendWallet,
    last_status: &mut String,
    last_balance: &mut Option<KaspaWalletBalanceSnapshot>,
) {
    match timeout(
        account_balance_lookup_timeout(),
        service.account_balance(live_wallet.account_id.clone()),
    )
    .await
    {
        Ok(Ok(balance)) => {
            let balance_changed = last_balance.as_ref() != Some(&balance);
            *last_balance = Some(balance);

            if balance_changed {
                log_status_once(
                    last_status,
                    format!(
                        "wallet account balance via active wallet connection for {}: {}",
                        live_wallet.state.filename,
                        format_optional_wallet_balance(last_balance.as_ref())
                    ),
                );
            }
        }
        Ok(Err(err)) => {
            let err_text = err.to_string();
            if !err_text.contains("wallet connection is still in progress") {
                log_status_once(
                    last_status,
                    format!(
                        "wallet account balance via active wallet connection for {} is not ready yet: {}",
                        live_wallet.state.filename,
                        err_text
                    ),
                );
            }
        }
        Err(_) => {
            log_status_once(
                last_status,
                format!(
                    "wallet account balance refresh via active wallet connection for {} timed out after {}s",
                    live_wallet.state.filename,
                    account_balance_lookup_timeout().as_secs()
                ),
            );
        }
    }
}

fn live_wallet_event_status(
    event: &KaspaWalletEvent,
    live_wallet: &LiveSendWallet,
    last_balance: &mut Option<KaspaWalletBalanceSnapshot>,
) -> Option<String> {
    match event {
        KaspaWalletEvent::Connect { url } => Some(format!(
            "wallet runtime connected for {} via {}",
            live_wallet.state.filename,
            url.as_deref().unwrap_or("unknown endpoint")
        )),
        KaspaWalletEvent::Disconnect { url } => Some(format!(
            "wallet runtime disconnected for {} from {}",
            live_wallet.state.filename,
            url.as_deref().unwrap_or("unknown endpoint")
        )),
        KaspaWalletEvent::ServerStatus { is_synced, url } => Some(format!(
            "wallet backend status for {} via {}: {}",
            live_wallet.state.filename,
            url.as_deref().unwrap_or("unknown endpoint"),
            if *is_synced { "synced" } else { "not synced yet" }
        )),
        KaspaWalletEvent::SyncState {
            is_synced,
            description,
        } => Some(format!(
            "wallet sync state for {}: {}{}",
            live_wallet.state.filename,
            description,
            if *is_synced { " (checking spendability)" } else { "" }
        )),
        KaspaWalletEvent::AccountActivation { ids } if ids.contains(&live_wallet.account_id) => Some(format!(
            "persistent live-smoke wallet account {} is active; waiting for balance and spendability events",
            live_wallet.state.filename,
        )),
        KaspaWalletEvent::Balance { account_id, balance } if account_id == &live_wallet.account_id => {
            *last_balance = balance.clone();
            Some(format!(
                "wallet balance event for {}: {}",
                live_wallet.state.filename,
                format_optional_wallet_balance(last_balance.as_ref())
            ))
        }
        KaspaWalletEvent::WalletError { message } => Some(format!(
            "wallet runtime reported an error while waiting for {}: {}",
            live_wallet.state.filename,
            message
        )),
        KaspaWalletEvent::UtxoProcStart => Some(format!(
            "wallet UTXO processor started for {}; waiting for account balance events",
            live_wallet.state.filename,
        )),
        KaspaWalletEvent::UtxoProcError { message } => Some(format!(
            "wallet UTXO processor reported an error while waiting for {}: {}",
            live_wallet.state.filename,
            message
        )),
        _ => None,
    }
}

fn event_requires_spendability_probe(event: &KaspaWalletEvent, account_id: &anchor::AccountId) -> bool {
    match event {
        KaspaWalletEvent::Connect { .. } => true,
        KaspaWalletEvent::Balance { account_id: event_account_id, .. } => event_account_id == account_id,
        KaspaWalletEvent::ServerStatus { .. }
        | KaspaWalletEvent::SyncState { .. }
        | KaspaWalletEvent::UtxoProcStart => true,
        _ => false,
    }
}

fn event_refreshes_account_balance_over_wallet_connection(
    event: &KaspaWalletEvent,
    account_id: &anchor::AccountId,
) -> bool {
    match event {
        KaspaWalletEvent::Connect { .. }
        | KaspaWalletEvent::ServerStatus { .. }
        | KaspaWalletEvent::SyncState { .. }
        | KaspaWalletEvent::UtxoProcStart => true,
        KaspaWalletEvent::Balance { account_id: event_account_id, .. } => event_account_id != account_id,
        _ => false,
    }
}

fn log_spendability_probe(
    last_status: &mut String,
    amount_sompi: u64,
    check: SpendabilityCheck,
) -> Option<anchor::GeneratorSummary> {
    match check {
        SpendabilityCheck::Ready(summary) => {
            let estimated_total = amount_sompi.saturating_add(summary.aggregated_fees);
            log_status_once(
                last_status,
                format!(
                    "reusable live-smoke wallet is spendable now: estimated fees {} KAS, estimated total {} KAS",
                    format_kaspa_amount(summary.aggregated_fees),
                    format_kaspa_amount(estimated_total)
                ),
            );
            Some(summary)
        }
        SpendabilityCheck::Waiting(status) => {
            log_status_once(last_status, status);
            None
        }
    }
}

fn log_status_once(last_status: &mut String, status: String) {
    if *last_status != status {
        println!("{status}");
        *last_status = status;
    }
}

fn format_optional_wallet_balance(balance: Option<&KaspaWalletBalanceSnapshot>) -> String {
    match balance {
        Some(balance) => format!(
            "mature {} KAS, pending {} KAS, outgoing {} KAS",
            format_kaspa_amount(balance.mature_sompi),
            format_kaspa_amount(balance.pending_sompi),
            format_kaspa_amount(balance.outgoing_sompi)
        ),
        None => "unavailable".to_string(),
    }
}

async fn resolve_live_wallet_backend() -> LiveBackendSelection {
    let wallet_config = common::wallet_config(WalletStorageMode::Local);

    if let Some(url) = wallet_config.url.clone() {
        return LiveBackendSelection {
            description: format!("{} (direct endpoint)", url),
            wallet_config,
        };
    }

    LiveBackendSelection {
        description: "public resolver (wallet runtime will report the connected endpoint after connect)".to_string(),
        wallet_config,
    }
}

fn format_kaspa_amount(sompi: u64) -> String {
    let whole = sompi / SOMPI_PER_KASPA;
    let fractional = sompi % SOMPI_PER_KASPA;

    if fractional == 0 {
        return whole.to_string();
    }

    let mut fractional_text = format!("{fractional:08}");
    while fractional_text.ends_with('0') {
        fractional_text.pop();
    }

    format!("{whole}.{fractional_text}")
}