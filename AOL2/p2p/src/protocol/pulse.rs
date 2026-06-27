use libp2p::PeerId;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::common::error::NetError;
use crate::common::utils::unix_timestamp_ns;

pub fn heartbeat_topic(network_id: u32) -> String {
    format!("aol2/p2p/heartbeat/net-{network_id}")
}

#[derive(Debug, Clone)]
pub struct LocalHeartbeat {
    pub timestamp_ns: u64,
    pub seed: [u8; 32],
    pub raw_jitter: Vec<u8>,
    pub sensor_entropy: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatEnvelope {
    pub peer_id: String,
    pub timestamp_ns: u64,
    pub seed_hex: String,
    pub raw_jitter: Vec<u8>,
    pub sensor_entropy: Vec<u8>,
}

impl HeartbeatEnvelope {
    #[must_use]
    pub fn from_heartbeat(peer_id: PeerId, hb: &LocalHeartbeat) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            timestamp_ns: hb.timestamp_ns,
            seed_hex: hex_seed(&hb.seed),
            raw_jitter: hb.raw_jitter.clone(),
            sensor_entropy: hb.sensor_entropy.clone(),
        }
    }

    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        !self.peer_id.is_empty()
            && self.seed_hex.len() == 64
            && density_ok(&self.raw_jitter)
            && self.sensor_entropy.len() >= 16
    }
}

pub fn collect_local_heartbeat() -> Result<LocalHeartbeat, NetError> {
    let mut raw_jitter = vec![0u8; 96];
    rand::thread_rng().fill_bytes(&mut raw_jitter);

    let seed = *blake3::hash(&raw_jitter).as_bytes();
    let sensor_entropy = raw_jitter[..32].to_vec();

    Ok(LocalHeartbeat {
        timestamp_ns: unix_timestamp_ns(),
        seed,
        raw_jitter,
        sensor_entropy,
    })
}

#[must_use]
pub fn density_ok(raw_jitter: &[u8]) -> bool {
    if raw_jitter.len() < 32 {
        return false;
    }

    let non_zero = raw_jitter.iter().filter(|byte| **byte != 0).count();
    let mut distinct = [false; 256];
    for byte in raw_jitter {
        distinct[*byte as usize] = true;
    }
    let distinct_count = distinct.iter().filter(|seen| **seen).count();

    non_zero * 4 >= raw_jitter.len() * 3 && distinct_count >= 8
}

fn hex_seed(seed: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in seed {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}
