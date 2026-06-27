mod common;

use anchor::{
    CreateWalletRequest, ImportWalletArchiveRequest, ImportWalletFromMnemonicRequest, KaspaWalletService,
    OpenWalletRequest, WalletAccountKind, WalletStorageMode,
};
use kaspa_wallet_core::{
    api::{WalletApi as _, WalletExportRequest},
    prelude::Secret,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wallet_lifecycle_round_trips_create_open_mnemonic_and_archive_imports() {
    let storage_dir = common::unique_temp_dir("wallet-lifecycle");
    common::install_storage_dir(&storage_dir);

    let wallet_secret = "anchor-test-wallet-secret".to_string();
    let payment_secret = "anchor-test-payment-secret".to_string();
    let primary_title = "Anchor Lifecycle Primary".to_string();
    let primary_filename = "anchor-lifecycle-primary".to_string();
    let mnemonic_title = "Anchor Lifecycle Mnemonic".to_string();
    let mnemonic_filename = "anchor-lifecycle-mnemonic".to_string();

    let config = common::wallet_config(WalletStorageMode::Local);
    let primary_service = KaspaWalletService::new(config.clone()).expect("should construct primary wallet service");

    let created = primary_service
        .create_wallet(CreateWalletRequest {
            wallet_secret: wallet_secret.clone(),
            title: Some(primary_title.clone()),
            filename: Some(primary_filename.clone()),
            overwrite: true,
            account_name: Some("primary-account".to_string()),
            payment_secret: Some(payment_secret.clone()),
        })
        .await
        .expect("should create wallet in isolated local storage");

    assert_eq!(created.wallet_descriptor.filename, primary_filename);
    assert_eq!(common::word_count(&created.mnemonic_phrase), 24);

    let opened = primary_service
        .open_wallet(OpenWalletRequest {
            wallet_secret: wallet_secret.clone(),
            filename: Some(primary_filename.clone()),
            account_descriptors: true,
            include_legacy_accounts: true,
        })
        .await
        .expect("should reopen created wallet")
        .expect("wallet open with descriptors enabled should return account descriptors");

    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].account_id, created.account_descriptor.account_id);

    let receive_address = primary_service
        .create_receive_address(created.account_descriptor.account_id)
        .await
        .expect("should derive a receive address without requiring a live node");

    assert!(
        receive_address.address.contains(':'),
        "derived receive address should include a Kaspa address prefix separator"
    );

    let exported = primary_service
        .wallet()
        .clone()
        .wallet_export_call(WalletExportRequest {
            wallet_secret: Secret::new(wallet_secret.as_bytes().to_vec()),
            include_transactions: false,
        })
        .await
        .expect("should export wallet archive bytes");

    assert!(!exported.wallet_data.is_empty(), "wallet export should produce archive bytes");

    let mnemonic_service = KaspaWalletService::new(config.clone()).expect("should construct mnemonic import service");
    let imported_mnemonic = mnemonic_service
        .import_wallet_from_mnemonic(ImportWalletFromMnemonicRequest {
            wallet_secret: wallet_secret.clone(),
            title: Some(mnemonic_title),
            filename: Some(mnemonic_filename.clone()),
            overwrite: true,
            payment_secret: Some(payment_secret.clone()),
            mnemonic_phrase: created.mnemonic_phrase.clone(),
            account_kind: WalletAccountKind::Bip32,
        })
        .await
        .expect("should import wallet from exported mnemonic phrase");

    assert_eq!(imported_mnemonic.account_descriptor.account_id, created.account_descriptor.account_id);

    let mnemonic_opened = mnemonic_service
        .open_wallet(OpenWalletRequest {
            wallet_secret: wallet_secret.clone(),
            filename: Some(mnemonic_filename),
            account_descriptors: true,
            include_legacy_accounts: true,
        })
        .await
        .expect("should open mnemonic-imported wallet")
        .expect("mnemonic import should expose one account descriptor");

    assert_eq!(mnemonic_opened.len(), 1);
    assert_eq!(mnemonic_opened[0].account_id, created.account_descriptor.account_id);

    let primary_wallet_path = storage_dir.join(format!("{primary_filename}.wallet"));
    std::fs::remove_file(&primary_wallet_path)
        .unwrap_or_else(|err| panic!("should remove the original wallet file before archive import: {err}"));

    let archive_service = KaspaWalletService::new(config).expect("should construct archive import service");
    let imported_archive = archive_service
        .import_wallet_archive(ImportWalletArchiveRequest {
            wallet_secret: wallet_secret.clone(),
            wallet_data: exported.wallet_data,
        })
        .await
        .expect("should import wallet archive bytes");

    assert_eq!(imported_archive.wallet_descriptor.filename, primary_filename);

    let archive_opened = archive_service
        .open_wallet(OpenWalletRequest {
            wallet_secret: wallet_secret.clone(),
            filename: Some(imported_archive.wallet_descriptor.filename.clone()),
            account_descriptors: true,
            include_legacy_accounts: true,
        })
        .await
        .expect("should open archive-imported wallet")
        .expect("archive import should expose one account descriptor");

    assert_eq!(archive_opened.len(), 1);
    assert_eq!(archive_opened[0].account_id, created.account_descriptor.account_id);

    let _ = std::fs::remove_dir_all(&storage_dir);
}