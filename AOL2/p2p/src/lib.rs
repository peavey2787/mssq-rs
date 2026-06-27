//! AOL2 peer-to-peer transport: discovery, gossip, session routing, and relay-assisted reachability.
//!
//! - **`stack`**: transport build, swarm behaviour, and discovery hooks.
//! - **`connectivity`**: NAT / relay bookkeeping and peer address cache.
//! - **`protocol`**: heartbeat gossip, session routing, and peer reputation.
//! - **`server`**: Tokio orchestration for the all-in-one relay-aware p2p server.
//! - **`common`**: `NetError` and shared helpers.

#![forbid(unsafe_code)]

pub mod common;
pub mod connectivity;
pub mod protocol;
pub mod stack;

mod server;

pub use common::error::NetError;
pub use protocol::pulse::{density_ok, heartbeat_topic, HeartbeatEnvelope};
pub use protocol::session::{session_topic, RouteMessage, RouteMessageKind};
pub use server::{snapshot_to_json, start_server, P2pConfig, P2pHandle, P2pSnapshot};

pub type NodeConfig = P2pConfig;
pub type NodeHandle = P2pHandle;
pub type NodeSnapshot = P2pSnapshot;

pub async fn start_node(cfg: NodeConfig) -> Result<NodeHandle, NetError> {
    start_server(cfg).await
}
