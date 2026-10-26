use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signer as _, SigningKey};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

use super::*;

const ISSUER: &str = "https://collab.example.com";
const NOW: u64 = 1_800_000_000;
const GO_V2_FIXTURE: &str = include_str!("../tests/fixtures/zseven-sso-go-union-policy-v2.json");
const GO_V2_GENERATION_4_FIXTURE: &[u8] =
    include_bytes!("../tests/fixtures/zseven-sso-go-union-policy-v2-generation-4.json");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GoUnionPolicyFixture {
    description: String,
    security_notice: String,
    profile_version: u32,
    now_unix_seconds: u64,
    root_x: String,
    canonical_unsigned_json: String,
    canonical_message_sha256: String,
    policy_json: String,
    legacy_v1_domain_signature: String,
}

fn sequence_key(first_byte: u8) -> SigningKey {
    let mut seed = [0_u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = first_byte + u8::try_from(index).unwrap();
    }
    SigningKey::from_bytes(&seed)
}

fn public_x(first_byte: u8) -> String {
    URL_SAFE_NO_PAD.encode(sequence_key(first_byte).verifying_key().to_bytes())
}

fn key(region: &str, kid: &str, first_byte: u8, activated: i64) -> Value {
    json!({
        "region": region,
        "kid": kid,
        "x": public_x(first_byte),
        "published_at_unix": 1_700_000_000,
        "activated_at_unix": activated,
        "retired_at_unix": 0,
        "not_after_unix": 0,
    })
}

fn policy_fixture() -> Value {
    json!({
        "version": 2,
        "generation": 7,
        "issuer": ISSUER,
        "not_before_unix": 1_799_900_000,
        "not_after_unix": 1_800_500_000,
        "required_regions": [
            {"region": "cn", "recovery_epoch": 7},
            {"region": "global", "recovery_epoch": 11},
        ],
        "keys": [
            key("cn", "active_key", 1, 1_700_000_300),
            key("cn", "next_key", 41, 0),
            key("global", "remote_active_key", 81, 1_700_000_300),
            key("global", "remote_next_key", 121, 0),
        ],
        "signature": "",
    })
}

fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[0x5a; 32])
}

fn sign_value(mut value: Value, signing_key: &SigningKey) -> Vec<u8> {
    value["signature"] = json!("");
    let wire: PolicyWire = serde_json::from_value(value.clone()).unwrap();
    let canonical = canonicalize(wire, value["issuer"].as_str().unwrap()).unwrap();
    let unsigned = serde_json::to_vec(&canonical.unsigned).unwrap();
    let mut message = POLICY_DOMAIN.to_vec();
    message.extend_from_slice(&unsigned);
    value["signature"] = json!(URL_SAFE_NO_PAD.encode(signing_key.sign(&message).to_bytes()));
    serde_json::to_vec(&value).unwrap()
}

fn parse_test_policy(value: Value, now: u64) -> Result<CollabUnionPolicy, CollabUnionPolicyError> {
    let signing_key = test_signing_key();
    let body = sign_value(value, &signing_key);
    CollabUnionPolicy::from_json_with_root(
        &body,
        64 * 1024,
        ISSUER,
        now,
        signing_key.verifying_key().to_bytes(),
    )
}

fn parse_test_policy_with_roots(
    value: Value,
    now: u64,
    signing_key: &SigningKey,
    roots: &[PolicyRoot],
) -> Result<CollabUnionPolicy, CollabUnionPolicyError> {
    let body = sign_value(value, signing_key);
    CollabUnionPolicy::from_json_with_roots(&body, 64 * 1024, ISSUER, now, roots)
}

fn parse_test_body(
    body: &[u8],
    maximum_body_bytes: usize,
    expected_issuer: &str,
    now: u64,
) -> Result<CollabUnionPolicy, CollabUnionPolicyError> {
    CollabUnionPolicy::from_json_with_root(
        body,
        maximum_body_bytes,
        expected_issuer,
        now,
        test_signing_key().verifying_key().to_bytes(),
    )
}

fn policy_with_retired() -> Value {
    let mut value = policy_fixture();
    let mut retired = key("cn", "retired_key", 5, 1_700_000_100);
    retired["retired_at_unix"] = json!(1_700_000_200);
    retired["not_after_unix"] = json!(1_800_000_100);
    value["keys"].as_array_mut().unwrap().push(retired);
    value["signature"] = json!("");
    value
}

#[test]
fn verifies_a_v2_policy_with_a_test_only_root() {
    let policy = parse_test_policy(policy_fixture(), NOW).unwrap();
    assert_eq!(policy.generation(), 7);
    assert_eq!(policy.issuer(), ISSUER);
    assert_eq!(policy.key_count(), 4);
    assert_eq!(policy.recovery_epoch("cn"), Some(7));
    assert_eq!(policy.recovery_epoch("global"), Some(11));
    assert_eq!(policy.recovery_epoch("missing"), None);
    assert_eq!(
        policy.verification_key_at("active_key", NOW),
        Some(sequence_key(1).verifying_key().to_bytes())
    );
    assert_eq!(policy.verification_key_at("next_key", NOW), None);
}

#[test]
fn verifies_the_frozen_go_production_root_fixture() {
    let fixture: GoUnionPolicyFixture = serde_json::from_str(GO_V2_FIXTURE).unwrap();
    assert!(!fixture.description.is_empty());
    assert!(fixture.security_notice.contains("no private key"));
    assert_eq!(fixture.profile_version, COLLAB_UNION_POLICY_VERSION);

    let wire: PolicyWire = serde_json::from_str(&fixture.policy_json).unwrap();
    let canonical = canonicalize(wire, ISSUER).unwrap();
    let unsigned_json = serde_json::to_string(&canonical.unsigned).unwrap();
    assert_eq!(unsigned_json, fixture.canonical_unsigned_json);

    let message = canonical_message_for_test(fixture.policy_json.as_bytes(), ISSUER).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(&message)),
        fixture.canonical_message_sha256
    );
    assert_eq!(fixture.root_x, COLLAB_UNION_POLICY_ROOT_X);
    let policy = CollabUnionPolicy::from_json(
        fixture.policy_json.as_bytes(),
        64 * 1024,
        ISSUER,
        fixture.now_unix_seconds,
    )
    .unwrap();
    assert_eq!(policy.recovery_epoch("cn"), Some(7));
    assert_eq!(policy.recovery_epoch("global"), Some(11));

    let mut legacy_domain: Value = serde_json::from_str(&fixture.policy_json).unwrap();
    legacy_domain["signature"] = json!(fixture.legacy_v1_domain_signature);
    let legacy_domain = serde_json::to_vec(&legacy_domain).unwrap();
    assert_eq!(
        CollabUnionPolicy::from_json(&legacy_domain, 64 * 1024, ISSUER, fixture.now_unix_seconds,),
        Err(CollabUnionPolicyError::InvalidSignature)
    );
}

#[test]
fn verifies_the_frozen_go_generation_four_current_root_fixture() {
    let fixture = GO_V2_GENERATION_4_FIXTURE
        .strip_suffix(b"\n")
        .unwrap_or(GO_V2_GENERATION_4_FIXTURE);
    assert_eq!(
        format!("{:x}", Sha256::digest(fixture)),
        "b02f32f7827b7d7056c97997c0f953f44ad3b18928676e9a46a8192dd059ee93"
    );
    let wire: PolicyWire = serde_json::from_slice(fixture).unwrap();
    assert_eq!(wire.generation, 4);
    let issuer = wire.issuer.clone();
    let now = u64::try_from(wire.not_before_unix).unwrap() + 1;
    let policy = CollabUnionPolicy::from_json(fixture, 64 * 1024, &issuer, now).unwrap();
    assert_eq!(policy.generation(), 4);
    assert_eq!(policy.issuer(), "https://sso.zseven.cn");
    assert_eq!(policy.recovery_epoch("cn"), Some(1));
    assert_eq!(policy.recovery_epoch("global"), Some(1));
}

#[test]
fn production_v2_policy_root_is_pinned() {
    for (encoded, expected) in [
        (
            COLLAB_UNION_POLICY_LEGACY_ROOT_X,
            "53700c011a688b8077850f1330567c265f97cd5e34c9b67aa6695a3fe8afb20c",
        ),
        (
            COLLAB_UNION_POLICY_CURRENT_ROOT_X,
            "ee695282bf7120eef385743c59cd9d8c900a182f7c518f0df0ca21891cf1809e",
        ),
    ] {
        let root = decode_fixed::<32>(encoded).unwrap();
        let mut spki = vec![
            0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
        ];
        spki.extend_from_slice(&root);
        assert_eq!(format!("{:x}", Sha256::digest(&spki)), expected);
    }
    assert_eq!(
        COLLAB_UNION_POLICY_ROOT_X,
        COLLAB_UNION_POLICY_LEGACY_ROOT_X
    );
    assert_eq!(PINNED_POLICY_ROOTS.len(), 2);
    assert_eq!(PINNED_POLICY_ROOTS[0].minimum_generation, 1);
    assert_eq!(PINNED_POLICY_ROOTS[0].maximum_generation, 3);
    assert_eq!(PINNED_POLICY_ROOTS[1].minimum_generation, 4);
    assert_eq!(PINNED_POLICY_ROOTS[1].maximum_generation, 0);
}

#[test]
fn dual_root_generation_fence_accepts_only_the_authorized_signer() {
    let legacy = SigningKey::from_bytes(&[0x31; 32]);
    let current = SigningKey::from_bytes(&[0x32; 32]);
    let roots = [
        PolicyRoot {
            key: legacy.verifying_key(),
            minimum_generation: 1,
            maximum_generation: 3,
        },
        PolicyRoot {
            key: current.verifying_key(),
            minimum_generation: 4,
            maximum_generation: 0,
        },
    ];

    for (signing_key, generation, accepted) in [
        (&legacy, 3, true),
        (&legacy, 4, false),
        (&current, 3, false),
        (&current, 4, true),
    ] {
        let mut value = policy_fixture();
        value["generation"] = json!(generation);
        let parsed = parse_test_policy_with_roots(value, NOW, signing_key, &roots);
        assert_eq!(parsed.is_ok(), accepted, "generation {generation}");
    }
}

#[test]
fn dual_root_verification_rejects_unknown_tampered_and_ambiguous_signatures() {
    let legacy = SigningKey::from_bytes(&[0x31; 32]);
    let current = SigningKey::from_bytes(&[0x32; 32]);
    let unknown = SigningKey::from_bytes(&[0x33; 32]);
    let roots = [
        PolicyRoot {
            key: legacy.verifying_key(),
            minimum_generation: 1,
            maximum_generation: 3,
        },
        PolicyRoot {
            key: current.verifying_key(),
            minimum_generation: 4,
            maximum_generation: 0,
        },
    ];

    let mut generation_four = policy_fixture();
    generation_four["generation"] = json!(4);
    assert_eq!(
        parse_test_policy_with_roots(generation_four.clone(), NOW, &unknown, &roots),
        Err(CollabUnionPolicyError::InvalidSignature)
    );

    let signed = sign_value(generation_four, &current);
    let mut tampered: Value = serde_json::from_slice(&signed).unwrap();
    tampered["required_regions"][0]["recovery_epoch"] = json!(99);
    assert_eq!(
        CollabUnionPolicy::from_json_with_roots(
            &serde_json::to_vec(&tampered).unwrap(),
            64 * 1024,
            ISSUER,
            NOW,
            &roots,
        ),
        Err(CollabUnionPolicyError::InvalidSignature)
    );

    let ambiguous = [
        PolicyRoot {
            key: legacy.verifying_key(),
            minimum_generation: 1,
            maximum_generation: 3,
        },
        PolicyRoot {
            key: legacy.verifying_key(),
            minimum_generation: 4,
            maximum_generation: 0,
        },
    ];
    let mut generation_three = policy_fixture();
    generation_three["generation"] = json!(3);
    assert_eq!(
        parse_test_policy_with_roots(generation_three, NOW, &legacy, &ambiguous),
        Err(CollabUnionPolicyError::InvalidSignature)
    );
}

#[test]
fn canonical_sorting_matches_go_for_reordered_wire_arrays() {
    let mut fixture = policy_fixture();
    fixture["required_regions"] = json!([
        {"region": "global", "recovery_epoch": 11},
        {"region": "cn", "recovery_epoch": 7},
    ]);
    fixture["keys"].as_array_mut().unwrap().reverse();
    assert!(parse_test_policy(fixture, NOW).is_ok());
}

#[test]
fn next_keys_never_verify_and_retired_overlap_expires_at_not_after() {
    let mut retired = key("cn", "retired", 3, (NOW - 200) as i64);
    retired["retired_at_unix"] = json!(NOW - 100);
    retired["not_after_unix"] = json!(NOW + 50);
    let value = json!({
        "version": 2,
        "generation": 2,
        "issuer": ISSUER,
        "not_before_unix": NOW - 100,
        "not_after_unix": NOW + 300,
        "required_regions": [{"region": "cn", "recovery_epoch": 7}],
        "keys": [
            key("cn", "active", 1, (NOW - 200) as i64),
            key("cn", "next", 2, 0),
            retired,
        ],
        "signature": "",
    });
    let policy = parse_test_policy(value, NOW).unwrap();
    assert!(policy.verification_key_at("active", NOW).is_some());
    assert!(policy.verification_key_at("retired", NOW).is_some());
    assert_eq!(policy.verification_key_at("retired", NOW + 50), None);
    assert_eq!(policy.verification_key_at("next", NOW), None);
}

#[test]
fn enforces_generation_monotonicity_and_same_generation_immutability() {
    let mut current = policy_fixture();
    current["generation"] = json!(2);
    current["signature"] = json!("");
    let current = parse_test_policy(current, NOW).unwrap();

    let mut older = policy_fixture();
    older["generation"] = json!(1);
    older["signature"] = json!("");
    let older = parse_test_policy(older, NOW).unwrap();
    assert_eq!(
        older.ensure_successor_of(&current),
        Err(CollabUnionPolicyError::GenerationRollback)
    );

    let mut rewritten = policy_fixture();
    rewritten["generation"] = json!(2);
    rewritten["required_regions"][0]["recovery_epoch"] = json!(8);
    rewritten["signature"] = json!("");
    let rewritten = parse_test_policy(rewritten, NOW).unwrap();
    assert_eq!(
        rewritten.ensure_successor_of(&current),
        Err(CollabUnionPolicyError::GenerationRewrite)
    );

    let mut newer = policy_fixture();
    newer["generation"] = json!(3);
    newer["required_regions"][0]["recovery_epoch"] = json!(8);
    newer["signature"] = json!("");
    assert!(parse_test_policy(newer, NOW)
        .unwrap()
        .ensure_successor_of(&current)
        .is_ok());
}

#[test]
fn rejects_v1_bodies_and_invalid_recovery_epoch_records() {
    let root = test_signing_key().verifying_key().to_bytes();

    let mut old_version = policy_fixture();
    old_version["version"] = json!(1);
    assert_eq!(
        CollabUnionPolicy::from_json_with_root(
            &serde_json::to_vec(&old_version).unwrap(),
            64 * 1024,
            ISSUER,
            NOW,
            root,
        ),
        Err(CollabUnionPolicyError::InvalidProfile)
    );

    let mut old_body = policy_fixture();
    old_body["version"] = json!(1);
    old_body["required_regions"] = json!(["cn", "global"]);
    assert_eq!(
        CollabUnionPolicy::from_json_with_root(
            &serde_json::to_vec(&old_body).unwrap(),
            64 * 1024,
            ISSUER,
            NOW,
            root,
        ),
        Err(CollabUnionPolicyError::MalformedJson)
    );

    let mut zero_epoch = policy_fixture();
    zero_epoch["required_regions"][0]["recovery_epoch"] = json!(0);
    assert_eq!(
        CollabUnionPolicy::from_json_with_root(
            &serde_json::to_vec(&zero_epoch).unwrap(),
            64 * 1024,
            ISSUER,
            NOW,
            root,
        ),
        Err(CollabUnionPolicyError::InvalidRegions)
    );

    let valid = serde_json::from_str::<GoUnionPolicyFixture>(GO_V2_FIXTURE)
        .unwrap()
        .policy_json;
    for body in [
        valid.replacen(",\"recovery_epoch\":7", "", 1),
        valid.replacen(
            "\"recovery_epoch\":7",
            "\"recovery_epoch\":7,\"recovery_epoch\":8",
            1,
        ),
        valid.replacen(
            "\"recovery_epoch\":7",
            "\"recovery_epoch\":7,\"unexpected\":true",
            1,
        ),
    ] {
        assert_eq!(
            CollabUnionPolicy::from_json_with_root(body.as_bytes(), 64 * 1024, ISSUER, NOW, root,),
            Err(CollabUnionPolicyError::MalformedJson)
        );
    }
}

#[test]
fn rejects_invalid_authority_profile_and_resource_bounds() {
    let valid_body = sign_value(policy_fixture(), &test_signing_key());
    assert_eq!(
        parse_test_body(&valid_body, valid_body.len() - 1, ISSUER, NOW),
        Err(CollabUnionPolicyError::InvalidBodySize)
    );
    assert_eq!(
        parse_test_body(&valid_body, 64 * 1024, "https://other.example", NOW),
        Err(CollabUnionPolicyError::InvalidIssuer)
    );
    assert_eq!(
        parse_test_body(&valid_body, 64 * 1024, ISSUER, 1_799_899_999),
        Err(CollabUnionPolicyError::Inactive)
    );
    assert_eq!(
        parse_test_body(&valid_body, 64 * 1024, ISSUER, 1_800_500_000),
        Err(CollabUnionPolicyError::Inactive)
    );

    let mut unknown = policy_fixture();
    unknown["unexpected"] = json!(true);
    assert_eq!(
        parse_test_body(
            &serde_json::to_vec(&unknown).unwrap(),
            64 * 1024,
            ISSUER,
            NOW
        ),
        Err(CollabUnionPolicyError::MalformedJson)
    );

    let mut tampered = policy_fixture();
    tampered["generation"] = json!(8);
    assert_eq!(
        parse_test_body(
            &serde_json::to_vec(&tampered).unwrap(),
            64 * 1024,
            ISSUER,
            NOW
        ),
        Err(CollabUnionPolicyError::InvalidSignature)
    );
}

#[test]
fn rejects_any_union_key_outside_its_signed_active_time() {
    let mut future_published = policy_fixture();
    future_published["keys"][0]["published_at_unix"] = json!(NOW + 1);
    future_published["signature"] = json!("");

    let mut future_activated = policy_fixture();
    future_activated["keys"][0]["activated_at_unix"] = json!(NOW + 1);
    future_activated["signature"] = json!("");

    let mut future_retired = policy_with_retired();
    future_retired["keys"][4]["retired_at_unix"] = json!(NOW + 1);
    future_retired["keys"][4]["not_after_unix"] = json!(NOW + 100);

    let mut expired_retired = policy_with_retired();
    expired_retired["keys"][4]["not_after_unix"] = json!(NOW);

    for value in [
        future_published,
        future_activated,
        future_retired,
        expired_retired,
    ] {
        assert_eq!(
            parse_test_policy(value, NOW),
            Err(CollabUnionPolicyError::Inactive)
        );
    }
}

#[test]
fn rejects_incomplete_or_ambiguous_regional_unions() {
    let cases = [
        (
            {
                let mut value = policy_fixture();
                value["required_regions"] = json!([
                    {"region": "cn", "recovery_epoch": 7},
                    {"region": "cn", "recovery_epoch": 8},
                ]);
                value
            },
            CollabUnionPolicyError::InvalidRegions,
        ),
        (
            {
                let mut value = policy_fixture();
                value["keys"].as_array_mut().unwrap().remove(1);
                value
            },
            CollabUnionPolicyError::InvalidRotationPhase,
        ),
        (
            {
                let mut value = policy_fixture();
                value["keys"][1]["x"] = value["keys"][0]["x"].clone();
                value
            },
            CollabUnionPolicyError::InvalidKeys,
        ),
        (
            {
                let mut value = policy_fixture();
                value["keys"][1]["not_after_unix"] = json!(1_900_000_000_i64);
                value
            },
            CollabUnionPolicyError::InvalidKeyLifecycle,
        ),
    ];
    for (value, expected) in cases {
        let body = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            CollabUnionPolicy::from_json(&body, 64 * 1024, ISSUER, NOW),
            Err(expected)
        );
    }
}

#[test]
fn accepts_the_maximum_eight_region_twenty_four_key_union() {
    let mut regions = Vec::new();
    let mut keys = Vec::new();
    for index in 0_u8..8 {
        let region = format!("region-{index}");
        regions.push(region.clone());
        keys.push(key(
            &region,
            &format!("{region}-active"),
            1 + index * 3,
            (NOW - 200) as i64,
        ));
        keys.push(key(&region, &format!("{region}-next"), 2 + index * 3, 0));
        let mut retired = key(
            &region,
            &format!("{region}-retired"),
            3 + index * 3,
            (NOW - 300) as i64,
        );
        retired["retired_at_unix"] = json!(NOW - 100);
        retired["not_after_unix"] = json!(NOW + 200);
        keys.push(retired);
    }
    let value = json!({
        "version": 2,
        "generation": 9,
        "issuer": ISSUER,
        "not_before_unix": NOW - 100,
        "not_after_unix": NOW + 300,
        "required_regions": regions.into_iter().enumerate().map(|(index, region)| {
            json!({"region": region, "recovery_epoch": index + 1})
        }).collect::<Vec<_>>(),
        "keys": keys,
        "signature": "",
    });
    assert_eq!(parse_test_policy(value, NOW).unwrap().key_count(), 24);
}

// Sign independent window bounds so an over-limit policy still has a valid signature.
fn signed_window(not_before: u64, not_after: u64) -> Vec<u8> {
    let wire = serde_json::from_value(policy_fixture()).unwrap();
    let mut unsigned = canonicalize(wire, ISSUER).unwrap().unsigned;
    unsigned.not_before_unix = not_before.try_into().unwrap();
    unsigned.not_after_unix = not_after.try_into().unwrap();
    let mut message = POLICY_DOMAIN.to_vec();
    message.extend_from_slice(&serde_json::to_vec(&unsigned).unwrap());
    let mut value = serde_json::to_value(unsigned).unwrap();
    value["signature"] =
        json!(URL_SAFE_NO_PAD.encode(test_signing_key().sign(&message).to_bytes()));
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn accepts_seven_and_ninety_day_policy_windows_but_rejects_one_second_more() {
    const NINETY_DAYS: u64 = 7_776_000;
    assert_eq!(MAX_COLLAB_UNION_POLICY_LIFETIME_SECONDS, NINETY_DAYS as i64);
    for lifetime in [604_800, NINETY_DAYS] {
        let body = signed_window(NOW, NOW + lifetime);
        for now in [NOW, NOW + lifetime - 1] {
            assert!(parse_test_body(&body, 64 * 1024, ISSUER, now).is_ok());
        }
        assert_eq!(
            parse_test_body(&body, 64 * 1024, ISSUER, NOW + lifetime),
            Err(CollabUnionPolicyError::Inactive)
        );
    }
    assert_eq!(
        parse_test_body(
            &signed_window(NOW, NOW + NINETY_DAYS + 1),
            64 * 1024,
            ISSUER,
            NOW
        ),
        Err(CollabUnionPolicyError::InvalidProfile)
    );
}

#[test]
fn policy_window_retains_strict_not_before_without_bootstrap_clock_skew() {
    for future_seconds in [1, 300, 301] {
        let body = signed_window(NOW + future_seconds, NOW + future_seconds + 7_776_000);
        assert_eq!(
            parse_test_body(&body, 64 * 1024, ISSUER, NOW),
            Err(CollabUnionPolicyError::Inactive)
        );
        assert!(parse_test_body(&body, 64 * 1024, ISSUER, NOW + future_seconds).is_ok());
    }
}

#[test]
fn old_policy_expiry_and_generation_floor_survive_a_ninety_day_successor() {
    let old_body = signed_window(NOW - 604_800, NOW);
    let old = parse_test_body(&old_body, 64 * 1024, ISSUER, NOW - 1).unwrap();
    assert_eq!(
        old.ensure_active_at(NOW),
        Err(CollabUnionPolicyError::Inactive)
    );
    assert_eq!(
        parse_test_body(&old_body, 64 * 1024, ISSUER, NOW),
        Err(CollabUnionPolicyError::Inactive)
    );
    for (generation, expected) in [
        (6, Err(CollabUnionPolicyError::GenerationRollback)),
        (7, Err(CollabUnionPolicyError::GenerationRewrite)),
        (8, Ok(())),
    ] {
        let mut successor = policy_fixture();
        successor["generation"] = json!(generation);
        successor["not_before_unix"] = json!(NOW);
        successor["not_after_unix"] = json!(NOW + 7_776_000);
        assert_eq!(
            parse_test_policy(successor, NOW)
                .unwrap()
                .ensure_successor_of(&old),
            expected
        );
    }
}
