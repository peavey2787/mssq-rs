use std::{
    env,
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
};

use anchor::KaspaNetwork;

const DOCKER_USE_SUDO_ENV: &str = "ANCHOR_TEST_DOCKER_USE_SUDO";

fn main() {
    if let Err(err) = run() {
        eprintln!("anchor live smoke runner failed: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let default_network = KaspaNetwork::testnet_10().as_str().to_string();
    let network = prompt_with_default("Kaspa network", &default_network)?;
    let node_endpoint = prompt_with_default(
        "Specific Kaspa wRPC node endpoint (leave blank for public resolver)",
        "",
    )?;
    let mut run_local_indexer = prompt_yes_no(
        "Run bundled simply-kaspa-indexer smoke from AOL2/anchor/indexer/simply-kaspa-indexer",
        true,
    )?;
    let run_funded_send = prompt_yes_no(
        "Run funded-send smoke and wait for funds on the persistent test wallet",
        true,
    )?;
    let storage_dir = default_storage_dir(&network);
    let docker_use_sudo = prepare_docker_for_local_indexer(&mut run_local_indexer)?;

    println!();
    println!("Using network: {network}");
    if node_endpoint.is_empty() {
        println!("Using node discovery: public resolver");
    } else {
        println!("Using node endpoint: {node_endpoint}");
    }
    println!("Persistent wallet storage: {}", storage_dir.display());
    if run_local_indexer {
        println!("Bundled local indexer smoke target: http://127.0.0.1:8500");
        if docker_use_sudo {
            println!("Docker commands will run through sudo for this session");
        }
    }
    println!();

    let command_env = command_env(&network, &node_endpoint, &storage_dir, docker_use_sudo);

    run_cargo_test(
        "config contract",
        &["test", "-p", "anchor", "--test", "config_contract", "--", "--nocapture"],
        &command_env,
    )?;
    run_cargo_test(
        "wallet lifecycle",
        &["test", "-p", "anchor", "--test", "wallet_lifecycle", "--", "--nocapture"],
        &command_env,
    )?;
    run_cargo_test(
        "live node smoke",
        &[
            "test",
            "-p",
            "anchor",
            "--test",
            "live_smoke",
            "live_node_block_polling_via_real_kaspa_node",
            "--",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ],
        &command_env,
    )?;

    if run_local_indexer {
        run_cargo_test(
            "live indexer smoke",
            &[
                "test",
                "-p",
                "anchor",
                "--test",
                "live_smoke",
                "live_indexer_health_and_anchor_best_cursor_against_real_simply_kaspa_indexer",
                "--",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ],
            &command_env,
        )?;
    }

    if run_funded_send {
        run_cargo_test(
            "live funded send smoke",
            &[
                "test",
                "-p",
                "anchor",
                "--test",
                "live_smoke",
                "live_funded_send_flow_against_real_kaspa_account",
                "--",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ],
            &command_env,
        )?;
    }

    Ok(())
}

fn command_env(
    network: &str,
    node_endpoint: &str,
    storage_dir: &PathBuf,
    docker_use_sudo: bool,
) -> Vec<(OsString, Option<OsString>)> {
    vec![
        (OsString::from("ANCHOR_LIVE_TEST"), Some(OsString::from("1"))),
        (OsString::from("ANCHOR_TEST_NETWORK"), Some(OsString::from(network))),
        (
            OsString::from("ANCHOR_TEST_WALLET_STORAGE_DIR"),
            Some(storage_dir.as_os_str().to_os_string()),
        ),
        (
            OsString::from("ANCHOR_TEST_NODE_URL"),
            if node_endpoint.is_empty() {
                None
            } else {
                Some(OsString::from(node_endpoint))
            },
        ),
        (
            OsString::from(DOCKER_USE_SUDO_ENV),
            if docker_use_sudo {
                Some(OsString::from("1"))
            } else {
                None
            },
        ),
    ]
}

fn prepare_docker_for_local_indexer(run_local_indexer: &mut bool) -> Result<bool, String> {
    if !*run_local_indexer {
        return Ok(false);
    }

    if env::var_os("ANCHOR_TEST_INDEXER_URL").is_some() || env::var_os("ANCHOR_TEST_INDEXER_DATABASE_URL").is_some() {
        return Ok(false);
    }

    if !docker_installed() {
        let install = prompt_yes_no(
            "Bundled local indexer needs Docker for its temporary Postgres backend, but Docker is not installed. Install Docker now",
            true,
        )?;

        if !install {
            println!("Skipping bundled local indexer smoke because Docker is required and no indexer database URL was supplied.");
            *run_local_indexer = false;
            return Ok(false);
        }

        install_docker()?;
    }

    ensure_docker_service_started()?;

    if docker_usable_without_sudo() {
        return Ok(false);
    }

    if docker_usable_with_sudo() {
        return Ok(true);
    }

    Err("Docker is installed but is not usable directly or via sudo in this session".to_string())
}

fn install_docker() -> Result<(), String> {
    if !command_exists("apt-get") {
        return Err("automatic Docker installation is only implemented for apt-based systems in this runner".to_string());
    }

    run_system_command("update apt package index", "sudo", &["apt-get", "update"])?;
    run_system_command(
        "install Docker packages",
        "sudo",
        &["apt-get", "install", "-y", "docker.io", "docker-compose"],
    )
}

fn ensure_docker_service_started() -> Result<(), String> {
    if command_exists("service") {
        let status = Command::new("sudo")
            .args(["service", "docker", "start"])
            .status()
            .map_err(|err| format!("failed to start docker service via service: {err}"))?;
        if status.success() {
            return Ok(());
        }
    }

    if command_exists("systemctl") {
        let status = Command::new("sudo")
            .args(["systemctl", "start", "docker"])
            .status()
            .map_err(|err| format!("failed to start docker service via systemctl: {err}"))?;
        if status.success() {
            return Ok(());
        }
    }

    Err("failed to start the Docker service after installation".to_string())
}

fn run_system_command(label: &str, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| format!("failed to {label}: {err}"))?;

    ensure_success(label, status)
}

fn docker_installed() -> bool {
    command_succeeds("docker", &["--version"])
}

fn docker_usable_without_sudo() -> bool {
    command_succeeds("docker", &["version"])
}

fn docker_usable_with_sudo() -> bool {
    command_succeeds("sudo", &["docker", "version"])
}

fn command_exists(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program} >/dev/null 2>&1")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn command_succeeds(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn run_cargo_test(
    label: &str,
    args: &[&str],
    command_env: &[(OsString, Option<OsString>)],
) -> Result<(), String> {
    println!(">>> Running {label}");

    let mut command = Command::new("cargo");
    command.current_dir(env!("CARGO_MANIFEST_DIR"));
    command.args(args);

    for (key, value) in command_env {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }

    let status = command
        .status()
        .map_err(|err| format!("failed to launch cargo for {label}: {err}"))?;

    ensure_success(label, status)
}

fn ensure_success(label: &str, status: ExitStatus) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} exited with status {status}"))
    }
}

fn prompt_with_default(label: &str, default: &str) -> Result<String, String> {
    let suffix = if default.is_empty() {
        String::new()
    } else {
        format!(" [{default}]")
    };

    print!("{label}{suffix}: ");
    io::stdout()
        .flush()
        .map_err(|err| format!("failed to flush stdout for {label}: {err}"))?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|err| format!("failed to read {label}: {err}"))?;
    let input = input.trim();

    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool, String> {
    let default_hint = if default { "Y/n" } else { "y/N" };
    let response = prompt_with_default(&format!("{label} ({default_hint})"), "")?;

    if response.is_empty() {
        return Ok(default);
    }

    match response.to_ascii_lowercase().as_str() {
        "y" | "yes" => Ok(true),
        "n" | "no" => Ok(false),
        _ => Err(format!("invalid yes/no response for {label}: {response}")),
    }
}

fn default_storage_dir(network: &str) -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir)
        .join(".mssq-anchor-tests")
        .join(network)
}