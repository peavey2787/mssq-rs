#[cfg(all(windows, target_arch = "x86_64"))]
#[tokio::test]
async fn windows_tokio_boot_smoke() {
    let handle = p2p::start_server(p2p::P2pConfig::default())
        .await
        .expect("start server");
    handle.shutdown().await;
}
