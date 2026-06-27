use std::time::Duration;

use p2p::{start_server, P2pConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let handle = start_server(P2pConfig {
        heartbeat_every: Duration::from_secs(10),
        ..P2pConfig::default()
    })
    .await?;

    println!("peer_id={}", handle.peer_id);
    tokio::signal::ctrl_c().await?;
    handle.shutdown().await;
    Ok(())
}