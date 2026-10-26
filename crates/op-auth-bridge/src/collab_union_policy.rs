//! Offline-signed, cross-region collaboration verification-key policy.

use std::collections::{BTreeMap, BTreeSet};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::{
    collab_claims::valid_https_origin, CollabJwks, CollabUnionPolicyError,
    HARD_MAX_COLLAB_JWKS_BYTES,
};

pub const COLLAB_UNION_POLICY_VERSION: u32 = 2;
pub const MAX_COLLAB_UNION_POLICY_REGIONS: usize = 8;
pub const MAX_COLLAB_UNION_POLICY_KEYS: usize = 24;
pub const MAX_COLLAB_UNION_POLICY_LIFETIME_SECONDS: i64 = 90 * 24 * 60 * 60;
pub const COLLAB_UNION_POLICY_LEGACY_ROOT_X: &str = "5SVj-_jnJbuZlpDoD3M9x1eZAPDFLSq5jRb-c0xUh5A";
pub const COLLAB_UNION_POLICY_CURRENT_ROOT_X: &str = "DQJfLM6RZhfcHW52PKmzKNrubWGl0g5p3mBSKNsVOus";
/// Compatibility alias for the original generation 1-3 policy root.
pub const COLLAB_UNION_POLICY_ROOT_X: &str = COLLAB_UNION_POLICY_LEGACY_ROOT_X;

const POLICY_DOMAIN: &[u8] = b"openpencil/collab-union-policy/v2\0";
const MAX_REGION_ID_BYTES: usize = 32;
const MAX_KEY_ID_BYTES: usize = 128;

const PINNED_POLICY_ROOTS: [PolicyRootSpec; 2] = [
    PolicyRootSpec {
        public_key_x: COLLAB_UNION_POLICY_LEGACY_ROOT_X,
        minimum_generation: 1,
        maximum_generation: 3,
    },
    PolicyRootSpec {
        public_key_x: COLLAB_UNION_POLICY_CURRENT_ROOT_X,
        minimum_generation: 4,
        maximum_generation: 0,
    },
];

#[derive(Clone, Copy)]
struct PolicyRootSpec {
    public_key_x: &'static str,
    minimum_generation: u64,
    /// Zero means no upper bound.
    maximum_generation: u64,
}

#[derive(Clone)]
struct PolicyRoot {
    key: VerifyingKey,
    minimum_generation: u64,
    maximum_generation: u64,
}

/// A verified public-key union authorized by the pinned offline root.
#[derive(Clone, PartialEq, Eq)]
pub struct CollabUnionPolicy {
    generation: u64,
    issuer: String,
    not_before_unix: i64,
    not_after_unix: i64,
    recovery_epochs: BTreeMap<String, u64>,
    keyset: CollabJwks,
    verification_keys: BTreeMap<String, PolicyVerificationKey>,
    canonical_message: Vec<u8>,
}

impl CollabUnionPolicy {
    pub fn from_json(
        body: &[u8],
        maximum_body_bytes: usize,
        expected_issuer: &str,
        now_unix_seconds: u64,
    ) -> Result<Self, CollabUnionPolicyError> {
        let roots = pinned_policy_roots()?;
        Self::from_json_with_roots(
            body,
            maximum_body_bytes,
            expected_issuer,
            now_unix_seconds,
            &roots,
        )
    }

    #[cfg(test)]
    fn from_json_with_root(
        body: &[u8],
        maximum_body_bytes: usize,
        expected_issuer: &str,
        now_unix_seconds: u64,
        root: [u8; 32],
    ) -> Result<Self, CollabUnionPolicyError> {
        let root = VerifyingKey::from_bytes(&root)
            .map_err(|_| CollabUnionPolicyError::InvalidSignature)?;
        Self::from_json_with_roots(
            body,
            maximum_body_bytes,
            expected_issuer,
            now_unix_seconds,
            &[PolicyRoot {
                key: root,
                minimum_generation: 1,
                maximum_generation: 0,
            }],
        )
    }

    fn from_json_with_roots(
        body: &[u8],
        maximum_body_bytes: usize,
        expected_issuer: &str,
        now_unix_seconds: u64,
        roots: &[PolicyRoot],
    ) -> Result<Self, CollabUnionPolicyError> {
        let maximum_body_bytes = maximum_body_bytes.min(HARD_MAX_COLLAB_JWKS_BYTES);
        if body.is_empty() || body.len() > maximum_body_bytes {
            return Err(CollabUnionPolicyError::InvalidBodySize);
        }
        let wire: PolicyWire =
            serde_json::from_slice(body).map_err(|_| CollabUnionPolicyError::MalformedJson)?;
        let canonical = canonicalize(wire, expected_issuer)?;
        let unsigned_json = serde_json::to_vec(&canonical.unsigned)
            .map_err(|_| CollabUnionPolicyError::InvalidProfile)?;
        let mut message = Vec::with_capacity(POLICY_DOMAIN.len() + unsigned_json.len());
        message.extend_from_slice(POLICY_DOMAIN);
        message.extend_from_slice(&unsigned_json);

        let signature = decode_fixed::<64>(&canonical.signature)
            .ok_or(CollabUnionPolicyError::InvalidSignature)?;
        verify_policy_signature(
            canonical.unsigned.generation,
            &message,
            &Signature::from_bytes(&signature),
            roots,
        )?;

        let policy = Self {
            generation: canonical.unsigned.generation,
            issuer: canonical.unsigned.issuer,
            not_before_unix: canonical.unsigned.not_before_unix,
            not_after_unix: canonical.unsigned.not_after_unix,
            recovery_epochs: canonical
                .unsigned
                .required_regions
                .iter()
                .map(|region| (region.region.clone(), region.recovery_epoch))
                .collect(),
            keyset: CollabJwks::from_verification_keys(canonical.keys),
            verification_keys: canonical.verification_keys,
            canonical_message: message,
        };
        policy.ensure_active_at(now_unix_seconds)?;
        Ok(policy)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    pub fn key_count(&self) -> usize {
        self.keyset.len()
    }

    /// Returns the offline-authorized recovery lineage for a required region.
    pub fn recovery_epoch(&self, region: &str) -> Option<u64> {
        self.recovery_epochs.get(region).copied()
    }

    pub(crate) fn keyset(&self) -> &CollabJwks {
        &self.keyset
    }

    pub(crate) fn verification_key_at(
        &self,
        key_id: &str,
        now_unix_seconds: u64,
    ) -> Option<[u8; 32]> {
        let now = i64::try_from(now_unix_seconds).ok()?;
        let key = self.verification_keys.get(key_id)?;
        (key.activated_at_unix != 0
            && key.activated_at_unix <= now
            && (key.not_after_unix == 0 || now < key.not_after_unix))
            .then_some(key.public_key)
    }

    pub(crate) fn ensure_successor_of(&self, current: &Self) -> Result<(), CollabUnionPolicyError> {
        if self.generation < current.generation {
            return Err(CollabUnionPolicyError::GenerationRollback);
        }
        if self.generation == current.generation
            && self.canonical_message != current.canonical_message
        {
            return Err(CollabUnionPolicyError::GenerationRewrite);
        }
        Ok(())
    }

    pub(crate) fn ensure_active_at(
        &self,
        now_unix_seconds: u64,
    ) -> Result<(), CollabUnionPolicyError> {
        let now = i64::try_from(now_unix_seconds).map_err(|_| CollabUnionPolicyError::Inactive)?;
        if self.not_before_unix > now || self.not_after_unix <= now {
            return Err(CollabUnionPolicyError::Inactive);
        }
        if self.verification_keys.values().any(|key| {
            key.published_at_unix > now
                || key.activated_at_unix > now
                || key.retired_at_unix > now
                || (key.not_after_unix != 0 && key.not_after_unix <= now)
        }) {
            return Err(CollabUnionPolicyError::Inactive);
        }
        Ok(())
    }
}

fn pinned_policy_roots() -> Result<Vec<PolicyRoot>, CollabUnionPolicyError> {
    PINNED_POLICY_ROOTS
        .iter()
        .map(|spec| {
            let bytes = decode_fixed::<32>(spec.public_key_x)
                .ok_or(CollabUnionPolicyError::InvalidSignature)?;
            let key = VerifyingKey::from_bytes(&bytes)
                .map_err(|_| CollabUnionPolicyError::InvalidSignature)?;
            if spec.minimum_generation == 0
                || (spec.maximum_generation != 0
                    && spec.maximum_generation < spec.minimum_generation)
            {
                return Err(CollabUnionPolicyError::InvalidSignature);
            }
            Ok(PolicyRoot {
                key,
                minimum_generation: spec.minimum_generation,
                maximum_generation: spec.maximum_generation,
            })
        })
        .collect()
}

fn verify_policy_signature(
    generation: u64,
    message: &[u8],
    signature: &Signature,
    roots: &[PolicyRoot],
) -> Result<(), CollabUnionPolicyError> {
    let mut matching_roots = 0_u8;
    let mut authorized_roots = 0_u8;
    for root in roots {
        if root.key.verify_strict(message, signature).is_err() {
            continue;
        }
        matching_roots = matching_roots.saturating_add(1);
        if generation >= root.minimum_generation
            && (root.maximum_generation == 0 || generation <= root.maximum_generation)
        {
            authorized_roots = authorized_roots.saturating_add(1);
        }
    }
    if matching_roots != 1 || authorized_roots != 1 {
        return Err(CollabUnionPolicyError::InvalidSignature);
    }
    Ok(())
}

#[cfg(test)]
fn canonical_message_for_test(
    body: &[u8],
    expected_issuer: &str,
) -> Result<Vec<u8>, CollabUnionPolicyError> {
    let wire: PolicyWire =
        serde_json::from_slice(body).map_err(|_| CollabUnionPolicyError::MalformedJson)?;
    let canonical = canonicalize(wire, expected_issuer)?;
    let unsigned_json = serde_json::to_vec(&canonical.unsigned)
        .map_err(|_| CollabUnionPolicyError::InvalidProfile)?;
    let mut message = Vec::with_capacity(POLICY_DOMAIN.len() + unsigned_json.len());
    message.extend_from_slice(POLICY_DOMAIN);
    message.extend_from_slice(&unsigned_json);
    Ok(message)
}

impl std::fmt::Debug for CollabUnionPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CollabUnionPolicy")
            .field("generation", &self.generation)
            .field("issuer", &self.issuer)
            .field("not_before_unix", &self.not_before_unix)
            .field("not_after_unix", &self.not_after_unix)
            .field("region_count", &self.recovery_epochs.len())
            .field("key_count", &self.keyset.len())
            .finish()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyWire {
    version: u32,
    generation: u64,
    issuer: String,
    not_before_unix: i64,
    not_after_unix: i64,
    required_regions: Vec<PolicyRegion>,
    keys: Vec<PolicyKey>,
    signature: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PolicyKey {
    region: String,
    kid: String,
    x: String,
    published_at_unix: i64,
    activated_at_unix: i64,
    retired_at_unix: i64,
    not_after_unix: i64,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PolicyRegion {
    region: String,
    recovery_epoch: u64,
}

#[derive(Serialize)]
struct UnsignedPolicy {
    version: u32,
    generation: u64,
    issuer: String,
    not_before_unix: i64,
    not_after_unix: i64,
    required_regions: Vec<PolicyRegion>,
    keys: Vec<PolicyKey>,
}

struct CanonicalPolicy {
    unsigned: UnsignedPolicy,
    signature: String,
    keys: BTreeMap<String, [u8; 32]>,
    verification_keys: BTreeMap<String, PolicyVerificationKey>,
}

#[derive(Default)]
struct RegionState {
    active: usize,
    next: usize,
    retired: usize,
}

#[derive(Clone, PartialEq, Eq)]
struct PolicyVerificationKey {
    public_key: [u8; 32],
    published_at_unix: i64,
    activated_at_unix: i64,
    retired_at_unix: i64,
    not_after_unix: i64,
}

fn canonicalize(
    wire: PolicyWire,
    expected_issuer: &str,
) -> Result<CanonicalPolicy, CollabUnionPolicyError> {
    if wire.version != COLLAB_UNION_POLICY_VERSION
        || wire.generation == 0
        || wire.not_before_unix <= 0
        || wire.not_after_unix <= wire.not_before_unix
        || wire.not_after_unix - wire.not_before_unix > MAX_COLLAB_UNION_POLICY_LIFETIME_SECONDS
    {
        return Err(CollabUnionPolicyError::InvalidProfile);
    }
    if wire.issuer != expected_issuer || !valid_https_origin(&wire.issuer) {
        return Err(CollabUnionPolicyError::InvalidIssuer);
    }
    if wire.required_regions.is_empty()
        || wire.required_regions.len() > MAX_COLLAB_UNION_POLICY_REGIONS
    {
        return Err(CollabUnionPolicyError::InvalidRegions);
    }
    if wire.keys.is_empty() || wire.keys.len() > MAX_COLLAB_UNION_POLICY_KEYS {
        return Err(CollabUnionPolicyError::InvalidKeys);
    }

    let mut regions = wire.required_regions;
    regions.sort_by(|left, right| left.region.cmp(&right.region));
    if regions
        .iter()
        .any(|region| !valid_region_id(&region.region) || region.recovery_epoch == 0)
        || regions
            .windows(2)
            .any(|pair| pair[0].region == pair[1].region)
    {
        return Err(CollabUnionPolicyError::InvalidRegions);
    }
    let mut region_states = regions
        .iter()
        .map(|region| (region.region.clone(), RegionState::default()))
        .collect::<BTreeMap<_, _>>();

    let mut keys = wire.keys;
    keys.sort_by(|left, right| {
        left.kid
            .cmp(&right.kid)
            .then_with(|| left.region.cmp(&right.region))
    });
    let mut verification_keys = BTreeMap::new();
    let mut policy_verification_keys = BTreeMap::new();
    let mut public_keys = BTreeSet::new();
    for key in &keys {
        if !valid_key_id(&key.kid)
            || !valid_region_id(&key.region)
            || key.published_at_unix <= 0
            || key.activated_at_unix < 0
            || key.retired_at_unix < 0
            || key.not_after_unix < 0
        {
            return Err(CollabUnionPolicyError::InvalidKeys);
        }
        let Some(state) = region_states.get_mut(&key.region) else {
            return Err(CollabUnionPolicyError::InvalidKeys);
        };
        let public_key = decode_fixed::<32>(&key.x).ok_or(CollabUnionPolicyError::InvalidKeys)?;
        VerifyingKey::from_bytes(&public_key).map_err(|_| CollabUnionPolicyError::InvalidKeys)?;
        if verification_keys
            .insert(key.kid.clone(), public_key)
            .is_some()
            || !public_keys.insert(public_key)
        {
            return Err(CollabUnionPolicyError::InvalidKeys);
        }
        policy_verification_keys.insert(
            key.kid.clone(),
            PolicyVerificationKey {
                public_key,
                published_at_unix: key.published_at_unix,
                activated_at_unix: key.activated_at_unix,
                retired_at_unix: key.retired_at_unix,
                not_after_unix: key.not_after_unix,
            },
        );
        if (key.retired_at_unix == 0) != (key.not_after_unix == 0)
            || (key.retired_at_unix != 0
                && (key.activated_at_unix == 0
                    || key.retired_at_unix < key.activated_at_unix
                    || key.not_after_unix <= key.retired_at_unix))
        {
            return Err(CollabUnionPolicyError::InvalidKeyLifecycle);
        }
        if key.retired_at_unix != 0 {
            state.retired += 1;
        } else if key.activated_at_unix == 0 {
            state.next += 1;
        } else {
            state.active += 1;
        }
    }
    if region_states
        .values()
        .any(|state| state.active != 1 || state.next != 1 || state.retired > 1)
    {
        return Err(CollabUnionPolicyError::InvalidRotationPhase);
    }

    Ok(CanonicalPolicy {
        unsigned: UnsignedPolicy {
            version: wire.version,
            generation: wire.generation,
            issuer: wire.issuer,
            not_before_unix: wire.not_before_unix,
            not_after_unix: wire.not_after_unix,
            required_regions: regions,
            keys,
        },
        signature: wire.signature,
        keys: verification_keys,
        verification_keys: policy_verification_keys,
    })
}

fn decode_fixed<const SIZE: usize>(value: &str) -> Option<[u8; SIZE]> {
    if value.contains('=') {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    if URL_SAFE_NO_PAD.encode(&decoded) != value {
        return None;
    }
    decoded.try_into().ok()
}

fn valid_key_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_KEY_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_region_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_REGION_ID_BYTES
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

#[cfg(test)]
#[path = "collab_union_policy_tests.rs"]
mod tests;
