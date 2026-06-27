use anchor::{AnchorError, KaspaIndexerConfig, KaspaNetwork, KaspaNodeConfig, KaspaWalletConfig, WalletStorageMode};

#[test]
fn known_networks_map_to_network_ids() {
    for network in [
        KaspaNetwork::mainnet(),
        KaspaNetwork::testnet_10(),
        KaspaNetwork::testnet_11(),
        KaspaNetwork::devnet(),
        KaspaNetwork::simnet(),
    ] {
        let network_id = network
            .to_network_id()
            .unwrap_or_else(|err| panic!("known network {} should parse: {err}", network.as_str()));
        assert_eq!(network_id.to_string(), network.as_str());
    }
}

#[test]
fn invalid_networks_are_rejected() {
    let err = KaspaNetwork::new("not-a-kaspa-network")
        .to_network_id()
        .expect_err("invalid network labels should not parse");

    match err {
        AnchorError::InvalidNetwork(label) => assert_eq!(label, "not-a-kaspa-network"),
        other => panic!("expected invalid-network error, got {other:?}"),
    }
}

#[test]
fn indexer_base_url_is_normalized() {
    let config = KaspaIndexerConfig {
        base_url: "http://127.0.0.1:8500///".to_string(),
    };

    assert_eq!(config.normalized_base_url(), "http://127.0.0.1:8500");
}

#[test]
fn default_configs_remain_live_safe() {
    let node = KaspaNodeConfig::default();
    assert_eq!(node.network, KaspaNetwork::mainnet());
    assert!(node.use_public_resolver);
    assert!(node.poll_interval_ms > 0);

    let wallet = KaspaWalletConfig::default();
    assert_eq!(wallet.network, KaspaNetwork::mainnet());
    assert!(wallet.use_public_resolver);
    assert_eq!(wallet.storage_mode, WalletStorageMode::Local);
}