use reqwest::Client;
use serde_json::Value;

use crate::{
    config::KaspaIndexerConfig,
    error::{AnchorError, Result},
    types::{IndexerHealth, IndexerSnapshot},
};

#[derive(Clone)]
pub struct KaspaIndexerClient {
    config: KaspaIndexerConfig,
    http: Client,
}

impl KaspaIndexerClient {
    pub fn new(config: KaspaIndexerConfig) -> Self {
        Self {
            config,
            http: Client::new(),
        }
    }

    pub fn config(&self) -> &KaspaIndexerConfig {
        &self.config
    }

    pub async fn health(&self) -> Result<IndexerHealth> {
        Ok(IndexerHealth {
            payload: self.get_json("/api/health").await?,
        })
    }

    pub async fn metrics(&self) -> Result<IndexerSnapshot> {
        let metrics = self.get_json("/api/metrics").await?;

        Ok(IndexerSnapshot {
            checkpoint_hash: Self::find_first_string(
                &metrics,
                &[
                    &["checkpoint", "block", "hash"],
                    &["checkpoint", "block", "blockHash"],
                    &["checkpoint", "block", "header", "hash"],
                ],
            ),
            latest_block_hash: Self::find_first_string(
                &metrics,
                &[
                    &["components", "blockFetcher", "lastBlock", "hash"],
                    &["components", "block_fetcher", "last_block", "hash"],
                    &["checkpoint", "block", "hash"],
                ],
            ),
            metrics,
        })
    }

    pub async fn latest_indexed_block_hash(&self) -> Result<Option<String>> {
        Ok(self.metrics().await?.latest_block_hash)
    }

    pub async fn checkpoint_hash(&self) -> Result<Option<String>> {
        Ok(self.metrics().await?.checkpoint_hash)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.config.normalized_base_url(), path);
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|err| AnchorError::Http(err.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(AnchorError::Indexer(format!("indexer returned HTTP status {status}")));
        }

        response
            .json::<Value>()
            .await
            .map_err(|err| AnchorError::Json(err.to_string()))
    }

    fn find_first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
        paths
            .iter()
            .find_map(|path| Self::find_string(value, path))
    }

    fn find_string(value: &Value, path: &[&str]) -> Option<String> {
        let mut current = value;
        for key in path {
            current = current.get(*key)?;
        }
        current.as_str().map(ToOwned::to_owned)
    }
}