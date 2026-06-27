# AOL2 Implementation Guide and Roadmap

## Purpose

This document is the non-normative implementation companion to [mssq.md](./mssq.md). The formal specification defines the protocol vocabulary, record semantics, acceptance rules, and profile obligations. This guide maps those requirements onto the current repository, identifies the remaining gaps, and proposes a practical rollout order.

This document does not redefine the wire format, restate normative rules, or act as a second specification. If this guide and the formal specification ever diverge, [mssq.md](./mssq.md) wins.

## How to Use This Guide

Use the two documents for different jobs:

- read [mssq.md](./mssq.md) when defining protocol behavior, record layout, or conformance claims;
- read this guide when deciding where the behavior belongs in the repo, what is already implemented, and what still needs to be built;
- update this guide when repository structure, implementation status, or rollout order changes.

## Repository Crosswalk

| Formal concern | Primary repository surface | Current state |
| --- | --- | --- |
| Anchor identity and publication | `AOL2/anchor/`, `desktop/src-tauri/src/identity.rs`, `desktop/src-tauri/src/commands.rs` | Partial: local identity lifecycle exists; anchor publication surface is still mostly scaffold |
| Overlay transport | `AOL2/p2p/src/server.rs`, `AOL2/p2p/src/connectivity/`, `AOL2/p2p/src/protocol/` | Implemented core: discovery, gossip, NAT inference, relay arming, and route dissemination are present |
| Heartbeat signaling | `AOL2/p2p/src/protocol/pulse.rs` | Partial: heartbeat transport works, but entropy semantics are still placeholder-grade |
| Session routing and repair requests | `AOL2/p2p/src/protocol/session.rs`, `AOL2/p2p/src/server.rs` | Partial: routing exists, but state-affecting validation and repair responses are not end-to-end |
| Truth-layer verification | `AOL2/truth-engine/qssm-api/`, `AOL2/truth-engine/qssm-templates/`, related `qssm-*` crates | Implemented library surface: proving, verification, commitments, and template resolution exist |
| Desktop orchestration | `desktop/src-tauri/src/lib.rs`, `desktop/src-tauri/src/sidecar.rs`, `desktop/src-tauri/src/commands.rs` | Partial: desktop composes identity and transport, but does not yet enforce the full AOL2 boundary model |
| Sparse checkpoint and fraud publication | `AOL2/anchor/`, `desktop/src-tauri/src/sidecar.rs` | Planned: spec exists, but publication and verification flow are not implemented end-to-end |

## Spec-to-Repo Mapping

### Identity and Anchor Binding

The formal identity model lives in [mssq.md](./mssq.md#identity-and-l1-anchoring). In the repo, identity currently spans two different surfaces:

- `desktop/src-tauri/src/identity.rs` manages mnemonic-derived local identity material and encrypted storage;
- `desktop/src-tauri/src/commands.rs` exposes creation, activation, decryption, and deletion flows;
- `AOL2/anchor/` is the intended home for anchor publication, delegation-record verification, and Layer 1 interaction.

Current status:

- local identity lifecycle exists and is usable from the desktop application;
- the running `AOL2/p2p` node is not yet bound to the stored desktop identity or to a delegation record;
- the anchor crate does not yet implement publication, verification, or finality handling.

Implementation target:

- the transport runtime should use either the anchor identity directly or a transport key authorized by a delegation record;
- anchor publication logic should move into `AOL2/anchor/` rather than being implied by desktop code;
- the active deployment profile should determine chain-specific publication format and finality handling.

### Peer Discovery, Reachability, and Relay Fallback

The overlay transport responsibilities described in [mssq.md](./mssq.md#peer-discovery-and-connectivity) and [mssq.md](./mssq.md#relay-and-cgnat-fallback) are primarily implemented in `AOL2/p2p/src/server.rs` and the neighboring connectivity and protocol modules.

Current status:

- peer discovery, bootstrap dialing, session-topic publication, NAT state inference, and relay arming are present;
- direct-path upgrade mechanisms are present in the transport stack;
- relay economics, quota, operator policy, and denial semantics are not yet surfaced as first-class policy objects.

Implementation target:

- expose relay reservation policy, pricing, accounting, and abuse-control decisions through explicit config or profile objects;
- treat relay behavior as deployment policy, not as hard-coded transport trivia;
- keep relay nodes transport-only and out of the truth or settlement path.

### Heartbeat Signaling

The heartbeat section of the formal spec maps directly to `AOL2/p2p/src/protocol/pulse.rs`.

Current status:

- heartbeat publication and local density checks exist;
- current heartbeat material is still a transport placeholder based on local jitter bytes;
- the transport heartbeat path is not yet bound to the truth engine's entropy harvesting surface.

Implementation target:

- decide whether a deployment profile wants heartbeats to remain lightweight liveness signals or to become stronger entropy-linked claims;
- if stronger semantics are required, bind heartbeat generation and validation to the selected truth-layer or hardware-entropy strategy.

### Session Routing, Boundary Claims, and Replay Control

The formal state-affecting message boundary described in [mssq.md](./mssq.md#session-routing) is only partially reflected in code today.

Current status:

- `AOL2/p2p/src/protocol/session.rs` defines a generic route message envelope;
- `AOL2/p2p/src/server.rs` transports and logs payloads, and records repair requests in snapshot telemetry;
- route payloads are still treated as opaque bytes;
- state-affecting payloads are not yet decoded as boundary claims;
- replay-control policy is not yet enforced in the live route path.

Implementation target:

- decode state-affecting `data` payloads into the boundary-claim envelope defined by the formal spec;
- resolve the referenced blueprint or template through the truth layer before accepting the transition;
- add transition deduplication, nonce or sequence handling, and replay-window enforcement at the session boundary.

### Truth-Layer Integration

The truth-layer role defined in [mssq.md](./mssq.md#truth-engine-integration) is already present as a library surface under `AOL2/truth-engine/`.

Current status:

- `AOL2/truth-engine/qssm-api/` exposes `compile`, `commit`, `prove`, `verify`, and `open`;
- `AOL2/truth-engine/qssm-templates/` provides the current template and predicate vocabulary;
- the desktop app already uses `qssm-entropy` for mnemonic generation;
- the `p2p` route path does not yet invoke the truth layer to decide whether a state-affecting message is admissible.

Implementation target:

- treat `qssm-api` as the boundary-verifier facade consumed by higher-level AOL2 session logic;
- keep proof evaluation out of the transport core, but require session-boundary code to call it before accepting state transitions;
- expand profile-specific template distribution and verification policy without rewriting the transport layer.

### Checkpoints, Fraud Proofs, and Repair Flow

The sparse anchor artifacts defined in [mssq.md](./mssq.md#checkpoint-and-fraud-proof-anchors) are not yet fully wired through the repo.

Current status:

- the desktop sidecar keeps placeholder snapshot fields related to repair and witness flow;
- repair requests can be emitted and observed, but the response object is not yet defined end-to-end in code;
- checkpoint and fraud-proof publication are specified at the document level but not yet implemented through `AOL2/anchor/`.

Implementation target:

- make session checkpoint policy an explicit runtime object, not an implied convention;
- implement checkpoint and fraud-proof serialization, signing, publication, and verification in the anchor layer;
- define a repair-response object that names the answered root or transition, the returned witness or proof material, and any denial or quota semantics.

### Desktop Orchestration

The desktop application is the current integration shell for the repo.

Current status:

- `desktop/src-tauri/src/lib.rs` registers Tauri commands and runtime state;
- `desktop/src-tauri/src/sidecar.rs` runs the `p2p` sidecar and bridges network status to the UI;
- `desktop/src-tauri/src/commands.rs` exposes identity, network profile, storage, and repair-related entry points.

Implementation target:

- thread active profile selection, anchor identity, delegation choice, session checkpoint policy, and repair policy through the desktop command layer;
- keep the desktop crate as an orchestrator rather than the owner of protocol semantics.

## Known Gaps Blocking Full AOL2 Alignment

The following gaps are the main reasons the current repo is not yet a full end-to-end realization of the formal spec:

1. the live `AOL2/p2p` runtime identity is not yet bound to the stored desktop identity or to a delegation record;
2. `AOL2/anchor/` does not yet provide concrete publication, verification, or finality handling;
3. state-affecting session messages are not yet decoded and validated as boundary claims;
4. replay-control policy is not yet enforced in the route-handling path;
5. checkpoint and fraud-proof records are not yet published and verified end-to-end;
6. repair requests do not yet have a defined response contract in the running code;
7. relay pricing, accounting, and abuse-control policy are not yet surfaced as deployment-configurable objects;
8. the heartbeat path and the truth-layer entropy surface are not yet aligned for profiles that want stronger entropy semantics;
9. deployment-specific AOL2 profile documents still need to be authored.

## Recommended Rollout Order

The cleanest path from the current repo to a profile-complete AOL2 implementation is:

1. publish a concrete deployment profile that fixes anchor publication, finality, transport suite, replay-control, checkpoint policy, and relay policy;
2. bind the running transport identity to the stored desktop identity or to delegation-record-backed transport keys;
3. introduce boundary-claim decoding and truth-layer verification for state-affecting session messages;
4. implement replay control and session-scoped checkpoint policy handling;
5. implement checkpoint and fraud-proof publication and verification in `AOL2/anchor/`;
6. define and implement repair-response objects and witness delivery flow;
7. expose relay pricing, accounting, and abuse-control policy through configuration and operator surfaces.

## What This Guide Should Not Become

To keep this document useful, do not turn it back into a second spec.

- do not duplicate the field tables, acceptance equations, or conformance language from [mssq.md](./mssq.md);
- do not describe unimplemented repo ideas as if they were already protocol law;
- do not mix codebase roadmap items with formal normative requirements in the same paragraph.

When the spec changes, update [mssq.md](./mssq.md) first. When the repo changes, update this guide.