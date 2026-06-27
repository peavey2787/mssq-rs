use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use kaspa_wallet_core::{
    account::{BIP32_ACCOUNT_KIND, LEGACY_ACCOUNT_KIND},
    api::{AccountsEstimateRequest, AccountsSendRequest, NewAddressKind, WalletApi as _, WalletImportRequest},
    events::{Events, SyncState as RawSyncState},
    prelude::{
        AccountDescriptor, AccountId, Address, Fees, Language, Mnemonic, PaymentDestination, PaymentOutput, Secret,
        Wallet, WalletCreateArgs, WordCount,
    },
    tx::GeneratorSummary,
    utxo::{balance::Balance as RawBalance, UtxoEntryReference, UtxoEntryReferenceExtension},
};
use kaspa_wrpc_client::{client::{ConnectOptions, ConnectStrategy}, prelude::RpcState, Resolver};
use workflow_core::channel::MultiplexerChannel;

use crate::{
    config::{KaspaWalletConfig, WalletStorageMode},
    error::{AnchorError, Result},
    types::{
        CreateWalletRequest, CreatedWallet, ImportedWallet, ImportedWalletArchive, ImportWalletArchiveRequest,
        ImportWalletFromMnemonicRequest, NewAddressResult, OpenWalletRequest, SendTransactionRequest, SendTransactionResult,
        WalletAccountKind,
    },
};

const WALLET_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KaspaWalletBalanceSnapshot {
    pub mature_sompi: u64,
    pub pending_sompi: u64,
    pub outgoing_sompi: u64,
    pub mature_utxo_count: usize,
    pub pending_utxo_count: usize,
    pub stasis_utxo_count: usize,
}

impl From<RawBalance> for KaspaWalletBalanceSnapshot {
    fn from(balance: RawBalance) -> Self {
        Self {
            mature_sompi: balance.mature,
            pending_sompi: balance.pending,
            outgoing_sompi: balance.outgoing,
            mature_utxo_count: balance.mature_utxo_count,
            pending_utxo_count: balance.pending_utxo_count,
            stasis_utxo_count: balance.stasis_utxo_count,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KaspaWalletEvent {
    Connect { url: Option<String> },
    Disconnect { url: Option<String> },
    ServerStatus { is_synced: bool, url: Option<String> },
    SyncState { is_synced: bool, description: String },
    AccountActivation { ids: Vec<AccountId> },
    Balance {
        account_id: AccountId,
        balance: Option<KaspaWalletBalanceSnapshot>,
    },
    WalletError { message: String },
    UtxoProcStart,
    UtxoProcError { message: String },
    Other,
}

impl From<Events> for KaspaWalletEvent {
    fn from(event: Events) -> Self {
        match event {
            Events::ServerStatus { is_synced, url, .. } => Self::ServerStatus { is_synced, url },
            Events::SyncState { sync_state } => Self::SyncState {
                is_synced: sync_state.is_synced(),
                description: describe_sync_state(&sync_state),
            },
            Events::AccountActivation { ids } => Self::AccountActivation { ids },
            Events::Balance { balance, id } => Self::Balance {
                account_id: id.into(),
                balance: balance.map(Into::into),
            },
            Events::WalletError { message } => Self::WalletError { message },
            Events::UtxoProcStart => Self::UtxoProcStart,
            Events::UtxoProcError { message } => Self::UtxoProcError { message },
            _ => Self::Other,
        }
    }
}

#[derive(Clone)]
pub struct KaspaWalletEventSubscription {
    wallet: Arc<Wallet>,
    wallet_channel: MultiplexerChannel<Box<Events>>,
    rpc_state_channel: MultiplexerChannel<RpcState>,
}

impl KaspaWalletEventSubscription {
    pub async fn recv(&self) -> Result<KaspaWalletEvent> {
        tokio::select! {
            wallet_event = self.wallet_channel.recv() => {
                wallet_event
                    .map(|event| (*event).into())
                    .map_err(|err| AnchorError::KaspaWallet(format!("wallet event channel closed: {err}")))
            }
            rpc_state = self.rpc_state_channel.recv() => {
                rpc_state
                    .map(|state| match state {
                        RpcState::Connected => KaspaWalletEvent::Connect {
                            url: current_wallet_endpoint_description(&self.wallet),
                        },
                        RpcState::Disconnected => KaspaWalletEvent::Disconnect {
                            url: current_wallet_endpoint_description(&self.wallet),
                        },
                    })
                    .map_err(|err| AnchorError::KaspaWallet(format!("wallet rpc state channel closed: {err}")))
            }
        }
    }

    pub fn close(&self) {
        self.wallet_channel.close();
        self.rpc_state_channel.close();
    }
}

fn current_wallet_endpoint_description(wallet: &Arc<Wallet>) -> Option<String> {
    if let Some(wrpc_client) = wallet.try_wrpc_client() {
        let current_url = wrpc_client.url();
        let node_descriptor = wrpc_client.node_descriptor();

        if let Some(descriptor) = node_descriptor {
            return Some(format!("{} (peer uid {})", descriptor.url, descriptor.uid));
        }

        if let Some(url) = current_url {
            return Some(format!("{} (direct endpoint)", url));
        }
    }

    wallet
        .try_rpc_ctl()
        .and_then(|rpc_ctl| rpc_ctl.descriptor())
        .map(|url| format!("{} (rpc descriptor)", url))
}

fn describe_sync_state(sync_state: &RawSyncState) -> String {
    match sync_state {
        RawSyncState::Proof { level } => format!("proof sync level {level}"),
        RawSyncState::Headers { headers, progress } => {
            format!("header sync {headers} headers at {progress}%")
        }
        RawSyncState::Blocks { blocks, progress } => {
            format!("block sync {blocks} blocks at {progress}%")
        }
        RawSyncState::UtxoSync { chunks, total } => {
            format!("UTXO sync {chunks}/{total} chunks")
        }
        RawSyncState::TrustSync { processed, total } => {
            format!("trust sync {processed}/{total}")
        }
        RawSyncState::UtxoResync => "UTXO resync".to_string(),
        RawSyncState::NotSynced => "not synced yet".to_string(),
        RawSyncState::Synced => "synced".to_string(),
    }
}

#[derive(Clone)]
pub struct KaspaWalletService {
    config: KaspaWalletConfig,
    wallet: Arc<Wallet>,
    runtime_started: Arc<AtomicBool>,
    connection_requested: Arc<AtomicBool>,
}

impl KaspaWalletService {
    pub fn new(config: KaspaWalletConfig) -> Result<Self> {
        let storage = match config.storage_mode {
            WalletStorageMode::Local => Wallet::local_store().map_err(|err| AnchorError::KaspaWallet(err.to_string()))?,
            WalletStorageMode::Resident => Wallet::resident_store().map_err(|err| AnchorError::KaspaWallet(err.to_string()))?,
        };

        let network_id = config.network.to_network_id()?;
        let resolver = config.use_public_resolver.then(Resolver::default);
        let wallet = Wallet::try_new(storage, resolver, Some(network_id))
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        wallet
            .wrpc_client()
            .set_url(config.url.as_deref())
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(Self {
            config,
            wallet: Arc::new(wallet),
            runtime_started: Arc::new(AtomicBool::new(false)),
            connection_requested: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn config(&self) -> &KaspaWalletConfig {
        &self.config
    }

    pub fn wallet(&self) -> &Arc<Wallet> {
        &self.wallet
    }

    pub fn subscribe_events(&self) -> KaspaWalletEventSubscription {
        KaspaWalletEventSubscription {
            wallet: self.wallet.clone(),
            wallet_channel: self.wallet.multiplexer().channel(),
            rpc_state_channel: self
                .wallet
                .try_rpc_ctl()
                .expect("wallet should expose RpcCtl when using wRPC transport")
                .multiplexer()
                .channel(),
        }
    }

    fn connected_endpoint_description_from_wallet(&self) -> String {
        current_wallet_endpoint_description(&self.wallet).unwrap_or_else(|| "unavailable".to_string())
    }

    async fn request_connection(&self) -> Result<()> {
        let network_id = self.config.network.to_network_id()?;

        if self.wallet.is_connected() {
            return Ok(());
        }

        if self
            .connection_requested
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(());
        }

        let wrpc_client = self.wallet.wrpc_client();

        self.wallet
            .set_network_id(&network_id)
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        let connect_result = wrpc_client
            .connect(Some(ConnectOptions {
                block_async_connect: true,
                connect_timeout: Some(WALLET_CONNECT_TIMEOUT),
                strategy: ConnectStrategy::Fallback,
                ..Default::default()
            }))
            .await
            .map(|_| ())
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()));

        self.connection_requested.store(false, Ordering::SeqCst);

        connect_result
    }

    async fn ensure_connected(&self) -> Result<()> {
        self.ensure_runtime().await?;

        if self.wallet.is_connected() {
            Ok(())
        } else {
            Err(AnchorError::KaspaWallet("wallet connection is still in progress".to_string()))
        }
    }

    async fn ensure_runtime(&self) -> Result<()> {
        if self
            .runtime_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            if let Err(err) = self.wallet.start().await {
                self.runtime_started.store(false, Ordering::SeqCst);
                return Err(AnchorError::KaspaWallet(err.to_string()));
            }
        }

        self.request_connection().await?;

        Ok(())
    }

    pub async fn connected_endpoint_description(&self) -> Result<String> {
        self.ensure_connected().await?;
        Ok(self.connected_endpoint_description_from_wallet())
    }

    async fn ensure_account_active(&self, account_id: &AccountId) -> Result<()> {
        self.ensure_runtime().await?;

        self.wallet
            .clone()
            .accounts_activate(Some(vec![account_id.clone()]))
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(())
    }

    pub async fn activate_account(&self, account_id: AccountId) -> Result<()> {
        self.ensure_account_active(&account_id).await
    }

    pub async fn account_balance(&self, account_id: AccountId) -> Result<KaspaWalletBalanceSnapshot> {
        self.ensure_connected().await?;

        let guard = self.wallet.guard();
        let guard = guard.lock().await;
        let account = self
            .wallet
            .get_account_by_id(&account_id, &guard)
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?
            .ok_or_else(|| AnchorError::KaspaWallet(format!("account {account_id} was not found in wallet")))?;

        let current_daa_score = self
            .wallet
            .current_daa_score()
            .ok_or_else(|| AnchorError::KaspaWallet("wallet is connected but current DAA score is unavailable".to_string()))?;
        let params = self
            .wallet
            .utxo_processor()
            .network_params()
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        let addresses = if let Ok(derivation_account) = account.clone().as_derivation_capable() {
            let derivation = derivation_account.derivation();
            let receive_last = derivation.receive_address_manager().index();
            let change_last = derivation.change_address_manager().index();

            let mut addresses = derivation
                .receive_address_manager()
                .get_range_with_args(0..receive_last.saturating_add(1), false)
                .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;
            addresses.extend(
                derivation
                    .change_address_manager()
                    .get_range_with_args(0..change_last.saturating_add(1), false)
                    .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?,
            );
            addresses
        } else {
            vec![
                account.receive_address().map_err(|err| AnchorError::KaspaWallet(err.to_string()))?,
                account.change_address().map_err(|err| AnchorError::KaspaWallet(err.to_string()))?,
            ]
        };

        let mut aggregate = RawBalance::default();
        for chunk in addresses.chunks(64) {
            let utxos = self
                .wallet
                .rpc_api()
                .get_utxos_by_addresses(chunk.to_vec())
                .await
                .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

            let refs = utxos.into_iter().map(UtxoEntryReference::from).collect::<Vec<_>>();
            for utxo_ref in refs.iter() {
                let balance = utxo_ref.balance(params, current_daa_score);
                aggregate.mature += balance.mature;
                aggregate.pending += balance.pending;
                aggregate.outgoing += balance.outgoing;
                aggregate.mature_utxo_count += balance.mature_utxo_count;
                aggregate.pending_utxo_count += balance.pending_utxo_count;
                aggregate.stasis_utxo_count += balance.stasis_utxo_count;
            }
        }

        Ok(aggregate.into())
    }

    pub async fn create_wallet(&self, request: CreateWalletRequest) -> Result<CreatedWallet> {
        let wallet_secret = secret(&request.wallet_secret);
        let payment_secret = request.payment_secret.as_deref().map(secret);
        let wallet_args = WalletCreateArgs::new(
            request.title.clone(),
            request.filename.clone(),
            kaspa_wallet_core::prelude::EncryptionKind::XChaCha20Poly1305,
            None,
            request.overwrite,
        );

        let (wallet_descriptor, _storage_descriptor, mnemonic, account) = self
            .wallet
            .create_wallet_with_accounts(
                &wallet_secret,
                wallet_args,
                request.account_name.clone(),
                Some(BIP32_ACCOUNT_KIND.into()),
                WordCount::Words24,
                payment_secret.clone(),
            )
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        let account_descriptor = account
            .descriptor()
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(CreatedWallet {
            wallet_descriptor,
            account_descriptor,
            mnemonic_phrase: mnemonic.phrase().to_string(),
        })
    }

    pub async fn open_wallet(&self, request: OpenWalletRequest) -> Result<Option<Vec<AccountDescriptor>>> {
        self.wallet
            .clone()
            .wallet_open(
                secret(&request.wallet_secret),
                request.filename,
                request.account_descriptors,
                request.include_legacy_accounts,
            )
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))
    }

    pub async fn import_wallet_from_mnemonic(&self, request: ImportWalletFromMnemonicRequest) -> Result<ImportedWallet> {
        let wallet_secret = secret(&request.wallet_secret);
        let payment_secret = request.payment_secret.as_deref().map(secret);
        let wallet_args = WalletCreateArgs::new(
            request.title.clone(),
            request.filename.clone(),
            kaspa_wallet_core::prelude::EncryptionKind::XChaCha20Poly1305,
            None,
            request.overwrite,
        );

        let (wallet_descriptor, _storage_descriptor) = self
            .wallet
            .create_wallet(&wallet_secret, wallet_args)
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        let mnemonic = Mnemonic::new(request.mnemonic_phrase.trim(), Language::default())
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;
        let account_kind = match request.account_kind {
            WalletAccountKind::Bip32 => BIP32_ACCOUNT_KIND.into(),
            WalletAccountKind::Legacy => LEGACY_ACCOUNT_KIND.into(),
        };

        let account = self
            .wallet
            .import_with_mnemonic(&wallet_secret, payment_secret.as_ref(), mnemonic, account_kind)
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        let account_descriptor = account
            .descriptor()
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(ImportedWallet {
            wallet_descriptor,
            account_descriptor,
        })
    }

    pub async fn import_wallet_archive(&self, request: ImportWalletArchiveRequest) -> Result<ImportedWalletArchive> {
        let response = self
            .wallet
            .clone()
            .wallet_import_call(WalletImportRequest {
                wallet_secret: secret(&request.wallet_secret),
                wallet_data: request.wallet_data,
            })
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(ImportedWalletArchive {
            wallet_descriptor: response.wallet_descriptor,
        })
    }

    pub async fn create_receive_address(&self, account_id: AccountId) -> Result<NewAddressResult> {
        let response = self
            .wallet
            .clone()
            .accounts_create_new_address(account_id, NewAddressKind::Receive)
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(NewAddressResult {
            address: response.address.to_string(),
        })
    }

    pub async fn live_account_descriptor(&self, account_id: AccountId) -> Result<AccountDescriptor> {
        let account_id_string = account_id.to_string();
        self.ensure_account_active(&account_id).await?;

        self.wallet
            .clone()
            .accounts_enumerate()
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?
            .into_iter()
            .find(|descriptor| descriptor.account_id == account_id)
            .ok_or_else(|| AnchorError::KaspaWallet(format!("account {account_id_string} was not found in wallet")))
    }

    pub async fn estimate_send_to_address(
        &self,
        account_id: AccountId,
        address: &str,
        amount_sompi: u64,
        priority_fee_sompi: u64,
        payload: Option<Vec<u8>>,
    ) -> Result<GeneratorSummary> {
        let address = Address::try_from(address).map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;
        let destination = PaymentDestination::from(PaymentOutput::new(address, amount_sompi));

        self.ensure_account_active(&account_id).await?;

        self.wallet
            .clone()
            .accounts_estimate_call(AccountsEstimateRequest {
                account_id,
                destination,
                priority_fee_sompi: Fees::from(priority_fee_sompi),
                payload,
            })
            .await
            .map(|response| response.generator_summary)
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))
    }

    pub async fn send_transaction(&self, request: SendTransactionRequest) -> Result<SendTransactionResult> {
        self.ensure_account_active(&request.account_id).await?;

        let response = self
            .wallet
            .clone()
            .accounts_send_call(AccountsSendRequest {
                account_id: request.account_id,
                wallet_secret: secret(&request.wallet_secret),
                payment_secret: request.payment_secret.as_deref().map(secret),
                destination: request.destination,
                priority_fee_sompi: Fees::from(request.priority_fee_sompi),
                payload: request.payload,
            })
            .await
            .map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;

        Ok(SendTransactionResult {
            generator_summary: response.generator_summary,
            transaction_ids: response.transaction_ids,
        })
    }

    pub async fn send_to_address(
        &self,
        account_id: AccountId,
        wallet_secret: String,
        payment_secret: Option<String>,
        address: &str,
        amount_sompi: u64,
        priority_fee_sompi: u64,
        payload: Option<Vec<u8>>,
    ) -> Result<SendTransactionResult> {
        let address = Address::try_from(address).map_err(|err| AnchorError::KaspaWallet(err.to_string()))?;
        let destination = PaymentDestination::from(PaymentOutput::new(address, amount_sompi));

        self.send_transaction(SendTransactionRequest {
            account_id,
            wallet_secret,
            payment_secret,
            destination,
            priority_fee_sompi,
            payload,
        })
        .await
    }
}

fn secret(value: &str) -> Secret {
    Secret::new(value.as_bytes().to_vec())
}