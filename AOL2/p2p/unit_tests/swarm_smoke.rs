use p2p::{start_server, P2pConfig};

#[tokio::test]
async fn node_start_shutdown_smoke() {
    let handle = start_server(P2pConfig::default())
        .await
        .expect("start server");
    handle.shutdown().await;
}
