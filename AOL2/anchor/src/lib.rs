mod config;
mod error;
mod indexer;
mod node;
mod service;
mod types;
mod wallet;

pub use config::{KaspaAnchorConfig, KaspaIndexerConfig, KaspaNetwork, KaspaNodeConfig, KaspaWalletConfig, WalletStorageMode};
pub use error::{AnchorError, Result};
pub use indexer::KaspaIndexerClient;
pub use node::KaspaNodeClient;
pub use service::KaspaAnchorService;
pub use types::{
	BlockHashBatch, BlockHashCursor, CreateWalletRequest, CreatedWallet, ImportedWallet, ImportedWalletArchive,
	ImportWalletArchiveRequest, ImportWalletFromMnemonicRequest, IndexerHealth, IndexerSnapshot, NewAddressResult,
	OpenWalletRequest, SendTransactionRequest, SendTransactionResult, WalletAccountKind,
};
pub use wallet::{KaspaWalletBalanceSnapshot, KaspaWalletEvent, KaspaWalletEventSubscription, KaspaWalletService};

pub use futures::stream::BoxStream;
pub use kaspa_rpc_core::{GetBlockDagInfoResponse, RpcHash};
pub use kaspa_wallet_core::{
	prelude::{
		AccountDescriptor, AccountId, Address, Fees, PaymentDestination, PaymentOutput, PaymentOutputs, TransactionId,
		WalletDescriptor,
	},
	tx::GeneratorSummary,
};
pub use kaspa_wrpc_client::prelude::{NetworkId, NetworkType};
