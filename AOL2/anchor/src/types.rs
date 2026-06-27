use serde::{Deserialize, Serialize};

use kaspa_wallet_core::{
    prelude::{AccountDescriptor, AccountId, PaymentDestination, TransactionId, WalletDescriptor},
    tx::GeneratorSummary,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHashCursor {
    pub low_hash: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BlockHashBatch {
    pub low_hash: Option<String>,
    pub block_hashes: Vec<String>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateWalletRequest {
    pub wallet_secret: String,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub overwrite: bool,
    pub account_name: Option<String>,
    pub payment_secret: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpenWalletRequest {
    pub wallet_secret: String,
    pub filename: Option<String>,
    pub account_descriptors: bool,
    pub include_legacy_accounts: bool,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletAccountKind {
    #[default]
    Bip32,
    Legacy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportWalletFromMnemonicRequest {
    pub wallet_secret: String,
    pub title: Option<String>,
    pub filename: Option<String>,
    pub overwrite: bool,
    pub payment_secret: Option<String>,
    pub mnemonic_phrase: String,
    pub account_kind: WalletAccountKind,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportWalletArchiveRequest {
    pub wallet_secret: String,
    pub wallet_data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct CreatedWallet {
    pub wallet_descriptor: WalletDescriptor,
    pub account_descriptor: AccountDescriptor,
    pub mnemonic_phrase: String,
}

#[derive(Clone, Debug)]
pub struct ImportedWallet {
    pub wallet_descriptor: WalletDescriptor,
    pub account_descriptor: AccountDescriptor,
}

#[derive(Clone, Debug)]
pub struct ImportedWalletArchive {
    pub wallet_descriptor: WalletDescriptor,
}

#[derive(Clone, Debug)]
pub struct SendTransactionRequest {
    pub account_id: AccountId,
    pub wallet_secret: String,
    pub payment_secret: Option<String>,
    pub destination: PaymentDestination,
    pub priority_fee_sompi: u64,
    pub payload: Option<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct SendTransactionResult {
    pub generator_summary: GeneratorSummary,
    pub transaction_ids: Vec<TransactionId>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewAddressResult {
    pub address: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexerSnapshot {
    pub checkpoint_hash: Option<String>,
    pub latest_block_hash: Option<String>,
    pub metrics: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexerHealth {
    pub payload: serde_json::Value,
}