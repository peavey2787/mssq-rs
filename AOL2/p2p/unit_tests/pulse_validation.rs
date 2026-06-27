use libp2p::PeerId;
use p2p::protocol::pulse::{collect_local_heartbeat, density_ok};
use p2p::HeartbeatEnvelope;

#[test]
fn valid_pulse_density_accepts() {
    let hb = collect_local_heartbeat().expect("heartbeat");
    let env = HeartbeatEnvelope::from_heartbeat(PeerId::random(), &hb);
    assert!(density_ok(&env.raw_jitter));
    assert!(env.is_well_formed());
}

#[test]
fn synthetic_pulse_density_rejects() {
    assert!(!density_ok(&vec![0u8; 1024]));
}
