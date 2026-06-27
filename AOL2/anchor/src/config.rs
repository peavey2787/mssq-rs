use std::str::FromStr;

use kaspa_wrpc_client::prelude::NetworkId;
use serde::{Deserialize, Serialize};

use crate::error::{AnchorError, Result};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct KaspaNetwork(pub String);

impl KaspaNetwork {
    pub fn new(label: impl Into<String>) -> Self {
        Self(label.into())
    }

    pub fn mainnet() -> Self {
        Self::new("mainnet")
    }

    pub fn testnet_10() -> Self {
        Self::new("testnet-10")
    }

    pub fn testnet_11() -> Self {
        Self::new("testnet-11")
    }

    pub fn devnet() -> Self {
        Self::new("devnet")
    }

    pub fn simnet() -> Self {
        Self::new("simnet")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_network_id(&self) -> Result<NetworkId> {
        NetworkId::from_str(self.as_str()).map_err(|_| AnchorError::InvalidNetwork(self.0.clone()))
    }
}

impl Default for KaspaNetwork {
    fn default() -> Self {
        Self::mainnet()
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletStorageMode {
    #[default]
    Local,
    Resident,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KaspaNodeConfig {
    pub network: KaspaNetwork,
    pub url: Option<String>,
    pub use_public_resolver: bool,
    pub poll_interval_ms: u64,
}

impl Default for KaspaNodeConfig {
    fn default() -> Self {
        Self {
            network: KaspaNetwork::mainnet(),
            url: None,
            use_public_resolver: true,
            poll_interval_ms: 1_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KaspaWalletConfig {
    pub network: KaspaNetwork,
    pub url: Option<String>,
    pub use_public_resolver: bool,
    pub storage_mode: WalletStorageMode,
}

impl Default for KaspaWalletConfig {
    fn default() -> Self {
        Self {
            network: KaspaNetwork::mainnet(),
            url: None,
            use_public_resolver: true,
            storage_mode: WalletStorageMode::Local,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KaspaIndexerConfig {
    pub base_url: String,
}

impl KaspaIndexerConfig {
    pub fn normalized_base_url(&self) -> String {
        self.base_url.trim_end_matches('/').to_string()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct KaspaAnchorConfig {
    pub node: KaspaNodeConfig,
    pub wallet: KaspaWalletConfig,
    pub indexer: Option<KaspaIndexerConfig>,
}