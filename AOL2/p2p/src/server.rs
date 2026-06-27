use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use libp2p::gossipsub::{self, IdentTopic};
use libp2p::swarm::SwarmEvent;
use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex};

use crate::common::error::NetError;
use crate::connectivity::peer_cache;
use crate::connectivity::relay::{update_nat_state, RelayState};
use crate::protocol::pulse::{collect_local_heartbeat, density_ok, heartbeat_topic, HeartbeatEnvelope};
use crate::protocol::reputation::ReputationStore;
use crate::protocol::session::{session_topic, RouteMessage, RouteMessageKind};
use crate::stack::{build_swarm, on_mesh_event, seed_bootstrap, MeshEvent};

#[derive(Debug, Clone)]
pub struct P2pConfig {
    pub network_id: u32,
    pub heartbeat_every: Duration,
    pub startup_peer_cache_probe: usize,
    pub history_archive: bool,
    pub bootstrap_addrs: Vec<Multiaddr>,
    pub session_namespace: String,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            network_id: 1,
            heartbeat_every: Duration::from_secs(30),
            startup_peer_cache_probe: 5,
            history_archive: false,
            bootstrap_addrs: Vec::new(),
            session_namespace: "default".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct P2pSnapshot {
    pub network_id: u32,
    pub network_label: String,
    pub peer_id: String,
    pub nat_status: String,
    pub public_addr: Option<String>,
    pub active_transports: Vec<String>,
    pub connected_peers: usize,
    pub active_relays: usize,
    pub pulses: VecDeque<String>,
    pub global_density_avg_milli: i64,
    pub real_density_avg_milli: i64,
    pub is_bootstrap_mode: bool,
    pub current_t_min_milli: i64,
    pub top_deficit_peers: Vec<String>,
    pub primary_peers: Vec<String>,
    pub governor_state: String,
    pub local_merit_tier: String,
    pub uptime_secs: u64,
    pub smt_root_hex: String,
    pub active_leases: Vec<String>,
    pub history_archive: bool,
    pub repair_peer_id: Option<String>,
    pub repair_root_hex: Option<String>,
    pub repair_proof_hex: Option<String>,
    pub fraud_alert_message: Option<String>,
}

#[derive(Clone)]
pub struct P2pHandle {
    pub peer_id: PeerId,
    pub snapshot: Arc<Mutex<P2pSnapshot>>,
    shutdown_tx: mpsc::Sender<()>,
    control_tx: mpsc::UnboundedSender<ControlMessage>,
}

#[derive(Debug, Clone)]
enum ControlMessage {
    BroadcastRoute {
        route_key: String,
        kind: RouteMessageKind,
        payload: Vec<u8>,
    },
}

impl P2pHandle {
    pub async fn shutdown(&self) {
        let _ = self.shutdown_tx.send(()).await;
    }

    pub fn broadcast_route(&self, route_key: impl Into<String>, payload: Vec<u8>) -> Result<(), String> {
        self.control_tx
            .send(ControlMessage::BroadcastRoute {
                route_key: route_key.into(),
                kind: RouteMessageKind::Data,
                payload,
            })
            .map_err(|_| "control channel closed".to_string())
    }

    pub fn request_merkle_branch(&self, peer_id: String) -> Result<(), String> {
        self.control_tx
            .send(ControlMessage::BroadcastRoute {
                route_key: "repair_request".to_string(),
                kind: RouteMessageKind::RepairRequest,
                payload: peer_id.into_bytes(),
            })
            .map_err(|_| "control channel closed".to_string())
    }
}

pub async fn start_server(cfg: P2pConfig) -> Result<P2pHandle, NetError> {
    let local_key = libp2p::identity::Keypair::generate_ed25519();
    let local_peer = PeerId::from(local_key.public());
    let (mut swarm, transport_plan) = build_swarm(local_key, cfg.network_id).await?;

    let heartbeat_topic = IdentTopic::new(heartbeat_topic(cfg.network_id));
    let heartbeat_topic_hash = heartbeat_topic.hash().clone();
    let session_topic = IdentTopic::new(session_topic(cfg.network_id, &cfg.session_namespace));
    let session_topic_hash = session_topic.hash().clone();
    let _ = swarm.behaviour_mut().gossipsub.subscribe(&heartbeat_topic);
    let _ = swarm.behaviour_mut().gossipsub.subscribe(&session_topic);

    let mut bootstrap_addrs = peer_cache::load_last_addrs(cfg.startup_peer_cache_probe);
    for addr in &cfg.bootstrap_addrs {
        if !bootstrap_addrs.contains(addr) {
            bootstrap_addrs.push(addr.clone());
        }
    }
    seed_bootstrap(&mut swarm, &bootstrap_addrs);

    let snapshot = Arc::new(Mutex::new(P2pSnapshot {
        network_id: cfg.network_id,
        network_label: network_label(cfg.network_id),
        peer_id: local_peer.to_string(),
        nat_status: "unknown".to_string(),
        public_addr: None,
        active_transports: transport_plan.active.iter().map(|name| (*name).to_string()).collect(),
        connected_peers: 0,
        active_relays: 0,
        pulses: VecDeque::new(),
        global_density_avg_milli: 1000,
        real_density_avg_milli: 1000,
        is_bootstrap_mode: true,
        current_t_min_milli: 1000,
        top_deficit_peers: Vec::new(),
        primary_peers: Vec::new(),
        governor_state: "steady".to_string(),
        local_merit_tier: "transport".to_string(),
        uptime_secs: 0,
        smt_root_hex: hex::encode([0u8; 32]),
        active_leases: Vec::new(),
        history_archive: cfg.history_archive,
        repair_peer_id: None,
        repair_root_hex: None,
        repair_proof_hex: None,
        fraud_alert_message: None,
    }));

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    let (control_tx, mut control_rx) = mpsc::unbounded_channel();
    let task_snapshot = Arc::clone(&snapshot);
    tokio::spawn(async move {
        let started_at = Instant::now();
        let mut heartbeat_tick = tokio::time::interval(cfg.heartbeat_every);
        let mut relay_state = RelayState::default();
        let mut reputation = ReputationStore::default();
        let mut live_peers = HashSet::new();

        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.recv() => {
                    break;
                }
                Some(control) = control_rx.recv() => {
                    let local_peer = swarm.local_peer_id().to_owned();
                    if let Err(err) = handle_control_message(
                        &mut swarm,
                        local_peer,
                        &session_topic,
                        &task_snapshot,
                        control,
                    ).await {
                        let mut guard = task_snapshot.lock().await;
                        guard.fraud_alert_message = Some(err.to_string());
                        push_pulse(&mut guard.pulses, format!("control publish error: {err}"));
                    }
                }
                _ = heartbeat_tick.tick() => {
                    let local_peer = swarm.local_peer_id().to_owned();
                    if let Err(err) = publish_local_heartbeat(
                        &mut swarm,
                        local_peer,
                        &heartbeat_topic,
                        &task_snapshot,
                    ).await {
                        let mut guard = task_snapshot.lock().await;
                        guard.fraud_alert_message = Some(err.to_string());
                        push_pulse(&mut guard.pulses, format!("heartbeat error: {err}"));
                    }
                    refresh_snapshot(
                        &task_snapshot,
                        &reputation,
                        &relay_state,
                        swarm.connected_peers().count(),
                        &live_peers,
                        started_at.elapsed().as_secs(),
                    ).await;
                }
                evt = swarm.select_next_some() => {
                    handle_swarm_event(
                        evt,
                        &mut swarm,
                        &task_snapshot,
                        &heartbeat_topic_hash,
                        &session_topic_hash,
                        &mut live_peers,
                        &mut relay_state,
                        &mut reputation,
                    ).await;
                    refresh_snapshot(
                        &task_snapshot,
                        &reputation,
                        &relay_state,
                        swarm.connected_peers().count(),
                        &live_peers,
                        started_at.elapsed().as_secs(),
                    ).await;
                }
            }
        }
    });

    Ok(P2pHandle {
        peer_id: local_peer,
        snapshot,
        shutdown_tx,
        control_tx,
    })
}

pub fn snapshot_to_json(snapshot: &P2pSnapshot) -> Value {
    serde_json::to_value(snapshot).unwrap_or_else(|_| json!({}))
}

pub(crate) fn network_label(network_id: u32) -> String {
    if network_id == 0 {
        "MAINNET".to_string()
    } else {
        format!("TESTNET-{network_id}")
    }
}

async fn handle_control_message(
    swarm: &mut libp2p::Swarm<crate::stack::MeshBehaviour>,
    local_peer: PeerId,
    session_topic: &IdentTopic,
    snapshot: &Arc<Mutex<P2pSnapshot>>,
    control: ControlMessage,
) -> Result<(), NetError> {
    let ControlMessage::BroadcastRoute {
        route_key,
        kind,
        payload,
    } = control;
    let message = RouteMessage::new(
        local_peer,
        format!("session-{}", crate::common::utils::unix_timestamp_ns()),
        route_key.clone(),
        kind,
        payload,
    );
    let encoded = serde_json::to_vec(&message).map_err(|err| NetError::GossipCodec(err.to_string()))?;
    let _ = swarm.behaviour_mut().gossipsub.publish(session_topic.clone(), encoded);

    let mut guard = snapshot.lock().await;
    push_pulse(
        &mut guard.pulses,
        format!("session_route published route={} bytes={}", route_key, message.payload.len()),
    );
    Ok(())
}

async fn publish_local_heartbeat(
    swarm: &mut libp2p::Swarm<crate::stack::MeshBehaviour>,
    local_peer: PeerId,
    topic: &IdentTopic,
    snapshot: &Arc<Mutex<P2pSnapshot>>,
) -> Result<(), NetError> {
    let heartbeat = collect_local_heartbeat()?;
    let envelope = HeartbeatEnvelope::from_heartbeat(local_peer, &heartbeat);
    let peer_id = envelope.peer_id.clone();
    let seed_hex = envelope.seed_hex.clone();
    let encoded = serde_json::to_vec(&envelope).map_err(|err| NetError::GossipCodec(err.to_string()))?;
    let _ = swarm.behaviour_mut().gossipsub.publish(topic.clone(), encoded);

    let mut guard = snapshot.lock().await;
    push_pulse(
        &mut guard.pulses,
        format!("heartbeat {} {}", peer_id, seed_hex),
    );
    guard.real_density_avg_milli = if density_ok(&envelope.raw_jitter) { 1000 } else { 0 };
    Ok(())
}

async fn handle_swarm_event(
    event: SwarmEvent<MeshEvent>,
    swarm: &mut libp2p::Swarm<crate::stack::MeshBehaviour>,
    snapshot: &Arc<Mutex<P2pSnapshot>>,
    heartbeat_topic_hash: &gossipsub::TopicHash,
    session_topic_hash: &gossipsub::TopicHash,
    live_peers: &mut HashSet<PeerId>,
    relay_state: &mut RelayState,
    reputation: &mut ReputationStore,
) {
    match event {
        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
            peer_cache::record_seen_addr(endpoint.get_remote_address());
            live_peers.insert(peer_id);
            let mut guard = snapshot.lock().await;
            guard.connected_peers = swarm.connected_peers().count();
            push_pulse(&mut guard.pulses, format!("connected {peer_id}"));
        }
        SwarmEvent::ConnectionClosed { peer_id, .. } => {
            if !swarm.is_connected(&peer_id) {
                live_peers.remove(&peer_id);
            }
            let mut guard = snapshot.lock().await;
            guard.connected_peers = swarm.connected_peers().count();
            push_pulse(&mut guard.pulses, format!("disconnected {peer_id}"));
        }
        SwarmEvent::NewListenAddr { address, .. } => {
            let mut guard = snapshot.lock().await;
            guard.public_addr = Some(address.to_string());
            push_pulse(&mut guard.pulses, format!("listening {address}"));
        }
        SwarmEvent::Behaviour(MeshEvent::AutoNat(event)) => {
            update_nat_state(relay_state, &event);
            let mut guard = snapshot.lock().await;
            guard.nat_status = format!("{event:?}");
            if relay_state.behind_restrictive_nat && !relay_state.reservation_attempted {
                relay_state.reservation_attempted = true;
                guard.active_relays = 1;
                guard.active_leases = vec!["relay-fallback armed".to_string()];
                push_pulse(&mut guard.pulses, "relay fallback armed".to_string());
            }
        }
        SwarmEvent::Behaviour(MeshEvent::Gossipsub(gossipsub::Event::Message {
            propagation_source,
            message,
            ..
        })) => {
            if message.topic == *heartbeat_topic_hash {
                process_heartbeat_message(snapshot, live_peers, reputation, propagation_source, &message.data).await;
            } else if message.topic == *session_topic_hash {
                process_route_message(snapshot, reputation, propagation_source, &message.data).await;
            }
        }
        SwarmEvent::Behaviour(other) => {
            on_mesh_event(swarm, &other);
        }
        _ => {}
    }
}

async fn process_heartbeat_message(
    snapshot: &Arc<Mutex<P2pSnapshot>>,
    live_peers: &mut HashSet<PeerId>,
    reputation: &mut ReputationStore,
    peer: PeerId,
    data: &[u8],
) {
    let decoded = serde_json::from_slice::<HeartbeatEnvelope>(data);
    let mut guard = snapshot.lock().await;
    match decoded {
        Ok(envelope) if envelope.is_well_formed() => {
            live_peers.insert(peer);
            reputation.accept(peer);
            push_pulse(&mut guard.pulses, format!("heartbeat <-{} {}", peer, envelope.seed_hex));
            guard.fraud_alert_message = None;
        }
        Ok(_) => {
            reputation.penalize_density(peer);
            guard.fraud_alert_message = Some(format!("invalid heartbeat from {peer}"));
            push_pulse(&mut guard.pulses, format!("heartbeat rejected {peer}"));
        }
        Err(err) => {
            guard.fraud_alert_message = Some(format!("heartbeat decode error from {peer}: {err}"));
            push_pulse(&mut guard.pulses, format!("heartbeat decode error {peer}"));
        }
    }
}

async fn process_route_message(
    snapshot: &Arc<Mutex<P2pSnapshot>>,
    reputation: &mut ReputationStore,
    peer: PeerId,
    data: &[u8],
) {
    let decoded = serde_json::from_slice::<RouteMessage>(data);
    let mut guard = snapshot.lock().await;
    match decoded {
        Ok(message) => {
            reputation.accept(peer);
            if matches!(message.kind, RouteMessageKind::RepairRequest) {
                guard.repair_peer_id = Some(peer.to_string());
            }
            push_pulse(
                &mut guard.pulses,
                format!(
                    "session <-{} route={} kind={:?} bytes={} preview={}"
                    ,
                    peer,
                    message.route_key,
                    message.kind,
                    message.payload.len(),
                    preview_bytes(&message.payload),
                ),
            );
        }
        Err(err) => {
            guard.fraud_alert_message = Some(format!("session decode error from {peer}: {err}"));
            push_pulse(&mut guard.pulses, format!("session decode error {peer}"));
        }
    }
}

async fn refresh_snapshot(
    snapshot: &Arc<Mutex<P2pSnapshot>>,
    reputation: &ReputationStore,
    relay_state: &RelayState,
    connected_peers: usize,
    live_peers: &HashSet<PeerId>,
    uptime_secs: u64,
) {
    let mut guard = snapshot.lock().await;
    let primary_peers: Vec<String> = reputation
        .top_merit_holders(8)
        .into_iter()
        .map(|peer| peer.to_string())
        .collect();
    guard.connected_peers = connected_peers;
    guard.primary_peers = primary_peers;
    guard.top_deficit_peers = Vec::new();
    guard.global_density_avg_milli = if connected_peers == 0 {
        1000
    } else {
        ((live_peers.len() as i64) * 1000 / connected_peers as i64).clamp(0, 1000)
    };
    guard.is_bootstrap_mode = connected_peers < 2;
    guard.current_t_min_milli = if relay_state.behind_restrictive_nat { 1250 } else { 1000 };
    guard.governor_state = if relay_state.behind_restrictive_nat {
        "relay-assisted".to_string()
    } else {
        "steady".to_string()
    };
    guard.local_merit_tier = if connected_peers >= 4 {
        "mesh".to_string()
    } else {
        "bootstrap".to_string()
    };
    guard.active_relays = usize::from(relay_state.reservation_attempted);
    guard.uptime_secs = uptime_secs;
}

fn push_pulse(buf: &mut VecDeque<String>, line: String) {
    buf.push_front(line);
    while buf.len() > 24 {
        let _ = buf.pop_back();
    }
}

fn preview_bytes(bytes: &[u8]) -> String {
    let limit = bytes.len().min(16);
    let mut preview = hex::encode(&bytes[..limit]);
    if bytes.len() > limit {
        preview.push_str("...");
    }
    preview
}