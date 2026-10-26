# P2P collaboration threat model and trust boundary

Status: public M1 and M2.1 source checkpoint, 2026-07-30; the private identity
source integration and public relay/bootstrap client boundaries are
implemented, while production provisioning, op-hub/relay/locator deployment,
and the full real-platform acceptance matrix remain pending.

This document is the security contract for OpenPencil peer-to-peer
collaboration. It is intentionally public. The protocol, parsers, state
machines, cryptographic transport integration, resource limits, and security
tests do not rely on implementation secrecy.

## Scope and security goals

M1 is designed to connect native desktop peers on a local network by mDNS
discovery or a manually entered IP address. Document bytes travel directly
between peers.
Multicast DNS is LAN-scoped and is not a cross-region discovery mechanism.
A manually entered publicly routable address can exercise the current TCP and
Noise path across sites when firewall and NAT policy permit it, but M1 has no
rendezvous, NAT traversal, or relay and does not claim robust Internet
reachability.
M2.1 adds an owner-anchored public WSS relay path. An overseas peer may join a
signed `home_region=cn` invite through a Global L4 ingress that fixed-backhauls
to the same CN relay and locator services; being overseas does not change the
signed home region or authorize a Global-home fallback. Relay infrastructure
forwards bounded prelude, Noise handshake, and inner Noise ciphertext as an
opaque stream. It can observe admission and routing metadata at the documented
layers, but it must not decrypt or persist document frames.
ZSeven services authenticate accounts and are the production issuer boundary
for short-lived admission tickets. The private issuer and credential-bearing
provider source implementations now exist, including a strict JWKS profile,
persistent rotation ledger, Unix-HSM peer authentication, and an append-only
ABI-v2 client contract. Production HSM keys, protected static archives,
deployment policy, and hardened runners are separate provisioning gates and
are not claimed complete at this checkpoint. Identity and bootstrap services
do not store document content; relay services forward but do not decrypt or
persist it.

The implementation must:

- authenticate both ends with a server-signed collaboration ticket;
- prove that each ticket is bound to the Noise static key used by that
  connection;
- admit only the expected issuer and account subject;
- encrypt and integrity-protect all admission and document traffic;
- prevent an unauthenticated peer from receiving document, thumbnail, session
  name, or presence data;
- enforce the owner-authoritative role and edit policy;
- reject malformed, oversized, stale, replayed-state, or unsupported
  operations without partially mutating the document;
- bound connections, handshakes, frames, transfers, queues, document
  validation, and cached metadata;
- keep tickets, private keys, device credentials, document content, and
  personally identifying account data out of logs and discovery metadata.

Availability against a party that can saturate the host's network link is not
guaranteed. The limits below are intended to contain CPU and memory use after
traffic reaches OpenPencil.

## Assets

| Asset | Required protection |
| --- | --- |
| Document and presence content | Confidentiality and integrity in transit; no disclosure before admission |
| ZSeven device credential | Private implementation and platform-protected storage; never exposed through the open collaboration API |
| Collaboration signing private key | Issuer/HSM boundary only; never shipped to a client or repository |
| Collaboration bootstrap root private key | Offline signing/HSM boundary only; never held by the desktop or online op-hub service |
| Signed relay bootstrap and verified LKG | Authenticity, canonical encoding, bounded validity, rollback resistance, and atomic CN/Global endpoint/key selection |
| Short-lived collaboration ticket | Treated as a bearer credential, redacted and zeroized where owned |
| Noise X25519 static private key | Local-only, zeroized in memory, stored with platform/file protections |
| Account subject and device id | Derived only from verified claims; omitted from mDNS and routine logs |
| Collaboration display name and avatar URL | Accepted only as bounded signed claims; disclosed to admitted session participants and, on the relay path only, to the relay operator through the bearer ticket (see "Relay operator visibility") |
| Owner role and commit sequence | Owner-authoritative and guarded against guest self-assertion or stale writes |
| Resource availability | Bounded before allocation, reassembly, parsing, or broadcast |

## Trust boundaries and data flow

1. The private authentication provider uses an already authenticated device
   session to request an opaque ticket. Its public request contains only the
   local X25519 public key.
2. The ticket issuer signs the fixed public claims profile. It does not return
   signing keys to the client.
3. For a public relay session, the desktop fetches one signed collaboration
   bootstrap from a startup-configured exact HTTPS endpoint. It verifies the
   canonical payload under the embedded Ed25519 root and selects one complete
   CN or Global entry. The bootstrap mirror, DNS, TLS endpoint, invite, and peer
   cannot replace that trust root.
4. An owner publishes a route to the selected entry's locator service. A guest
   selects the entry named by the invite's signed `home_region`. Both use the
   same verified snapshot for the relay URL, locator URL and verification keys,
   and relay challenge X25519 pins.
5. mDNS advertises only an ephemeral discovery id, protocol version, and TCP
   port. Discovery is a locator, not an authentication statement.
6. Peers complete `Noise_XX_25519_ChaChaPoly_BLAKE2s`. The responder prelude is
   included in the Noise prologue.
7. Tickets are exchanged inside the encrypted Noise channel. The open verifier
   checks signature, issuer, audience, version, scope, time, identifiers, and
   the equality of `dh_pub_x25519` with the observed remote Noise static key.
   Optional `display_name` and `avatar_url` claims are accepted only from this
   signed payload; names are bounded and control-free, and avatar URLs must be
   bounded HTTPS URLs without credentials or fragments.
8. Only after both admission checks succeed may the owner send a welcome,
   snapshot, commit, or presence message.
9. The owner assigns the connection role and is the serialization point for
   accepted commits. A guest cannot acquire permissions by putting a role,
   author, subject, or device id in an untrusted message.

Neither an mDNS record, a local `SignedIn` flag, an email address, nor a
peer-supplied profile is an authentication source. Issuers that have not yet
added the optional signed profile claims remain compatible, but their peers
receive generic epoch-local labels rather than a fallback from local account
UI state.

Ticket-bearing protocol values are deliberately non-`Clone`. Generic JSON and
frame-transfer encoders reject `RenewTicket`; the dedicated sensitive encoder
serializes directly into uniquely owned zeroizing storage without first
materializing a `serde_json::Value` or ordinary output buffer. Admission,
renewal commands, chunking, decryption, reassembly, and per-peer queues retain
that ownership discipline so ticket plaintext is zeroized on every drop path.

### Domestic and overseas issuer topology

Regional credential origins are not collaboration trust roots. A domestic
client may use its local SSO for login and ticket requests while an overseas
client uses another SSO origin. Both deployments must issue the exact same
logical collaboration `iss` and the same immutable global account `sub`; email
address matching or region-local account ids are not a federation mechanism.

The desktop reads `OPENPENCIL_SSO_URL`,
`OPENPENCIL_COLLAB_ISSUER`, and
`OPENPENCIL_COLLAB_POLICY_ENDPOINT` only from trusted process-startup
configuration. Production fetches `/api/v1/collab/policy`; the envelope must
verify under an offline Ed25519 root pinned into the open client. The emergency
root transition is bounded by policy generation: the legacy union-policy root
is authorized only for generations 1-3, and the replacement union-policy root
only for generation 4 and later. The
client verifies every compile-time root and requires exactly one signature
match and exactly one generation-authorized match. A production environment
variable cannot add or replace a union-policy root. Endpoint-only
configuration, conflicting policy/JWKS endpoints, signature or issuer
mismatch, inactive key metadata, generation rollback, and same-generation
rewrites fail closed without a raw-JWKS fallback. The old
`OPENPENCIL_COLLAB_JWKS_ENDPOINT` remains an explicit self-hosted compatibility
path. No ticket, provider response, mDNS record, invite, or peer message can
supply or replace the pinned trust values.

Each region owns an independent HSM signing key hierarchy and never exports or
copies private keys to the other region. Region-local mirrors publish one
canonical offline-signed policy containing the complete public union. The
client validates no more than 8 regions/24 keys, exact region membership,
unique key ids/public keys, one active and one next key per region, and at most
one retired overlap key. It excludes unactivated next keys from ticket
verification and rechecks policy/key times on every cache use. Key publication,
activation, overlap, retirement, and emergency removal must therefore be
authorized in a higher generation before either region changes signing state.
Mirror availability, consistency, HSM provisioning, and physical multi-region
timing tests remain production gates.

### Signed collaboration bootstrap and cross-region relay routing

The desktop's former direct injection of five relay/locator endpoint and public
key values is retired. Release builds carry two built-in production hub URLs
(one per region) injected at compile time from the release pipeline's secret
store — the open-source tree contains no production endpoint, and a build
without the injection keeps the relay purely environment-configured. A
persisted user preference selects which hub serves the signed bootstrap
document, and — absent an override — which region an owner publishes in. Both
hubs serve the same signed document verified against the same embedded root set,
so the preference is a reachability choice, not a trust choice. `OPENPENCIL_COLLAB_BOOTSTRAP_URL` remains as an
operator override that wins when set and stays fail-closed on an invalid
value, and an owner may still pin
`OPENPENCIL_COLLAB_RELAY_HOME_REGION=cn|global` as a local home selector that
overrides the preference. A guest obtains its home region only from the
signed invite or the region-tagged pairing code. During the emergency
transition the embedded bootstrap root set contains the legacy
`openpencil-collab-union-root-v2` key and the independent
`openpencil-collab-bootstrap-root-v2` key. The envelope `kid` selects exactly
one of them before signature verification. The legacy root is authorized only
through bootstrap generation 2, while the successor root is authorized from
generation 3 onward. An unknown id, a signature made by the other root, or a
root outside its generation range fails closed.

The bootstrap URL must be HTTPS with the exact
`/api/v1/collaboration/bootstrap` path and no credentials, query, or fragment.
The envelope and decoded payload must each match their canonical JSON
re-encoding. Payload, signature, and public-key fields use unpadded canonical
base64url. The signature is Ed25519 over:

```text
"openpencil/op-hub/collaboration-bootstrap/v1\0" ||
canonical_payload_json_bytes
```

The domain separator prevents a valid signature for another collaboration
artifact from being interpreted as a bootstrap signature. A payload has one
non-zero generation and one validity window of at most 90 days (7,776,000
seconds), and contains exactly one CN and one Global entry in the same signed
snapshot. Each entry
atomically contains its exact relay WSS URL, locator HTTPS URL, locator
Ed25519 public keys, and relay-challenge X25519 public keys. The shared client
allows at most 300 seconds of future `not_before` clock skew, while `not_after` remains
exclusive.

The same bootstrap verifier serves desktop, mobile (`op-engine-ffi`), and the
web daemon through `op-collab-host`. Existing seven-day snapshots still verify
until their original signed expiry; increasing the maximum never revives an
expired snapshot. Clients with the old seven-day maximum must be upgraded
before publishing a longer snapshot. The envelope version, signing domain,
pinned roots, signature verification, generation checks, and 300-second future
skew limit do not change. The SSO union-policy verifier in `op-auth-bridge`
likewise accepts at most 90 days, but retains its stricter `not_before <= now`
check with no future skew allowance.

A longer signed window increases how long a captured snapshot or policy can
remain usable by a client that has not observed a newer generation. Expiry,
signature verification, and persisted generation floors remain required;
the longer ceiling does not extend an artifact's signed `not_after` value.
Renewal must issue a new generation before the existing signed expiry, and
operators must account for old clients before activating the 90-day profile.

Release order for the 90-day migration:

1. Publish an official OpenPencil revision and client packages containing both
   90-day verifiers. Passing source compatibility tests does not demonstrate
   that customers have installed the upgraded desktop or mobile clients.
2. Update `dsh-openpencil`'s `vendor/openpencil` submodule to that published
   revision. At the 2026-10-21 audit, its pinned revision was
   `20cf4316a4a4f56a653cadf627fb44c96bee09d2`, whose bootstrap and union-policy
   verifiers still cap validity at seven days. DSH builds its own
   `op-host-web-server` from this submodule, so upgrading another OpenPencil
   installation does not update the bundled daemon.
3. Rebuild, verify, and publish all six DSH runtime packages from the upgraded
   submodule: `darwin-arm64`, `darwin-x64`, `linux-arm64`, `linux-x64`,
   `win32-arm64`, and `win32-x64`. Confirm the installed packaged daemons and
   other supported clients use the compatible release.
4. Only then publish new server-side bootstrap and union-policy generations
   with the 90-day window. Until client upgrades are complete, keep published
   artifacts within the old seven-day ceiling. This checklist does not select
   an unpublished source revision or authorize changing a release pin without
   an official release.

An overseas guest joining a signed `home_region=cn` invite therefore selects
the complete CN entry from that snapshot. GeoDNS or an audited edge may land
the CN entry's logical hostnames at a Global L4 ingress and fixed-backhaul the
untouched inner TLS stream to CN. The guest must not select the Global entry
because of its physical location, combine a Global endpoint with CN keys, or
fall back to a Global home when the CN path fails. The CN locator signer and CN
relay authenticator remain the authoritative route and admission boundaries.

The desktop cache is bound to the exact bootstrap endpoint and stores the
signed body with its strong SHA-256 ETag. A still-current, freshly reverified
last-known-good snapshot may be used after a fetch/request-send failure before
a response is available, a status other than 200/304, or a valid 304 with the
exact cached ETag. A body-read failure after a 200 response, or any invalid 200
response, does not fall back to LKG. A lower generation, or different signed
payload bytes at the same generation, fails closed. An expired snapshot is not
used for routing, but remains a rollback baseline. This high-water mark exists
only in a valid persisted endpoint-bound cache: changing the bootstrap URL, or
deleting, corrupting, making unreadable, or failing to persist the cache, does
not preserve a global generation floor.

Domestic and overseas op-hub sites may serve the same byte-identical signed
envelope, but the current service does not replicate or hot-reload snapshots;
operations must deploy the same artifact to both sites. Each op-hub process
loads `OP_HUB_BOOTSTRAP_FILE`, verifies it with
`OP_HUB_BOOTSTRAP_ROOT_KEYS`, and enforces
`OP_HUB_BOOTSTRAP_MIN_GENERATION` before serving an immutable in-memory
snapshot. The online service never receives the offline bootstrap private key.

Debug plaintext is a narrow exception, not a production alternate trust path.
Only a test/debug build with
`OPENPENCIL_COLLAB_BOOTSTRAP_DEV_HTTP=1` may fetch bootstrap HTTP from a
numeric-loopback address. Additional
`OPENPENCIL_COLLAB_BOOTSTRAP_DEV_ROOT_KEYS` are considered only in that mode;
plaintext locator and relay endpoints in the snapshot must also be numeric
loopback. Unsigned relay operation separately requires
`OPENPENCIL_COLLAB_RELAY_DEV_UNSIGNED=1` in a debug build and numeric-loopback
WebSocket endpoint. `localhost`, non-loopback plaintext, and release builds are
not covered by these exceptions.

Client bootstrap does not replace service-side secret and policy
provisioning. The relay server retains
`OPENPENCIL_COLLAB_RELAY_TICKET_POLICY_FILE`,
`OPENPENCIL_COLLAB_RELAY_LOCATOR_KEYS_FILE`, and
`OPENPENCIL_COLLAB_RELAY_X25519_KEYS_FILE`; the locator server retains
`OPENPENCIL_COLLAB_LOCATOR_TICKET_POLICY_FILE`. Those files remain bounded
operator/HSM-side production inputs and are not desktop discovery settings.

## Public and private ownership

The default is open source. Code stays private only when publishing it would
expose an account credential or a production signing secret.

| Component | Repository boundary | Rationale |
| --- | --- | --- |
| Wire protocol, exact diff/apply, canonical hash, owner/guest state machines | Public `openpencil/crates/op-collab` | Reviewable deterministic behavior; wasm-compatible |
| Noise/TCP framing, admission, limits, queues, discovery, key-store interface and safe fallback | Public `openpencil/crates/op-collab-transport` | Security comes from open protocol and audited libraries |
| Ticket claims/profile, signed-union-policy and legacy JWKS parser/cache, Ed25519 verifier, provider trait, stub, ABI declarations | Public `openpencil/crates/op-auth-bridge` | Trust decisions must remain reviewable |
| Relay protocol/client/server, signed locator verifier, bootstrap verifier/cache, canonical wire and rollback tests | Public `openpencil` crates and desktop host | Endpoint, key-selection, and data-plane trust decisions must remain reviewable |
| Signed bootstrap HTTP serving and deployment | Private `op-hub` service; public signed response | The online service holds no root private key; private deployment topology is not a cryptographic control |
| Deterministic issuer and rotation fixtures | Public, compiled only for tests or `test-issuer` | Contain deliberately public seeds and a `.invalid` issuer; production verifier rejects that issuer |
| Host integration, UI, recovery, diagnostics, and smoke tests | Public `openpencil` | No credential-handling reason to hide them |
| Device token and authenticated ticket request implementation | Private `op-platform` real provider | Holds the account credential and platform storage integration |
| Ticket issuance policy, production signing/HSM keys, rotation and revocation | Private `zseven-sso` | Contains production signing authority and account policy |
| Runtime tickets, Noise private keys, device tokens, HSM material | Never committed | Secrets are runtime data, not source assets |

Public test keys are not a backup or development production key. A build that
enables `test-issuer` must still use an explicitly pinned test verifier; the
production constructor defaults to `https://sso.zseven.cn`. The desktop may
use explicit startup-pinned collaboration issuer/JWKS values for a controlled
regional deployment, independently of its credential origin.

## Threats, controls, and residual risk

### LAN interception and active man-in-the-middle

Noise XX encrypts traffic and authenticates possession of both static private
keys. The signed ticket binds the authenticated account/device claims to the
remote static key observed in that handshake. A relayed or substituted static
key therefore fails admission.

Traffic endpoints, timing, and approximate volume remain visible to the local
network. M1 does not provide anonymity or traffic-shape concealment.

### Discovery spoofing and privacy

mDNS is untrusted and spoofable. Advertisements contain no account, email,
device name, document title, session title, or stable hostname. Consumers
strictly parse the `id`, `v`, and `p` fields, cap the discovery cache, honor
removal, and proceed to Noise plus ticket authentication before disclosure.
On macOS, a live `DNSServiceGetAddrInfo` operation refreshes a 30-second
upper-layer lease every 10 seconds; removal clears it, while a dead worker ages
out within that lease. A full bounded event lane drops only the current
notification and relies on the next heartbeat to republish current state;
receiver disconnection is the condition that stops the worker.

The macOS source plist declares the canonical local-network usage description
and exactly `_openpencil-collab._tcp` in `NSBonjourServices`. Both bundle paths
patch and validate the final plist through one helper, and the release workflow
validates the exact app plist before notarization. This is a packaging/TCC
precondition, not peer authentication.

An attacker can advertise many endpoints or make connection attempts. Cache,
pending-handshake, per-IP, and active-connection limits reduce impact but
cannot prevent network-link exhaustion.

### Relay operator visibility

The relay never sees document content. The tunnel payload is a byte-for-byte
bridge of the same prelude, Noise `XX_25519_ChaChaPoly_BLAKE2s` handshake, and
encrypted records the LAN path uses, and post-pair frames are forwarded as
opaque binary without being parsed. Admission is re-established inside the
Noise channel, so peer admission never depends on the relay's view.

What the relay does see, in plaintext after its own TLS terminator, is more
than "routing metadata" in the narrow sense, and is stated here explicitly:

- **The full admission ticket**, presented as the WSS `Authorization: Bearer`
  credential and verified by the relay. Its claims carry the global account
  subject, the device id, and — when present — the display name and avatar
  URL. Cross-account collaboration is supported, so a relay operator
  reconstructs **which accounts collaborate with each other, from which
  devices, and when** — a social graph, not merely one account's device fleet.
  This is the strongest argument for minimizing the credential, and it is why
  the minimization is tracked as open work below rather than as a nicety.

  Note also that the relay reads exactly one field out of this ticket, the
  expiry it clamps the session deadline to. The subject, device id, `jti`,
  display name, and avatar URL are format-checked and then discarded — they
  are never compared, stored, or returned by the authorization path. The
  disclosure is therefore gratuitous rather than load-bearing.
- **The client hello**: route id, role, caller device X25519 public key,
  possession proof, and the embedded signed locator (owner Noise static key,
  region, validity window, discovery id).
- **Traffic shape**: pairing times, message counts, and volume.
- **The cleartext server prelude**, which necessarily precedes the handshake
  because it is the Noise prologue. It carries `session_id` and `epoch`, which
  are stable correlation handles for the lifetime of a session — unlike the
  discovery id, which rotates every five minutes.

Residual risk: this is a disclosure to the relay operator, not to the network,
and it is inherent to operating a relay that authenticates before forwarding.
It is accepted for the first-party relay. Two mitigations were investigated. A claim-minimized relay credential that
carries no account identity is open work, and is designed: the relay's
authorization output is `(route, role, expiry)`, none of which comes from the
ticket's identity claims, so the credential can be reduced to an
audience-scoped token carrying only issuer, audience, version, scope, the
channel binding to the caller's X25519 key, and the time bounds. Two honest
limits on what that buys. First, it de-identifies but does not make the relay's
view unlinkable: the device's X25519 static key is persistent and travels in
cleartext in every hello, so the operator still builds a permanent *device*
graph — it simply can no longer name the nodes or join them to the account
namespace used by support, billing, and every other first-party service.
Second, an operator who runs both the issuer and the relay can re-identify a
connection by joining on the channel-binding key and the issuance timestamp, so
against the first party the change is close to symbolic; its real value is
against relay compromise, log leakage, a third-party or regional relay
operator, and relay-scoped lawful-access requests. Implementation is blocked on
the private provider ABI, which mints the credential.
Replacing the cleartext `session_id` in the prologue with a binding commitment
was prototyped and **rejected**: the prologue is not merely a binding, it is the
only channel by which a guest learns the session id at all. The LAN join path
reads it straight out of the prelude, the mDNS record deliberately carries no
session identifier, and the relay invite carries only the locator and route
capability. A commitment would therefore require publishing the session id
somewhere a guest can reach before connecting — on LAN that means the mDNS
record, which trades a handle visible to one relay operator for a handle
broadcast to the whole local network. The marginal gain was also narrower than
it first appeared: the relay already holds the `discovery_id` inside the signed
locator for the whole session, so only cross-epoch correlation would have been
closed. Closing this properly needs the session id added to the invite and a
different LAN join handshake — a product change, not a protocol tweak. Until then, the deployment requirement is
that relay and locator operators must not persist bearer headers, hello bytes,
or client addresses beyond what an incident needs, and must not log them at
all: no relay or locator code path interpolates ticket, key, or identity
material into a log statement, and both binaries clamp their log filter to
their own crate so dependency traces cannot be turned on to dump wire data.

### Invite disclosure and re-sharing

A relay invite is a bearer capability. Anyone holding it can reach the pairing
step for that route, so it is treated as a secret: it is never placed in a URL
path or query, its pairing window is capped at one hour, and it is bound to the
signed locator's region and validity window. It does **not** by itself grant
admission — the owner must still approve each guest, and the guest must still
present a valid ticket bound to its own device key inside the Noise channel.

Residual risk: a leaked or forwarded invite lets an unintended party consume
pairing capacity and reach the owner's approval prompt, which is a social-
engineering surface (an approval prompt for a plausible-looking name) rather
than a cryptographic one. Deliberate re-sharing by an authorized participant is
out of scope, consistent with the rest of this model.

### Short pairing codes

The 10-character pairing code is the only invite surface production sessions
expose (the ~500-char `opc1_` fragment is retired from every UI; it survives
only as a hidden compatibility parse on join and in development-unsigned
loops). The full invite is sealed (encrypt-then-MAC under keys derived from
the code) and stored on the locator control plane under a `code_id` that is an
independent derivation of the code, so holding the stored blob does not reveal
the sealing key, and holding the lookup id does not reveal the code.

The first character names the relay region; a guest therefore claims from
exactly one control plane, and neither the `code_id` nor the guest's bearer
ticket is ever presented to a region that is not part of the session. The
remaining nine characters are random: 45 bits of entropy.

Brute-force controls: an online guess requires presenting a `code_id` — which
requires the code — through a ticket-authenticated, per-IP and globally
rate-limited HTTPS endpoint; every stored blob expires with the locator
pairing window (≤ 1 hour) and carries a bounded per-code claim budget.
Store-abuse controls: a slot is never released before its expiry (an
exhausted claim budget leaves a tombstone), so a code holder cannot burn the
budget and substitute their own sealed invite under the same id; entries are
attributed to the ticket-verified device key with a per-device cap, so one
account cannot squat the global capacity.

Accepted residual risk: the control-plane operator holding a sealed blob can
grind the 45-bit code space offline. Because the sealing key is symmetric,
that is an authenticity loss as well as a confidentiality loss — a grinding
operator can forge the blob a guest claims and steer the join toward a
session it controls. Two gates remain: the forged locator must verify under
the same region's signing keys (which that operator does control), and the
guest still sees the owner-identity confirmation screen before any document
data is applied, so the forgery must also survive a human check of the
displayed owner account. Owners who cannot accept operator-grindable codes
should share over LAN. Code publishing is owner-side best-effort; when it
fails the session surfaces a relay notice and remains joinable over LAN
only.

### Local process boundary during a session

The desktop exposes an HTTP JSON-RPC MCP endpoint on `127.0.0.1` so external
agent CLIs can drive the editor. During a collaboration session this endpoint
reads and writes the same document that the admission system guards, but it is
a *local* boundary: its per-instance token authenticates identity probes and
shutdown, while document tool calls are available to any process running as the
same user, and the endpoint does not check `Origin`/`Host`.

Residual risk: any local process — or, through DNS rebinding, a web page open
in a browser on the same machine — can read or mutate the shared document
without passing session admission. A `CollabGatePolicy` can reject MCP
mutations mid-session, but reads and the boundary itself remain outside the
collaboration trust model. This is accepted for a single-user desktop where a
hostile local process already has the document on disk; it is called out here
because the collaboration feature widens the blast radius from "this user's
file" to "every participant's live document". Origin/Host validation and
authenticating document tool calls are open work.

### Forged, replayed, wrong-account, or downgraded tickets

The verifier accepts only compact JWS with protected `alg=Ed25519`,
`typ=openpencil-collab+jwt`, and a bounded `kid`. Claims are exact and include
the pinned issuer/audience/version/scope, canonical subject/device ids, channel
binding, `iat`, `nbf`, `exp`, and `jti`. JWKS data is fetched only from the
pinned HTTPS endpoint and is strictly parsed as public OKP/Ed25519 verification
keys.

A stolen ticket can be used only with the bound Noise private key and until its
signed expiry. M1's revocation service-level objective is the ticket lifetime,
currently at most 15 minutes; sign-out prevents renewal but does not
retroactively invalidate an issued ticket. If immediate revocation becomes a
product requirement, it needs an online revocation epoch or introspection
mechanism.

Unknown signing keys trigger a throttled refresh. Expired JWKS cache entries
fail closed when refresh fails. Key rotation must publish overlap keys for at
least the maximum ticket lifetime and cache interval.

In a multi-region deployment, a regional ticket is accepted only after its
signing key appears in the same logical union JWKS observed by every client.
The pinned endpoint URL may be a region-local mirror, but its canonical keyset
must match every other mirror. Routing clients to partial regional keysets
would create asymmetric admission and renewal failures and is a deployment
error, never a reason to fall back to a ticket-provided key or a second issuer.

Renewal distinguishes evidence that a ticket is invalid from an unavailable
trust source. A key id absent from a successfully refreshed keyset is rejected;
transport, cache, malformed-keyset, invalid-ETag, rejected-response, and
response-limit failures publish no renewed ticket and retry only while the
previously verified ticket remains valid. Persistent failure closes the
session before the old ticket expires.

The production native fetcher propagates cancellation through cache-lock
waiting, the async HTTPS send, and every streamed body chunk. Cancellation
drops the unfinished request future, rolls back refresh/unknown-key timing
markers, closes the pending result lane, and joins the worker without waiting
for the network timeout or publishing a late result. The trait default can only
check before and after a blocking third-party fetch; any other production
blocking adapter must override the cancellable method.

### Bootstrap mirror compromise, rollback, and region confusion

A compromised op-hub mirror, DNS path, CDN, or TLS terminator can deny service,
replay bytes, or return malformed data, but cannot authorize a new endpoint or
public key without a valid domain-separated root signature. Canonical JSON and
base64url checks remove alternate encodings; bounded response, payload, region,
and key counts constrain parser and allocation work. An invalid signed time
window, weak key, unknown root, malformed ETag, lower generation, or
same-generation rewrite fails closed.

LKG is an availability control, not a trust bypass. It is usable only while its
signature and validity window still verify, and it does not conceal a malformed
200 response. Operators must nevertheless treat cache deletion, corruption,
unreadability, persistence failure, bootstrap URL changes, and unsynchronized
regional mirrors as rollback-risk events because the client has no durable
cross-endpoint global generation ledger without a valid cache. Production
rollouts must publish one byte-identical envelope to domestic and overseas
mirrors, advance the op-hub minimum-generation floor, and preserve overlapping
region keys inside the signed snapshot. Rotating the embedded root still
requires a coordinated client-and-service release. The bounded dual-root set
supports that rollout, but the production desktop does not load additional
roots from runtime configuration.

The invite's signed `home_region` is authoritative. Physical geolocation,
bootstrap mirror location, DNS answer, and edge ingress do not authorize a
different region. If an overseas client cannot reach the CN entry for a CN-home
invite, it closes or reports relay unavailable; it does not mix Global
endpoint/key material or silently create a Global-home session.

### Which accounts may pair

Collaboration is cross-account: a peer holding a valid ticket from the trusted
issuer may pair with a peer of any other account. The account is therefore no
longer the authorization, and what replaces it is deliberately **asymmetric**,
because the two sides do not have the same ability to answer "who is this?".

- **The owner accepting a guest** admits any issued account. Nothing at this
  layer decides whether the guest joins — a human does, from the approval
  prompt, which is shown the verified identity, and which the admission state
  machine makes unskippable (`Active` is reachable only through
  `OwnerAuthorized`).
- **A guest joining by invite or relay** admits any issued account, because
  the invite's signed locator has already pinned the owner's Noise static key
  and that pin is checked before admission runs. The device is authenticated
  regardless of the account behind it, which is precisely what makes joining a
  stranger's session safe.
- **A guest joining over an unpinned LAN discovery** still requires the same
  account. A guest has no approval prompt — whatever it accepts, it accepts
  silently — and on an unpinned LAN join nothing else names the peer: mDNS is
  spoofable and no key is known in advance. There the subject *is* the
  authentication, so relaxing it would let anyone on the segment holding any
  valid ticket pose as the owner, undetected.

Relaxing the account relaxes nothing else: the issuer must still be the pinned
one, the ticket must be unexpired, and it must be bound to the Noise static key
actually observed on the connection. Renewal continuity is also unchanged — a
session's issuer, subject, device id, and static key may not change mid-session.

Residual risk: the unpinned LAN path remains same-account only. Opening it needs
a way for the guest to confirm who it is joining, which is a user-facing
decision rather than a protocol change.

### Unauthorized edits and identity injection

Verified identity metadata is constructed only by the admission boundary.
Owner state assigns roles and rewrites authoritative author/sequence fields.
Viewer edits, counter gaps, stale bases, failed preconditions, unsupported
operations, and session/epoch mismatches are typed rejection paths.

The authenticated `Participant` roster wire projects only the verified display
name and HTTPS avatar URL alongside epoch-local participant/peer ids and role.
Persistent subject and device ids remain inside the non-serializable connection
principal and are never added to Welcome, presence, commit, or roster messages.
The desktop host copies the URL only into a process-local, generation-scoped
avatar registry. It is absent from the document, `EditorState`, narrowed
off-thread snapshots, and redacted `Debug`; fetched image bytes never enter the
collaboration protocol.

The owner can intentionally send document content or grant edit access; that is
the collaboration action, not an attacker bypass. M1 does not protect a
document from an authorized malicious participant after disclosure.

### Parser, memory, CPU, and queue exhaustion

Limits are typed configuration with hard maxima. They cover compact tickets,
JWKS bodies and key counts, identifiers, operation count, validation visits,
tree depth, document node count, presence, envelopes, transaction/snapshot
transfers, Noise records and handshakes, reassembly, connection counts,
per-peer/global queues, rate buckets, timeouts, commit history, and discovery
cache entries.

Lengths and counts are checked before allocating complete buffers or applying
mutations. Invalid refreshes do not replace a valid JWKS cache. Exact apply is
transactional: rejection must not leave partial document changes.

Every new externally controlled collection, string, transfer, parser, retry,
cache, or queue requires:

1. a named default and hard maximum;
2. a typed configuration or wrapper validated at construction;
3. a typed, log-safe error;
4. tests at the limit and one unit over it;
5. outbound enforcement as well as inbound enforcement.

### Local key theft and filesystem attacks

Private key buffers implement zeroization and redact `Debug`. The open
`OsKeyStore` uses macOS Keychain, Windows Credential Manager, or Linux Secret
Service for the device static key. A locked, inaccessible, ambiguous, or
malformed platform-store entry fails closed; it must not silently create a
replacement identity.

The Unix file store is a narrowly scoped fallback only when the selected
platform store has definitively reported that it is unavailable. It uses a
dedicated `0700` directory, `0600` files, no-follow opens, atomic installation,
length/all-zero checks, and reopens through the hardened read path. A locked or
temporarily inaccessible platform store is not “unavailable” and must not
trigger this fallback. The file store is not hardware-backed and cannot protect
against a compromised user account or process; platforms without the required
filesystem guarantees fail closed.

Platform key-store adapters may remain public; only their runtime secret values
are private.

### Logging, crash reports, and fixtures

Errors crossing public boundaries contain closed codes and sizes, never remote
bodies or credentials. `Debug` for ticket, key, verified identity, signed
profile, roster profile, and document containers must redact content.
Production logging must not include tickets, Noise keys, full snapshots,
document text, email, subject, device id, display name, or avatar URL.

Repository fixtures must not contain PEM private keys, production-looking
tokens, compact bearer tickets, or copied runtime key files. Deterministic test
signing material is permitted only behind the `test-issuer`/test compilation
gate and must use a non-production `.invalid` issuer.

The permanent Go-to-Rust interoperability vector follows that boundary. Its
public fixture stores the non-production compact JWS as three separate segments
and locks the segments plus public JWKS with one SHA-256 digest. The Rust test
joins and verifies them in memory; a private-repository producer test invokes
the real Go issuer service and independently locks the exact same segments,
JWKS, and digest. It contains no device token, production identity, signing
private key, HSM metadata, or authorization policy.

### Supply-chain and cryptographic downgrade

Cryptographic algorithms and protocol versions are fixed rather than selected
by a peer. Unknown versions and algorithms fail closed. Dependencies remain
covered by the repository's pinned lockfile, cargo-deny advisories/bans checks,
and targeted collaboration CI.

Changing the Noise pattern, JWS algorithm, canonical encoding, key type,
protocol version, dependency source, or maximum size is a security review
event, not a routine refactor.

Committed authentication static archives are inspectable client inputs, not a
trust boundary. The current ABI-v1 matrix is a legacy compatibility lane:
SHA-256 and the narrow C ABI are audited, but the archives still leak
source/debug/private-symbol metadata and are not claimed to be stripped or
obfuscated. Production ABI-v2 requires a private-source hardened rebuild plus
an Ed25519-signed provenance manifest covering the exact artifact hash, target,
version, ABI, source revision, build id, and hardening profile; missing or
invalid provenance fails closed. Obfuscation may raise reverse-engineering
cost, but authorization and ticket trust remain rooted in server-held signing
keys and policy.

Encrypting an archive at rest is useful only when the decryption key remains in
the private release system. Committing the key, embedding it in `build.rs`, or
shipping a client-side decryptor beside the ciphertext does not materially
raise the reverse-engineering boundary and would make a reproducible public
build misleading. A published client binary remains inspectable even when its
build input was encrypted.

## Automated boundary gate

Run:

```bash
bash tools/check-collab-security-boundaries.sh
bash tools/check-collab-security-boundaries.test.sh
bash tools/check-op-auth-prebuilt.sh
bash tools/check-op-auth-prebuilt.test.sh
bash tools/check-macos-bundle-plist.sh
bash tools/check-macos-bundle-plist.test.sh
```

The gate verifies:

- the `op-collab` wasm dependency closure contains no native transport,
  authentication, network, or random-key dependencies;
- every `op-collab*` crate and `op-auth-bridge` resolves its license from the
  MIT workspace license;
- required typed protocol, apply, transport, queue, and hard-limit anchors are
  present, and collaboration source does not return `Result<_, String>`;
- test issuer/signing fixtures remain behind explicit test feature gates;
- committed authentication archives keep their exact hashes and documented C
  ABI, while hardened ABI-v2 artifacts require signed provenance and contain
  no source paths, debug markers, or private Rust module symbols;
- high-signal private-key, token, compact-ticket, and sensitive-file patterns
  are absent;
- both macOS bundle paths and the pre-notarization release path preserve the
  exact local-network and Bonjour plist declarations;
- collaboration Rust files stay within the repository's 800-line limit.

The independent `collab-security.yml` workflow also compiles `op-collab` for
`wasm32-unknown-unknown`, runs the full protocol/state-machine/property suite,
checks transport limits and production-verifier isolation, and exercises the
two-process Noise/ticket collaboration smoke with the isolated public test
issuer.

Static scanning is defense in depth, not a secret scanner or security review
replacement. Production credentials must also be blocked by repository host
secret scanning and incident-response procedures.

## Review triggers

Request a security review when any change:

- adds or changes a claim, trust root, key source, cryptographic algorithm, or
  protocol version;
- permits document or presence data before admission completes;
- adds discovery metadata or log fields;
- changes expiry, renewal, rotation, revocation, or owner-leave behavior;
- introduces an externally controlled allocation, retry, cache, or queue;
- adds a new collaboration crate or moves code across the public/private
  boundary;
- adds a native dependency to `op-collab` or enables a new default feature;
- changes persistent private-key storage.

Security reports should identify the affected boundary and avoid attaching
live tickets, credentials, private keys, or document content.
