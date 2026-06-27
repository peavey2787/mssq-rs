use libp2p::PeerId;
use serde::{Deserialize, Serialize};

use crate::common::utils::unix_timestamp_ns;

pub fn session_topic(network_id: u32, namespace: &str) -> String {
    format!("aol2/p2p/session/{namespace}/net-{network_id}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMessageKind {
    Announce,
    Data,
    RepairRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteMessage {
    pub sender_peer_id: String,
    pub session_id: String,
    pub route_key: String,
    pub kind: RouteMessageKind,
    pub payload: Vec<u8>,
    pub emitted_unix_ms: u64,
}

impl RouteMessage {
    #[must_use]
    pub fn new(
        sender_peer_id: PeerId,
        session_id: impl Into<String>,
        route_key: impl Into<String>,
        kind: RouteMessageKind,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            sender_peer_id: sender_peer_id.to_string(),
            session_id: session_id.into(),
            route_key: route_key.into(),
            kind,
            payload,
            emitted_unix_ms: unix_timestamp_ns() / 1_000_000,
        }
    }
}