use std::{str::FromStr, sync::Arc, time::Duration};

use futures::stream::{self, BoxStream};
use kaspa_rpc_core::{api::rpc::RpcApi, GetBlockDagInfoResponse, RpcAddress, RpcHash};
use kaspa_wrpc_client::{
    client::{ConnectOptions, ConnectStrategy},
    KaspaRpcClient, Resolver, WrpcEncoding,
};
use tokio::time::sleep;

use crate::{
    config::KaspaNodeConfig,
    error::{AnchorError, Result},
    types::{BlockHashBatch, BlockHashCursor},
};

const NODE_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct KaspaNodeClient {
    config: KaspaNodeConfig,
    client: Arc<KaspaRpcClient>,
}

impl KaspaNodeClient {
    pub fn new(config: KaspaNodeConfig) -> Result<Self> {
        let network_id = config.network.to_network_id()?;
        let resolver = config.use_public_resolver.then(Resolver::default);
        let client = Arc::new(
            KaspaRpcClient::new(WrpcEncoding::Borsh, config.url.as_deref(), resolver, Some(network_id), None)
                .map_err(|err| AnchorError::KaspaRpc(err.to_string()))?,
        );

        Ok(Self { config, client })
    }

    pub fn config(&self) -> &KaspaNodeConfig {
        &self.config
    }

    pub fn client(&self) -> &Arc<KaspaRpcClient> {
        &self.client
    }

    async fn ensure_connected(&self) -> Result<()> {
        if self.client.is_connected() {
            return Ok(());
        }

        let options = ConnectOptions {
            block_async_connect: true,
            connect_timeout: Some(NODE_CONNECT_TIMEOUT),
            strategy: ConnectStrategy::Fallback,
            ..Default::default()
        };

        self.client
            .connect(Some(options))
            .await
            .map_err(|err| AnchorError::KaspaRpc(err.to_string()))?;

        Ok(())
    }

    pub async fn block_dag_info(&self) -> Result<GetBlockDagInfoResponse> {
        self.ensure_connected().await?;
        self.client
            .get_block_dag_info()
            .await
            .map_err(|err| AnchorError::KaspaRpc(err.to_string()))
    }

    pub async fn recommended_cursor(&self) -> Result<BlockHashCursor> {
        let dag = self.block_dag_info().await?;
        Ok(BlockHashCursor {
            low_hash: Some(dag.pruning_point_hash.to_string()),
        })
    }

    pub async fn current_sink_hash(&self) -> Result<String> {
        Ok(self.block_dag_info().await?.sink.to_string())
    }

    pub async fn current_tip_hashes(&self) -> Result<Vec<String>> {
        Ok(self
            .block_dag_info()
            .await?
            .tip_hashes
            .into_iter()
            .map(|hash| hash.to_string())
            .collect())
    }

    pub async fn address_balance(&self, address: &str) -> Result<Option<u64>> {
        self.ensure_connected().await?;

        let address = RpcAddress::try_from(address).map_err(|err| AnchorError::KaspaRpc(err.to_string()))?;

        self.client
            .get_balances_by_addresses(vec![address])
            .await
            .map_err(|err| AnchorError::KaspaRpc(err.to_string()))
            .map(|entries| entries.into_iter().find_map(|entry| entry.balance))
    }

    pub async fn get_block_hashes_since(&self, cursor: &BlockHashCursor) -> Result<BlockHashBatch> {
        self.ensure_connected().await?;

        let resolved_low_hash = if let Some(low_hash) = cursor.low_hash.clone() {
            Some(low_hash)
        } else {
            self.recommended_cursor().await?.low_hash
        };

        let rpc_low_hash = match resolved_low_hash.as_deref() {
            Some(low_hash) => Some(RpcHash::from_str(low_hash).map_err(|err| AnchorError::KaspaRpc(err.to_string()))?),
            None => None,
        };

        let response = self
            .client
            .get_blocks(rpc_low_hash, false, false)
            .await
            .map_err(|err| AnchorError::KaspaRpc(err.to_string()))?;

        let block_hashes = response
            .block_hashes
            .into_iter()
            .map(|hash| hash.to_string())
            .collect::<Vec<_>>();

        let next_cursor = block_hashes.last().cloned().or(resolved_low_hash.clone());

        Ok(BlockHashBatch {
            low_hash: resolved_low_hash,
            block_hashes,
            next_cursor,
        })
    }

    pub fn stream_incoming_block_hashes(&self, cursor: BlockHashCursor) -> BoxStream<'static, Result<BlockHashBatch>> {
        let client = self.clone();

        Box::pin(stream::unfold(cursor, move |state| {
            let client = client.clone();
            async move {
                let result = client.get_block_hashes_since(&state).await;
                let next_state = match &result {
                    Ok(batch) => BlockHashCursor {
                        low_hash: batch.next_cursor.clone(),
                    },
                    Err(_) => state.clone(),
                };

                sleep(Duration::from_millis(client.config.poll_interval_ms)).await;
                Some((result, next_state))
            }
        }))
    }
}