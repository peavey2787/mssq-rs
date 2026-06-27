# AOL2 Architecture Overview

## Runtime split

The repository is now organized around three node-facing boundaries:

1. `anchor`
	The L1 interface. It owns identity registration, PKI material, signature verification, fraud-proof anchoring, checkpoint anchoring, and any transaction the node submits to L1.
2. `p2p`
	The stateless transport layer. It owns peer discovery, gossip, session routing, NAT detection, relay fallback, and the mechanics required to connect peers that may both be behind CGNAT.
3. `truth-engine`
	The predicate and proof system. The frozen crates imported under `AOL2/truth-engine/` come from `qssm-rs`; the primary façade for node integration is `qssm-api`, with the remaining crates providing proving, verification, entropy, templates, commitments, and supporting utilities.

## Boundary rules

- `anchor` does not discover peers or route traffic.
- `p2p` does not own application state, proof semantics, or chain settlement logic.
- `truth-engine` does not manage peer connectivity or L1 submission.
- The desktop crate remains a consumer of these boundaries.

## Node flow

1. `anchor` establishes the node's accountable identity and verifies signed L1-facing actions.
2. `p2p` brings the node online, discovers peers, maintains gossip and session routes, and falls back to relays when direct paths fail.
3. `truth-engine` evaluates predicates and generates or verifies proof artifacts used by the node's higher-level application logic.
4. Resulting checkpoints, fraud proofs, or other accountable outcomes are handed back to `anchor` for anchoring on L1.

## Truth Engine import layout

The vendored `AOL2/truth-engine/` directory contains the frozen Rust crates imported from `qssm-rs` and published individually on crates.io. The current workspace includes:

- `qssm-api`
- `qssm-core`
- `qssm-entropy`
- `qssm-gadget`
- `qssm-le`
- `qssm-local-prover`
- `qssm-local-verifier`
- `qssm-ms`
- `qssm-proofs`
- `qssm-templates`
- `qssm-utils`

## Operational intent

The transport stack is intentionally generic so it can serve as an all-in-one p2p server for two-party or multi-party communication. Direct paths are preferred, but relay-assisted delivery is available when both peers are behind restrictive NAT or CGNAT.
