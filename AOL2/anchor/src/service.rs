use futures::stream::BoxStream;

use crate::{
    config::KaspaAnchorConfig,
    error::Result,
    indexer::KaspaIndexerClient,
    node::KaspaNodeClient,
    types::{BlockHashBatch, BlockHashCursor},
    wallet::KaspaWalletService,
};

#[derive(Clone)]
pub struct KaspaAnchorService {
    node: KaspaNodeClient,
    wallet: KaspaWalletService,
    indexer: Option<KaspaIndexerClient>,
}

impl KaspaAnchorService {
    pub fn new(config: KaspaAnchorConfig) -> Result<Self> {
        let node = KaspaNodeClient::new(config.node)?;
        let wallet = KaspaWalletService::new(config.wallet)?;
        let indexer = config.indexer.map(KaspaIndexerClient::new);

        Ok(Self { node, wallet, indexer })
    }

    pub fn node(&self) -> &KaspaNodeClient {
        &self.node
    }

    pub fn wallet(&self) -> &KaspaWalletService {
        &self.wallet
    }

    pub fn indexer(&self) -> Option<&KaspaIndexerClient> {
        self.indexer.as_ref()
    }

    pub async fn best_cursor(&self) -> Result<BlockHashCursor> {
        if let Some(indexer) = self.indexer() {
            if let Some(checkpoint_hash) = indexer.checkpoint_hash().await? {
                return Ok(BlockHashCursor {
                    low_hash: Some(checkpoint_hash),
                });
            }

            if let Some(latest_block_hash) = indexer.latest_indexed_block_hash().await? {
                return Ok(BlockHashCursor {
                    low_hash: Some(latest_block_hash),
                });
            }
        }

        self.node.recommended_cursor().await
    }

    pub async fn poll_incoming_block_hashes(&self, cursor: Option<BlockHashCursor>) -> Result<BlockHashBatch> {
        let cursor = match cursor {
            Some(cursor) => cursor,
            None => self.best_cursor().await?,
        };

        self.node.get_block_hashes_since(&cursor).await
    }

    pub async fn stream_incoming_block_hashes(
        &self,
        cursor: Option<BlockHashCursor>,
    ) -> Result<BoxStream<'static, Result<BlockHashBatch>>> {
        let cursor = match cursor {
            Some(cursor) => cursor,
            None => self.best_cursor().await?,
        };

        Ok(self.node.stream_incoming_block_hashes(cursor))
    }
}