# Anchored-Overlay Layer 2 (AOL2) Formal Specification

## Abstract

The Anchored-Overlay Layer 2 (AOL2) is a stateless overlay architecture for client-driven execution in which ordinary Layer 2 operation is performed at the network edge and Layer 1 is reserved for identity, durable publication, sparse settlement, and accountability. AOL2 does not require a global sequencer, a universal mempool, or continuously replicated virtual machine state. Participants MAY execute locally and divergently; however, any artifact intended to influence a counterparty or to reach the anchor layer is admissible only if it satisfies the agreed boundary-validation rules. This document defines the baseline AOL2 architecture, conformance classes, message semantics, proof-boundary obligations, checkpoint and fraud-proof records, security properties, and profile parameters.

## Status of This Specification

This document defines the baseline AOL2 protocol and a baseline interchange profile. It is implementation-independent and intentionally does not describe any single codebase or product. Implementations and deployment profiles MAY specialize the anchor chain, transport suite, proof backend, template registry, record encoding, admission policy, and settlement rules, provided that they preserve the mandatory safety and accountability requirements defined here.

This specification does not standardize application-specific business logic, application state machines, token economics, or user interface behavior. It standardizes only the cross-application rules by which AOL2 participants identify themselves, exchange overlay messages, validate boundary artifacts, and publish sparse accountable records to an anchor layer.

## Table of Contents

1. [Scope](#scope)
2. [Conformance and Normative Language](#conformance-and-normative-language)
3. [System Roles](#system-roles)
4. [Identity and L1 Anchoring](#identity-and-l1-anchoring)
5. [Peer Discovery and Connectivity](#peer-discovery-and-connectivity)
6. [Heartbeat Gossip](#heartbeat-gossip)
7. [Session Routing](#session-routing)
8. [Relay and CGNAT Fallback](#relay-and-cgnat-fallback)
9. [Truth Engine Integration](#truth-engine-integration)
10. [Checkpoint and Fraud-Proof Anchors](#checkpoint-and-fraud-proof-anchors)
11. [Security Considerations](#security-considerations)
12. [Profile Parameters](#profile-parameters)
13. [References](#references)

## Scope

This specification defines the formal AOL2 model for node identity, overlay communication, boundary validation, sparse settlement, and accountable dispute publication.

An AOL2 deployment is characterized by the following design commitments:

- execution is performed locally by participants rather than by a continuously replicated Layer 2 virtual machine;
- acceptance is enforced at the receiving boundary by deterministic verification rules;
- ordinary overlay traffic does not depend on protocol-level block production, universal blockspace competition, or total ordering of all messages;
- Layer 1 is used as an immutable anchor for identity, sparse publication, and durable accountability rather than as a continuously executing coprocessor.

Accordingly, a conformant AOL2 profile MUST NOT require all ordinary overlay messages to be globally ordered, globally replayed, or globally executed. Profiles MAY define ordered or threshold-signed checkpoints for specific application domains, but such checkpoints are sparse artifacts rather than a continuously advancing shared ledger.

## Conformance and Normative Language

The key words MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT, SHOULD, SHOULD NOT, RECOMMENDED, NOT RECOMMENDED, MAY, and OPTIONAL in this document are to be interpreted as described in RFC 2119 and RFC 8174 when, and only when, they appear in all capitals.

### Conformance Classes

This specification defines the following conformance classes:

| Class | Obligations |
| --- | --- |
| Anchor-Layer Implementation | Publishes or verifies identity, delegation, checkpoint, and fraud-proof records on an immutable anchor layer. |
| Overlay Transport Implementation | Provides discovery, authenticated connectivity, heartbeat exchange, session routing, and relay fallback. |
| Boundary Verifier | Resolves templates or blueprints, validates proofs, applies acceptance policy, and rejects invalid boundary artifacts. |
| Full AOL2 Participant | Implements the obligations of the anchor layer, overlay transport, and boundary verifier needed for active participation in an AOL2 session. |

An implementation claiming general AOL2 participation MUST satisfy the Full AOL2 Participant class.

### Baseline Interchange Profile

Unless a deployment profile explicitly defines an alternative encoding, this specification assumes the following baseline interchange rules:

- structured records are encoded as UTF-8 JSON objects with `snake_case` member names;
- fixed-width 32-byte values are encoded as lowercase hexadecimal strings;
- arbitrary octet strings are encoded as base64url strings without padding;
- timestamps use Unix time and MUST be named according to their unit, such as `timestamp_unix_secs`, `emitted_unix_ms`, or `timestamp_ns`;
- peer and key identifiers MUST use canonical text encodings stable across transports.

Alternative encodings MAY be used only when they are explicitly specified by a profile document that preserves the same field semantics.

## System Roles

AOL2 is defined by three architectural roles and several derived operational roles.

### Architectural Roles

| Role | Function |
| --- | --- |
| Anchor Layer | Provides immutable publication, long-lived identity binding, sparse settlement, and durable accountability. |
| Overlay Layer | Provides discovery, reachability, message dissemination, liveness signaling, and repair coordination. |
| Truth Layer | Provides deterministic predicate evaluation, commitment derivation, proof generation, proof verification, and template resolution. |

### Operational Roles

| Role | Function |
| --- | --- |
| Participant | A principal that executes locally and exchanges overlay artifacts with counterparties. |
| Counterparty Verifier | A participant acting as the receiving boundary that decides whether an artifact is admissible. |
| Relay | A transport intermediary that preserves liveness when direct connectivity is unavailable. A relay transports bytes only and does not decide truth. |
| Observer | An implementation that reads checkpoints, fraud proofs, or session artifacts without participating in state advancement. |

### Non-Goals

The AOL2 base model does not define:

- a global sequencer role;
- a universal protocol-level mempool;
- a continuously advancing shared Layer 2 blockchain;
- consensus-driven execution of arbitrary user code on every node.

The absence of these components is a property of the design, not an omission.

## Identity and L1 Anchoring

### Identity Model

Each participant MUST possess at least one long-lived anchor identity key pair, denoted here as `A`. An anchor identity is the root of accountability for any sparse settlement or dispute record published to Layer 1.

An AOL2 deployment MAY allow a distinct transport or session key, denoted `T`, to act on behalf of `A`. If `T` is distinct from `A`, the participant MUST publish or otherwise present a valid delegation record binding `T` to `A`.

### Delegation Record

The baseline signed delegation record contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `delegation` |
| `anchor_id` | identifier | Long-lived accountable identity |
| `delegate_id` | identifier | Transport or session identity being authorized |
| `scope` | string or string array | Declares what the delegate may do |
| `valid_from_unix_secs` | u64 | Delegation start time |
| `valid_until_unix_secs` | u64 | Delegation expiry time |
| `nonce_hex` | 32-byte hex | Anti-replay domain value |
| `signature_b64` | signature | Signature by the anchor identity over the delegation record body |

A receiving participant MUST reject a delegated identity if any of the following is true:

- the delegation signature is invalid;
- the current time is outside the certificate validity interval;
- the requested action is outside the declared scope;
- the delegation record violates profile-defined replay-control or revocation rules.

### Anchoring Principles

The anchor layer exists to publish durable accountable artifacts, not to supersede local semantic validity checks. Consequently:

- inclusion on Layer 1 MUST be interpreted as durable publication and attribution;
- inclusion on Layer 1 MUST NOT by itself force a participant to accept a semantically invalid artifact;
- a participant MAY reject an anchored checkpoint or anchored majority statement if that artifact fails local validity checks or fails the participant's declared acceptance policy.

### Sparse Anchor Records

The anchor layer SHOULD publish only the minimum durable record classes necessary for accountability:

- identity records;
- delegation records;
- checkpoints;
- fraud proofs;
- optional reputation-bootstrap records.

Ordinary overlay messages MUST NOT require immediate Layer 1 publication.

### Sybil Resistance

Anchor identity does not, by itself, imply economic scarcity or robust Sybil resistance. A conformant AOL2 profile that depends on participant scarcity MUST define one or more of the following:

- bonding or escrow;
- allowlists or curated registries;
- rate limits;
- invitation or sponsorship;
- anchored or locally replicated reputation systems.

## Peer Discovery and Connectivity

### Network Domains

Every AOL2 deployment MUST define a `network_id` that domain-separates all overlay activity. A node MUST NOT treat a peer from a different `network_id` as belonging to the same overlay domain.

The baseline overlay identity string is:

`/aol2/p2p/net-{network_id}/1.0.0`

Profiles MAY define successor versions of the domain string. Nodes MUST reject incompatible major versions unless a profile explicitly defines cross-version interoperability.

### Bootstrap Sources

A conformant AOL2 participant MUST support at least one bootstrap source and SHOULD support more than one independent source. Common bootstrap sources include:

- static bootstrap addresses;
- cached last-known-good addresses;
- profile-defined registries or DNS seeds;
- peer introductions learned during prior sessions.

Bootstrap memory SHOULD be bounded and SHOULD retain enough recently successful addresses to improve restart liveness without becoming an unbounded trust database.

### Transport Requirements

The overlay transport MUST provide authenticated and encrypted sessions. A deployment profile MAY choose any transport suite satisfying this requirement. The following transports are RECOMMENDED for broad interoperability:

| Transport | Status |
| --- | --- |
| QUIC | RECOMMENDED |
| TCP with authenticated encryption and multiplexing | RECOMMENDED |
| WebSocket with authenticated encryption | RECOMMENDED for browser-compatible profiles |
| WebRTC-Direct | OPTIONAL |
| WebTransport | OPTIONAL |

Direct peer-to-peer connectivity is preferred. A relay path MAY be used to preserve liveness when direct connectivity is unavailable.

### Connectivity Policy

An AOL2 transport implementation:

- MUST authenticate peers before admitting them to overlay channels;
- MUST reject peers outside the local `network_id` domain;
- SHOULD maintain multiple candidate paths when available;
- SHOULD prefer direct connectivity over relayed delivery;
- MUST NOT assume that discovery success implies semantic trust.

## Heartbeat Gossip

Heartbeat gossip is the baseline liveness and entropy-quality signaling channel for the overlay. It is operational telemetry, not a settlement artifact.

### Channel Identifier

The baseline channel identifier is:

`aol2/p2p/heartbeat/net-{network_id}`

### Heartbeat Record

The baseline heartbeat record contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `heartbeat` |
| `network_id` | u32 | Overlay network discriminator |
| `peer_id` | identifier | Sender identity |
| `timestamp_ns` | u64 | Best-effort Unix time in nanoseconds |
| `seed_hex` | 32-byte hex | Digest derived from `raw_jitter_b64` |
| `raw_jitter_b64` | octet string | Primary entropy or jitter sample |
| `sensor_entropy_b64` | octet string | Auxiliary entropy evidence |

### Derivation Rule

The baseline heartbeat seed is defined as:

`seed_hex = BLAKE3(raw_jitter)`

where `raw_jitter` is the decoded byte string represented by `raw_jitter_b64`.

### Structural Validity

A heartbeat is structurally valid only if all of the following hold:

- `record_type` equals `heartbeat`;
- `network_id` matches the receiver's overlay domain;
- `peer_id` is non-empty and syntactically valid under the active profile;
- `seed_hex` encodes exactly 32 bytes;
- `raw_jitter` contains at least 32 bytes;
- at least 75% of `raw_jitter` bytes are non-zero;
- `raw_jitter` contains at least 8 distinct byte values;
- `sensor_entropy` contains at least 16 bytes;
- `seed_hex` equals the BLAKE3 digest of `raw_jitter`.

Profiles MAY define an explicit freshness window for `timestamp_ns`. In the absence of such a window, timestamps are informative and MUST NOT be the sole basis for rejection.

### Operational Semantics

Accepted heartbeats MAY feed local transport telemetry, peer scoring, or local entropy floor calculations. Heartbeats MUST NOT be interpreted as proof of application validity, checkpoint finality, or dispute resolution.

Profiles SHOULD define a heartbeat interval. A period of 30 seconds is RECOMMENDED for the baseline profile.

## Session Routing

Session routing is the baseline transport for application coordination and boundary artifacts.

### Channel Identifier

The baseline channel identifier is:

`aol2/p2p/session/{namespace}/net-{network_id}`

where `namespace` is a profile-defined session namespace.

### Route Envelope

The baseline route envelope contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `session_route` |
| `network_id` | u32 | Overlay network discriminator |
| `session_namespace` | string | Session namespace |
| `sender_id` | identifier | Sending participant or delegate |
| `session_id` | string | Session identifier |
| `route_key` | string | Application-defined route discriminator |
| `kind` | enum | `announce`, `data`, or `repair_request` |
| `payload_b64` | octet string | Encoded route payload |
| `emitted_unix_ms` | u64 | Best-effort Unix time in milliseconds |

### Route Kinds

| Kind | Meaning |
| --- | --- |
| `announce` | Advertises presence, membership, or capabilities within a session namespace. |
| `data` | Carries application data or a state-affecting boundary artifact. |
| `repair_request` | Requests a repair witness, state witness, or profile-defined recovery artifact. |

### Boundary Claim Requirement

If a `data` route is intended to alter mutually accepted state, create a durable commitment, or become eligible for checkpointing, the decoded payload MUST be a Boundary Claim record.

### Boundary Claim Record

The baseline Boundary Claim record contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `boundary_claim` |
| `transition_id_hex` | 32-byte hex | Unique transition identifier |
| `previous_state_root_hex` | optional 32-byte hex | Parent state reference, if one exists |
| `proposed_state_root_hex` | 32-byte hex | Proposed successor commitment or state root |
| `public_claim` | JSON value | Public claim or statement to be validated |
| `blueprint_ref` | string or 32-byte hex | Template identifier, blueprint hash, or profile-defined blueprint reference |
| `proof_mode` | enum | `inline` or `reference` |
| `proof_b64` | optional octet string | Inline proof bytes when `proof_mode = inline` |
| `proof_ref` | optional string | External proof locator when `proof_mode = reference` |
| `affected_ids` | identifier array | Principals affected by the transition |
| `authorizing_ids` | identifier array | Principals whose policy approval is required |

If `proof_mode = inline`, `proof_b64` MUST be present. If `proof_mode = reference`, `proof_ref` MUST be present.

### Acceptance Rule

For any boundary artifact `b`, a conformant verifier MUST apply the following rule:

$$
\mathrm{Accept}(b) = \mathrm{DomainOK}(b) \land \mathrm{IdentityOK}(b) \land \mathrm{ProofOK}(b) \land \mathrm{PolicyOK}(b)
$$

where:

- `DomainOK` means the artifact belongs to the receiver's network, namespace, and session domain;
- `IdentityOK` means the sender is authorized directly or through a valid delegation record;
- `ProofOK` means the referenced blueprint or template resolves and the truth layer accepts the proof or claim;
- `PolicyOK` means the application-specific acceptance policy is satisfied.

If any term evaluates to false, the receiver MUST reject the artifact.

### Repair Requests

The payload of a `repair_request` SHOULD identify the requested witness by a state root, transition id, participant id, or profile-defined repair token. A deployment profile MUST define the repair-response contract, including success or failure signaling, the root or transition being answered, any witness or proof bytes returned, and any quota, pricing, or denial semantics. That response contract is profile-defined and is not part of the AOL2 core standard.

## Relay and CGNAT Fallback

### NAT Reachability States

For operational purposes, AOL2 recognizes three reachability states:

- `public`: the node is directly reachable;
- `private`: the node is not directly reachable without traversal assistance;
- `unknown`: reachability could not be determined reliably.

Nodes in the `private` or `unknown` state MAY arm relay fallback.

### Relay Semantics

A relay is a transport intermediary only. A relay:

- MAY forward overlay traffic;
- MUST NOT modify message semantics;
- MUST NOT be treated as an authority on validity;
- MUST NOT cause an artifact to be accepted if the receiver would reject it over a direct path.

### Direct-Path Preference

An AOL2 node:

- SHOULD prefer direct connectivity whenever available;
- MAY establish relay reservations when direct connectivity is unavailable;
- SHOULD periodically attempt direct-path upgrade after relayed connectivity is established;
- SHOULD release unnecessary relay reservations when direct reachability is restored.

### Profile Obligations

Each deployment profile MUST define:

- relay reservation limits;
- relay abuse and rate-limiting policy;
- relay lease or lifetime policy;
- direct-path retry policy;
- any relay economics or accounting semantics.

## Truth Engine Integration

### Abstract Interface

The AOL2 truth layer is defined in terms of the following abstract operations:

| Operation | Semantics |
| --- | --- |
| `compile(template)` | Resolves a built-in template identifier or template definition into an opaque blueprint. |
| `commit(secret, salt)` | Produces a deterministic commitment for a secret and salt pair. |
| `prove(claim, salt, blueprint)` | Produces a proof that a claim satisfies the blueprint's predicates. |
| `verify(proof, blueprint)` | Returns `true` if the proof is valid for the blueprint; otherwise `false`. |
| `open(secret, salt)` | Reconstructs the commitment value for the same secret and salt. |

The abstract interface MAY be implemented by a single façade API or by equivalent operations distributed across multiple components.

### Template Model

The baseline AOL2 template model contains the following fields:

| Field | Type | Semantics |
| --- | --- | --- |
| `template_version` | u32 | Template schema version |
| `id` | string | Template identifier |
| `title` | string | Human-readable title |
| `description` | optional string | Human-readable description |
| `allowed_anchor_kinds` | enum array | Admissible anchor kinds |
| `predicates` | predicate array | Predicate blocks evaluated by the verifier |
| `lattice_vk_seed_hex` | optional 32-byte hex | Optional profile-defined proving material reference |
| `notes` | optional string | Non-normative annotation |

The baseline anchor kinds are:

- `anchor_hash`
- `static_root`
- `timestamp_unix_secs`

The baseline predicate blocks are:

- `compare`
- `range`
- `in_set`
- `at_least`

### Verification Requirement

A conformant verifier MUST collapse any internal proof-system error, deserialization error, missing template, or verification failure into a single invalid outcome for acceptance decisions. It MUST NOT treat ambiguity or parser failure as success.

### Blueprint Protection

If a blueprint contains seed material, proving material, or any other non-public setup data, that blueprint SHOULD be protected at rest and in transit according to the security expectations of the deployment profile.

### Built-In Templates

A profile MAY publish a standard template registry. Such a registry MAY include named templates such as age-gating, balance-range claims, or application-specific eligibility rules. No single built-in template is mandatory for AOL2 conformance.

## Checkpoint and Fraud-Proof Anchors

### Sparse Settlement Principle

Checkpoints and fraud proofs are sparse accountable artifacts. They are not ordinary overlay traffic, they are not a continuously replicated Layer 2 ledger, and they are not a substitute for local boundary verification.

### Checkpoint Record

The baseline checkpoint record contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `checkpoint` |
| `network_id` | u32 | Overlay network discriminator |
| `session_namespace` | string | Session namespace |
| `session_id` | string | Session identifier |
| `checkpoint_seq` | u64 | Monotonic checkpoint number within the session |
| `parent_anchor_hash_hex` | optional 32-byte hex | Prior checkpoint reference |
| `state_root_hex` | 32-byte hex | Checkpointed state or commitment root |
| `participant_ids` | identifier array | Participant set bound to the checkpoint |
| `policy` | JSON object | Session-defined checkpoint acceptance policy |
| `truth_ref_hashes` | optional 32-byte hex array | Hashes of proofs, commitments, or blueprint references |
| `signatures` | signature array | Signatures satisfying the checkpoint policy |
| `timestamp_unix_secs` | u64 | Claimed publication time |

### Checkpoint Validity

A checkpoint is valid only if all of the following hold:

- `record_type` equals `checkpoint`;
- `network_id`, `session_namespace`, and `session_id` are unambiguous;
- `state_root_hex` encodes exactly 32 bytes;
- `checkpoint_seq` is monotonic within the session;
- the participant set is uniquely defined;
- the checkpoint policy is explicit and session-scoped;
- the included signatures satisfy that policy.

### Fraud-Proof Record

The baseline fraud-proof record contains the following fields:

| Field | Type | Requirement |
| --- | --- | --- |
| `record_type` | string | MUST equal `fraud_proof` |
| `network_id` | u32 | Overlay network discriminator |
| `session_id` | string | Challenged session |
| `reporter_id` | identifier | Reporting participant |
| `accused_ids` | identifier array | Accused participants |
| `violation_kind` | string | Profile-recognized violation category |
| `prior_agreed_root_hex` | optional 32-byte hex | Last mutually accepted root, if one exists |
| `challenged_root_hex` | 32-byte hex | Root or checkpoint under challenge |
| `evidence_hash_hex` | 32-byte hex | Digest of evidence body |
| `evidence_b64` | optional octet string | Inline evidence bytes |
| `evidence_ref` | optional string | External evidence locator |
| `reporter_signature_b64` | signature | Signature by the reporter |
| `timestamp_unix_secs` | u64 | Claimed publication time |

At least one of `evidence_b64` or `evidence_ref` MUST be present.

### Violation Registry

The baseline violation registry includes the following categories:

- `missing_required_signature`
- `invalid_proof`
- `predicate_failure`
- `conflicting_checkpoint`
- `equivocation`
- `unauthorized_delegate`
- `invalid_state_root`

Profiles MAY extend this registry but SHOULD NOT redefine existing meanings.

### Local Versus Anchored Effect

Fraud proofs MAY be shared over the overlay for immediate edge filtering and MAY be anchored to Layer 1 for durable public accountability. A participant MAY lower reputation, refuse future sessions, or reject later checkpoints based on a locally verified fraud proof even before that proof is anchored.

### Majority Policy Limitation

If a checkpoint policy accepts a threshold or majority rather than unanimity, Layer 1 MAY contain a majority-signed checkpoint that an honest participant still rejects locally. This is not an AOL2 safety failure. It is an explicit consequence of the selected checkpoint policy. Therefore, a participant MUST NOT equate mere anchor inclusion with semantic correctness.

## Security Considerations

### Boundary Validation Model

AOL2 assumes that local execution environments are mutable and untrusted. Participants MAY alter their local binaries, memory, or application state. Security therefore rests on boundary validation rather than on policing local execution.

### Safety and Liveness

AOL2 is designed primarily for safety rather than for unconditional liveness.

- Safety means that an invalid boundary artifact cannot compel acceptance by an honest verifier.
- Liveness means that a session can continue and eventually conclude.

A malicious majority MAY degrade or halt liveness by disconnecting, withholding signatures, or refusing to cooperate. A malicious majority MUST NOT be able to force an honest verifier to accept an invalid transition if the verifier correctly applies this specification.

### Predicate Completeness

The truth layer can only enforce what the predicates actually express. If an application omits a rule from its template or blueprint semantics, that rule is outside the protection boundary. Profile authors MUST treat predicate completeness as part of the security model.

### Sybil and Admission Risk

Long-lived anchor identities create accountability but do not eliminate cheap identity creation. Any deployment that depends on participant scarcity or costly abuse MUST define admission controls in its profile.

### Replay and Equivocation

Profiles SHOULD define replay windows, deduplication rules, and canonical transition identifiers. Participants SHOULD treat duplicate transitions, conflicting checkpoints, and conflicting signed statements as evidence of equivocation.

### Discovery and Relay Risk

Discovery, routing, relay fallback, and bootstrap infrastructure remain attack surfaces. Honest participants SHOULD use more than one bootstrap source, SHOULD avoid single-relay dependence, and SHOULD treat route withholding and eclipse attempts as live threats.

### Newcomer Safety

Locally accumulated reputation does not automatically transfer to newcomers. Profiles that rely on reputational memory SHOULD define a bootstrap mechanism such as anchored fraud records, trusted peer summaries, or curated registries.

### Privacy Considerations

Overlay routing, heartbeat publication, checkpoint anchoring, and fraud-proof publication may leak metadata including timing, counterparties, and session participation. Profiles SHOULD define what metadata minimization or obfuscation techniques are expected.

## Profile Parameters

Any deployment claiming AOL2 conformance MUST publish a profile document that defines at least the following parameters:

1. the anchor publication interface, anchor-chain surface, and publication finality rule;
2. the anchor identity algorithm and transport-key binding model, including whether transport keys are identical to anchor keys or authorized by delegation records;
3. the accepted transport suite and address representation;
4. the bootstrap source set and bootstrap trust model;
5. the heartbeat interval, freshness window, and local scoring policy;
6. the session namespaces, route key semantics, and replay-control policy;
7. the proof envelope and carriage mode for state-affecting messages;
8. the template registry, blueprint distribution policy, and verification backend;
9. the session checkpoint policy, including who defines it for a session and whether unanimity, threshold, or another rule applies;
10. the fraud-proof admissibility policy and evidence retention rules;
11. the relay reservation, pricing, accounting, abuse-control, and repair-response policy;
12. the reputation bootstrap policy for newcomers;
13. any alternative binary or canonical encoding if the baseline JSON profile is not used.

The profile document MAY define additional parameters, provided that those parameters do not conflict with the mandatory requirements of this specification.

## References

### Normative References

Bradner, S. (1997). *Key words for use in RFCs to indicate requirement levels* (RFC 2119). RFC Editor. https://doi.org/10.17487/RFC2119

Leiba, B. (2017). *Ambiguity of uppercase vs lowercase in RFC 2119 key words* (RFC 8174). RFC Editor. https://doi.org/10.17487/RFC8174

O'Connor, J., Aumasson, J.-P., Neves, S., & Wilcox-O'Hearn, Z. (2020). *BLAKE3* [Specification]. https://github.com/BLAKE3-team/BLAKE3-specs/blob/master/blake3.pdf

### Informative References

Nakamoto, S. (2008). *Bitcoin: A peer-to-peer electronic cash system*. https://bitcoin.org/bitcoin.pdf

Peavey Koding. (2026, May 18). *The anchored-overlay layer 2 (AOL2): A stateless overlay paradigm for client-driven execution with sparse L1 anchoring and without blockspace, fee, or security budget competition* [Unpublished manuscript].

Protocol Labs. (n.d.). *libp2p*. Retrieved May 22, 2026, from https://libp2p.io/
