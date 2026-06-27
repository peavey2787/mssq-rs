#![allow(dead_code)]

use std::{
    env, fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anchor::{KaspaAnchorConfig, KaspaIndexerConfig, KaspaNetwork, KaspaNodeConfig, KaspaWalletConfig, WalletStorageMode};
use kaspa_wallet_core::storage::local::set_default_storage_folder;

const DEFAULT_LIVE_INDEXER_URL: &str = "http://127.0.0.1:8500";
const DEFAULT_LIVE_INDEXER_LISTEN: &str = "127.0.0.1:8500";
const DEFAULT_LIVE_STORAGE_ROOT: &str = ".mssq-anchor-tests";
const DEFAULT_INDEXER_POSTGRES_PORT: u16 = 55_432;
const DEFAULT_INDEXER_STARTUP_TIMEOUT_SECS: u64 = 300;
const LOCAL_INDEXER_DISABLE: &str = "block_parent_table,blocks_transactions_table,addresses_transactions_table,rejected_transactions";
const LOCAL_INDEXER_EXCLUDE_FIELDS: &str = "block_accepted_id_merkle_root,block_merge_set_blues_hashes,block_merge_set_reds_hashes,block_selected_parent_hash,block_bits,block_blue_work,block_daa_score,block_hash_merkle_root,block_nonce,block_pruning_point,block_utxo_commitment,block_version,tx_hash,tx_mass,tx_payload,tx_in_signature_script,tx_in_sig_op_count,tx_out_script_public_key_address";
const DOCKER_USE_SUDO_ENV: &str = "ANCHOR_TEST_DOCKER_USE_SUDO";

pub fn env_var(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

pub fn bool_env(name: &str) -> bool {
    env_var(name).is_some_and(|value| {
        matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
    })
}

pub fn live_enabled() -> bool {
    bool_env("ANCHOR_LIVE_TEST")
}

pub fn require_live_enabled(test_name: &str) {
    assert!(live_enabled(), "{test_name} requires ANCHOR_LIVE_TEST=1");
}

pub fn required_env(name: &str) -> String {
    env_var(name).unwrap_or_else(|| panic!("{name} is required"))
}

pub fn optional_hex_env(name: &str) -> Option<Vec<u8>> {
    env_var(name).map(|value| hex::decode(&value).unwrap_or_else(|err| panic!("{name} must be valid hex: {err}")))
}

pub fn network() -> KaspaNetwork {
    env_var("ANCHOR_TEST_NETWORK")
        .map(KaspaNetwork::new)
        .unwrap_or_else(KaspaNetwork::testnet_10)
}

pub fn node_url() -> Option<String> {
    env_var("ANCHOR_TEST_NODE_URL")
}

pub fn indexer_url() -> Option<String> {
    env_var("ANCHOR_TEST_INDEXER_URL")
}

pub fn live_indexer_url() -> String {
    indexer_url().unwrap_or_else(|| DEFAULT_LIVE_INDEXER_URL.to_string())
}

pub fn poll_interval_ms() -> u64 {
    env_var("ANCHOR_TEST_POLL_INTERVAL_MS")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250)
}

pub fn node_config() -> KaspaNodeConfig {
    let url = node_url();
    KaspaNodeConfig {
        network: network(),
        url: url.clone(),
        use_public_resolver: url.is_none(),
        poll_interval_ms: poll_interval_ms(),
    }
}

pub fn wallet_config(storage_mode: WalletStorageMode) -> KaspaWalletConfig {
    let url = node_url();
    KaspaWalletConfig {
        network: network(),
        url: url.clone(),
        use_public_resolver: url.is_none(),
        storage_mode,
    }
}

pub fn anchor_config(storage_mode: WalletStorageMode) -> KaspaAnchorConfig {
    KaspaAnchorConfig {
        node: node_config(),
        wallet: wallet_config(storage_mode),
        indexer: indexer_url().map(|base_url| KaspaIndexerConfig { base_url }),
    }
}

pub fn anchor_config_with_indexer(storage_mode: WalletStorageMode, base_url: String) -> KaspaAnchorConfig {
    KaspaAnchorConfig {
        node: node_config(),
        wallet: wallet_config(storage_mode),
        indexer: Some(KaspaIndexerConfig { base_url }),
    }
}

pub fn word_count(phrase: &str) -> usize {
    phrase.split_whitespace().count()
}

pub fn unique_temp_dir(prefix: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let path = env::temp_dir().join(format!("mssq-anchor-{prefix}-{}-{unique}", std::process::id()));
    fs::create_dir_all(&path).expect("should create unique temp directory for anchor tests");
    path
}

pub fn install_storage_dir(path: &Path) {
    unsafe {
        set_default_storage_folder(path.to_string_lossy().into_owned())
            .expect("should configure wallet-core local storage folder for this test process");
    }
}

pub fn persistent_live_wallet_storage_dir() -> PathBuf {
    if let Some(storage_dir) = env_var("ANCHOR_TEST_WALLET_STORAGE_DIR") {
        return PathBuf::from(storage_dir);
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join(DEFAULT_LIVE_STORAGE_ROOT)
        .join(network().as_str())
}

pub fn local_indexer_repo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("indexer")
        .join("simply-kaspa-indexer")
}

pub struct ManagedLiveIndexer {
    base_url: String,
    indexer_child: Option<Child>,
    postgres_container_name: Option<String>,
}

impl ManagedLiveIndexer {
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Drop for ManagedLiveIndexer {
    fn drop(&mut self) {
        if let Some(mut child) = self.indexer_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

        if let Some(container_name) = self.postgres_container_name.take() {
            let mut command = docker_command();
            let _ = command
                .args(["rm", "-f", &container_name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

pub fn ensure_live_indexer() -> ManagedLiveIndexer {
    if let Some(base_url) = indexer_url() {
        return ManagedLiveIndexer {
            base_url,
            indexer_child: None,
            postgres_container_name: None,
        };
    }

    let repo_dir = local_indexer_repo_dir();
    assert!(
        repo_dir.join("README.md").exists(),
        "bundled simply-kaspa-indexer checkout was not found at {}; clone it into AOL2/anchor/indexer/simply-kaspa-indexer",
        repo_dir.display()
    );

    let (postgres_container_name, database_url) = resolve_indexer_database();
    let base_url = DEFAULT_LIVE_INDEXER_URL.to_string();
    let network = network();
    let mut command = Command::new("cargo");
    command.current_dir(&repo_dir);
    command.arg("run");
    command.arg("-p");
    command.arg("simply-kaspa-indexer");
    command.arg("--");
    command.arg("-u");
    command.arg("-n");
    command.arg(network.as_str());
    command.arg("-d");
    command.arg(&database_url);
    command.arg("-l");
    command.arg(DEFAULT_LIVE_INDEXER_LISTEN);
    command.arg(format!("--disable={LOCAL_INDEXER_DISABLE}"));
    command.arg(format!("--exclude-fields={LOCAL_INDEXER_EXCLUDE_FIELDS}"));
    if let Some(url) = node_url() {
        command.arg("-s");
        command.arg(url);
    }
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    let mut child = command.spawn().unwrap_or_else(|err| {
        panic!(
            "should launch bundled simply-kaspa-indexer from {}: {err}",
            repo_dir.display()
        )
    });

    wait_for_local_indexer_health(&mut child, &base_url);

    ManagedLiveIndexer {
        base_url,
        indexer_child: Some(child),
        postgres_container_name,
    }
}

fn resolve_indexer_database() -> (Option<String>, String) {
    if let Some(database_url) = env_var("ANCHOR_TEST_INDEXER_DATABASE_URL") {
        return (None, database_url);
    }

    assert!(
        docker_available(),
        "bundled simply-kaspa-indexer startup requires either ANCHOR_TEST_INDEXER_DATABASE_URL or a working Docker installation for local Postgres; use anchor-live-smoke if you want the runner to offer Docker installation interactively"
    );

    let port = env_var("ANCHOR_TEST_INDEXER_POSTGRES_PORT")
        .map(|value| {
            value
                .parse::<u16>()
                .unwrap_or_else(|err| panic!("ANCHOR_TEST_INDEXER_POSTGRES_PORT should parse as a u16: {err}"))
        })
        .unwrap_or(DEFAULT_INDEXER_POSTGRES_PORT);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_secs();
    let container_name = format!("anchor-indexer-postgres-{}-{unique}", std::process::id());
    let port_mapping = format!("127.0.0.1:{port}:5432");
    let mut command = docker_command();
    let status = command
        .args([
            "run",
            "-d",
            "--rm",
            "--name",
            &container_name,
            "-e",
            "POSTGRES_USER=postgres",
            "-e",
            "POSTGRES_PASSWORD=postgres",
            "-e",
            "POSTGRES_DB=postgres",
            "-p",
            &port_mapping,
            "postgres:16-alpine",
        ])
        .status()
        .unwrap_or_else(|err| panic!("should launch local Postgres container for bundled indexer: {err}"));

    assert!(
        status.success(),
        "failed to launch local Postgres container for bundled indexer with status {status}"
    );

    wait_for_tcp_port(
        &format!("127.0.0.1:{port}"),
        Duration::from_secs(30),
        "local Postgres container for bundled indexer",
    );

    (
        Some(container_name),
        format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres"),
    )
}

fn docker_available() -> bool {
    docker_command()
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn docker_command() -> Command {
    if bool_env(DOCKER_USE_SUDO_ENV) {
        let mut command = Command::new("sudo");
        command.arg("docker");
        command
    } else {
        Command::new("docker")
    }
}

fn wait_for_local_indexer_health(child: &mut Child, base_url: &str) {
    let timeout = env_var("ANCHOR_TEST_INDEXER_STARTUP_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .unwrap_or_else(|err| panic!("ANCHOR_TEST_INDEXER_STARTUP_TIMEOUT_SECS should parse as a u64: {err}"))
        })
        .unwrap_or(DEFAULT_INDEXER_STARTUP_TIMEOUT_SECS);
    let deadline = Instant::now() + Duration::from_secs(timeout);

    while Instant::now() < deadline {
        if let Some(status) = child
            .try_wait()
            .unwrap_or_else(|err| panic!("should poll bundled simply-kaspa-indexer child status: {err}"))
        {
            panic!("bundled simply-kaspa-indexer exited before becoming healthy with status {status}");
        }

        if http_get_ok(DEFAULT_LIVE_INDEXER_LISTEN, "/api/health") {
            return;
        }

        thread::sleep(Duration::from_millis(500));
    }

    panic!(
        "bundled simply-kaspa-indexer did not become healthy at {base_url}/api/health within {timeout}s"
    );
}

fn wait_for_tcp_port(address: &str, timeout: Duration, label: &str) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(address).is_ok() {
            return;
        }

        thread::sleep(Duration::from_millis(250));
    }

    panic!("{label} did not open {address} within {}s", timeout.as_secs());
}

fn http_get_ok(address: &str, path: &str) -> bool {
    let mut stream = match TcpStream::connect(address) {
        Ok(stream) => stream,
        Err(_) => return false,
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }

    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return false;
    }

    response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200")
}