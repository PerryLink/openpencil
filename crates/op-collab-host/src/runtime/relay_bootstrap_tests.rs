use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener};
use std::sync::mpsc;
use std::thread;

use ed25519_dalek::{Signer as _, SigningKey};

use super::*;

pub(super) const NOW: u64 = 1_900_000_000;

fn key(kid: &str, bytes: [u8; 32]) -> BootstrapKey {
    BootstrapKey {
        kid: kid.to_owned(),
        x: URL_SAFE_NO_PAD.encode(bytes),
    }
}

fn noncanonical_ed25519_encoding() -> [u8; 32] {
    for y in 2_u8..=18 {
        for sign in [0_u8, 0x80] {
            let mut encoded = [0xff; 32];
            encoded[0] = 0xed + y;
            encoded[31] = 0x7f | sign;
            if VerifyingKey::from_bytes(&encoded).is_ok_and(|key| !key.is_weak()) {
                return encoded;
            }
        }
    }
    panic!("dalek accepted no non-canonical non-weak Ed25519 encoding")
}

pub(super) fn valid_payload() -> BootstrapPayload {
    let locator_cn = SigningKey::from_bytes(&[11; 32]);
    let locator_global = SigningKey::from_bytes(&[12; 32]);
    let relay_cn = DeviceStaticKey::from_private([21; 32]).unwrap();
    let relay_global = DeviceStaticKey::from_private([22; 32]).unwrap();
    BootstrapPayload {
        version: 1,
        generation: 7,
        not_before_unix: NOW - 60,
        not_after_unix: NOW + 3_600,
        regions: vec![
            BootstrapRegion {
                region: "cn".to_owned(),
                relay_url: "wss://relay-cn.example/v1/tunnel".to_owned(),
                locator_url: "https://locator-cn.example/v1/locator".to_owned(),
                locator_keys: vec![key("locator_cn_1", *locator_cn.verifying_key().as_bytes())],
                relay_x25519_keys: vec![key("relay_cn_1", *relay_cn.public_key())],
            },
            BootstrapRegion {
                region: "global".to_owned(),
                relay_url: "wss://relay-global.example/v1/tunnel".to_owned(),
                locator_url: "https://locator-global.example/v1/locator".to_owned(),
                locator_keys: vec![key(
                    "locator_global_1",
                    *locator_global.verifying_key().as_bytes(),
                )],
                relay_x25519_keys: vec![key("relay_global_1", *relay_global.public_key())],
            },
        ],
    }
}

pub(super) fn signed_envelope(
    signing: &SigningKey,
    kid: &str,
    payload: &BootstrapPayload,
) -> Vec<u8> {
    let payload = serde_json::to_vec(payload).unwrap();
    let mut signing_bytes = BOOTSTRAP_CONTEXT.to_vec();
    signing_bytes.extend_from_slice(&payload);
    let envelope = BootstrapEnvelope {
        version: 1,
        kid: kid.to_owned(),
        payload: URL_SAFE_NO_PAD.encode(payload),
        signature: URL_SAFE_NO_PAD.encode(signing.sign(&signing_bytes).to_bytes()),
    };
    serde_json::to_vec(&envelope).unwrap()
}

pub(super) fn roots(signing: &SigningKey, kid: &str) -> HashMap<String, VerifyingKey> {
    HashMap::from([(kid.to_owned(), signing.verifying_key())])
}

#[test]
fn signed_canonical_bundle_builds_both_region_snapshots() {
    assert!(builtin_roots().is_ok());
    let signing = SigningKey::from_bytes(&[7; 32]);
    let body = signed_envelope(&signing, "test_root_1", &valid_payload());
    let bootstrap =
        verify_bootstrap(&body, &roots(&signing, "test_root_1"), NOW, false, true).unwrap();

    assert_eq!(bootstrap.generation, 7);
    let cn = bootstrap.region(RelayRegion::Cn).unwrap();
    let global = bootstrap.region(RelayRegion::Global).unwrap();
    assert_eq!(
        cn.relay_endpoint,
        RelayEndpoint::parse("wss://relay-cn.example/v1/tunnel").unwrap()
    );
    assert_eq!(
        global.relay_endpoint,
        RelayEndpoint::parse("wss://relay-global.example/v1/tunnel").unwrap()
    );
    assert_eq!(cn.locator_url, "https://locator-cn.example/v1/locator");
    assert!(!cn.development_http);
}

#[test]
fn signature_payload_and_envelope_are_strictly_canonical() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    let body = signed_envelope(&signing, "test_root_1", &valid_payload());

    let mut tampered = body.clone();
    let position = tampered
        .iter()
        .position(|byte| *byte == b'A')
        .unwrap_or(tampered.len() / 2);
    tampered[position] ^= 1;
    assert!(verify_bootstrap(&tampered, &roots, NOW, false, true).is_err());

    let envelope: BootstrapEnvelope = serde_json::from_slice(&body).unwrap();
    let padded_payload = format!("{}=", envelope.payload);
    let noncanonical = serde_json::to_vec(&BootstrapEnvelope {
        payload: padded_payload,
        ..envelope
    })
    .unwrap();
    assert_eq!(
        verify_bootstrap(&noncanonical, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidBase64
    );

    let spaced = format!(" {}", String::from_utf8(body).unwrap());
    assert_eq!(
        verify_bootstrap(spaced.as_bytes(), &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidResponse
    );
}

#[test]
fn payload_rejects_noncanonical_json_duplicate_regions_keys_and_unknown_fields() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");

    let mut payload = valid_payload();
    payload.regions[1].region = "cn".to_owned();
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    payload.regions.reverse();
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    let duplicate = payload.regions[0].locator_keys[0].clone();
    payload.regions[0].locator_keys.push(duplicate);
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    let extra = SigningKey::from_bytes(&[13; 32]);
    payload.regions[0]
        .locator_keys
        .push(key("aaa_out_of_order", *extra.verifying_key().as_bytes()));
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    payload.regions[1].locator_keys[0].kid = "locator_cn_1".to_owned();
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    let reused_x = payload.regions[0].locator_keys[0].x.clone();
    payload.regions[1].locator_keys[0].x = reused_x;
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let canonical = serde_json::to_vec(&valid_payload()).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_owned(), serde_json::Value::Bool(true));
    let raw = serde_json::to_vec(&value).unwrap();
    let mut signing_bytes = BOOTSTRAP_CONTEXT.to_vec();
    signing_bytes.extend_from_slice(&raw);
    let envelope = BootstrapEnvelope {
        version: 1,
        kid: "test_root_1".to_owned(),
        payload: URL_SAFE_NO_PAD.encode(raw),
        signature: URL_SAFE_NO_PAD.encode(signing.sign(&signing_bytes).to_bytes()),
    };
    let body = serde_json::to_vec(&envelope).unwrap();
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );
}

#[test]
fn payload_rejects_exact_cross_region_key_reuse() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");

    let mut payload = valid_payload();
    payload.regions[1].locator_keys[0] = payload.regions[0].locator_keys[0].clone();
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );

    let mut payload = valid_payload();
    payload.regions[1].relay_x25519_keys[0] = payload.regions[0].relay_x25519_keys[0].clone();
    let body = signed_envelope(&signing, "test_root_1", &payload);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::InvalidPayload
    );
}

#[test]
fn payload_rejects_invalid_time_urls_kids_and_low_order_keys() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    for mutate in [
        |payload: &mut BootstrapPayload| payload.not_after_unix = payload.not_before_unix,
        |payload: &mut BootstrapPayload| {
            payload.regions[0].relay_url = "ws://127.0.0.1:1/v1/tunnel".to_owned()
        },
        |payload: &mut BootstrapPayload| {
            payload.regions[0].locator_url = "https://locator.example/wrong".to_owned()
        },
        |payload: &mut BootstrapPayload| {
            payload.regions[0].locator_keys[0].kid = "bad.kid".to_owned()
        },
        |payload: &mut BootstrapPayload| {
            payload.regions[0].relay_x25519_keys[0].x = URL_SAFE_NO_PAD.encode([0; 32])
        },
        |payload: &mut BootstrapPayload| {
            let mut noncanonical = [0xff; 32];
            noncanonical[0] = 0xed;
            noncanonical[31] = 0x7f;
            payload.regions[0].relay_x25519_keys[0].x = URL_SAFE_NO_PAD.encode(noncanonical)
        },
    ] {
        let mut payload = valid_payload();
        mutate(&mut payload);
        let body = signed_envelope(&signing, "test_root_1", &payload);
        assert_eq!(
            verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
            BootstrapError::InvalidPayload
        );
    }
}

#[test]
fn locator_keys_reject_noncanonical_ed25519_encodings() {
    let noncanonical = noncanonical_ed25519_encoding();
    assert!(VerifyingKey::from_bytes(&noncanonical).is_ok_and(|key| !key.is_weak()));
    assert_eq!(
        canonical_ed25519_key(noncanonical, BootstrapError::InvalidPayload),
        Err(BootstrapError::InvalidPayload)
    );

    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    let mut payload = valid_payload();
    payload.regions[0].locator_keys[0].x = URL_SAFE_NO_PAD.encode(noncanonical);
    assert_eq!(
        verify_bootstrap(
            &signed_envelope(&signing, "test_root_1", &payload),
            &roots,
            NOW,
            false,
            true,
        )
        .unwrap_err(),
        BootstrapError::InvalidPayload
    );
}

#[test]
fn integer_wire_boundaries_match_the_go_and_browser_contract() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    let mut boundary = valid_payload();
    boundary.generation = MAX_SAFE_INTEGER;
    boundary.not_before_unix = MAX_UNIX_SECOND - 3_600;
    boundary.not_after_unix = MAX_UNIX_SECOND;
    assert!(verify_bootstrap(
        &signed_envelope(&signing, "test_root_1", &boundary),
        &roots,
        MAX_UNIX_SECOND - 1_800,
        false,
        true,
    )
    .is_ok());

    for mutate in [
        |payload: &mut BootstrapPayload| payload.generation = MAX_SAFE_INTEGER + 1,
        |payload: &mut BootstrapPayload| {
            payload.not_before_unix = MAX_UNIX_SECOND + 1;
            payload.not_after_unix = MAX_UNIX_SECOND + 2;
        },
        |payload: &mut BootstrapPayload| {
            payload.not_before_unix = MAX_UNIX_SECOND - 60;
            payload.not_after_unix = MAX_UNIX_SECOND + 1;
        },
    ] {
        let mut payload = valid_payload();
        mutate(&mut payload);
        assert_eq!(
            verify_bootstrap(
                &signed_envelope(&signing, "test_root_1", &payload),
                &roots,
                NOW,
                false,
                false,
            )
            .unwrap_err(),
            BootstrapError::InvalidPayload
        );
    }
}

#[test]
fn payload_urls_are_canonical_and_unique_across_regions() {
    assert!(bootstrap_url::parse("wss://[2001:db8::1]/v1/tunnel", "/v1/tunnel").is_some());
    for (value, path) in [
        ("wss://Relay.example.cn/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn:443/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn:0444/v1/tunnel", "/v1/tunnel"),
        ("wss://[fe80::1%25en0]/v1/tunnel", "/v1/tunnel"),
        ("wss://[2001:DB8::1]/v1/tunnel", "/v1/tunnel"),
        (
            "wss://[2001:0db8:0000:0000:0000:0000:0000:0001]/v1/tunnel",
            "/v1/tunnel",
        ),
        ("wss://192.168.001.001/v1/tunnel", "/v1/tunnel"),
        ("wss://127.1/v1/tunnel", "/v1/tunnel"),
        ("wss://2130706433/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn/v1/%74unnel", "/v1/tunnel"),
        ("wss://relay.example.cn:0/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn:65536/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn:99999/v1/tunnel", "/v1/tunnel"),
        ("wss://relay.example.cn:/v1/tunnel", "/v1/tunnel"),
        (" wss://relay.example.cn/v1/tunnel", "/v1/tunnel"),
        ("https://Locator.example.cn/v1/locator", "/v1/locator"),
        ("https://locator.example.cn:443/v1/locator", "/v1/locator"),
        ("https://locator.example.cn:0/v1/locator", "/v1/locator"),
        ("https://locator.example.cn:65536/v1/locator", "/v1/locator"),
    ] {
        assert!(bootstrap_url::parse(value, path).is_none(), "{value}");
    }

    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    for mutate in [
        |payload: &mut BootstrapPayload| {
            payload.regions[1].relay_url = payload.regions[0].relay_url.clone()
        },
        |payload: &mut BootstrapPayload| {
            payload.regions[1].locator_url = payload.regions[0].locator_url.clone()
        },
    ] {
        let mut payload = valid_payload();
        mutate(&mut payload);
        assert_eq!(
            verify_bootstrap(
                &signed_envelope(&signing, "test_root_1", &payload),
                &roots,
                NOW,
                false,
                true,
            )
            .unwrap_err(),
            BootstrapError::InvalidPayload
        );
    }
}

#[test]
fn validity_and_generation_fail_closed() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    let mut expired = valid_payload();
    expired.not_before_unix = NOW - 600;
    expired.not_after_unix = NOW;
    let body = signed_envelope(&signing, "test_root_1", &expired);
    assert_eq!(
        verify_bootstrap(&body, &roots, NOW, false, true).unwrap_err(),
        BootstrapError::Expired
    );

    let previous = verify_bootstrap(
        &signed_envelope(&signing, "test_root_1", &valid_payload()),
        &roots,
        NOW,
        false,
        true,
    )
    .unwrap();
    let mut lower = valid_payload();
    lower.generation = 6;
    let lower = verify_bootstrap(
        &signed_envelope(&signing, "test_root_1", &lower),
        &roots,
        NOW,
        false,
        true,
    )
    .unwrap();
    assert_eq!(
        reject_rollback(&previous, &lower),
        Err(BootstrapError::Rollback)
    );

    let mut rewritten = valid_payload();
    rewritten.regions[0].relay_url = "wss://relay-cn-2.example/v1/tunnel".to_owned();
    let rewritten = verify_bootstrap(
        &signed_envelope(&signing, "test_root_1", &rewritten),
        &roots,
        NOW,
        false,
        true,
    )
    .unwrap();
    assert_eq!(
        reject_rollback(&previous, &rewritten),
        Err(BootstrapError::Rollback)
    );
}

#[test]
fn production_bootstrap_endpoint_is_exact_https_only() {
    assert!(
        parse_bootstrap_endpoint("https://hub.openpencil.dev/api/v1/collaboration/bootstrap")
            .is_ok()
    );
    for endpoint in [
        "http://hub.openpencil.dev/api/v1/collaboration/bootstrap",
        " https://hub.openpencil.dev/api/v1/collaboration/bootstrap",
        "https://Hub.openpencil.dev/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev:443/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev:0/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev:65536/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev:99999/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev:/api/v1/collaboration/bootstrap",
        "https://[fe80::1%25en0]/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev/api/v1/collaboration/%62ootstrap",
        "https://hub.openpencil.dev/api/v1/collaboration/bootstrap/",
        "https://user@hub.openpencil.dev/api/v1/collaboration/bootstrap",
        "https://hub.openpencil.dev/api/v1/collaboration/bootstrap?q=1",
        "https://hub.openpencil.dev/api/v1/collaboration/bootstrap#fragment",
    ] {
        assert_eq!(
            parse_bootstrap_endpoint(endpoint).unwrap_err(),
            BootstrapError::InvalidEndpoint
        );
    }
    assert!(parse_bootstrap_endpoint_with_policy(
        "http://127.0.0.1:34123/api/v1/collaboration/bootstrap",
        true
    )
    .is_ok());
    assert!(parse_bootstrap_endpoint_with_policy(
        "http://127.0.0.1:34123/api/v1/collaboration/bootstrap",
        false
    )
    .is_err());
    assert!(parse_bootstrap_endpoint_with_policy(
        "http://localhost:34123/api/v1/collaboration/bootstrap",
        true
    )
    .is_err());
}

#[test]
fn etag_is_strong_and_bound_to_exact_envelope_bytes() {
    let body = b"{\"version\":1}";
    let etag = strong_etag(body);
    assert_eq!(etag.len(), 66);
    assert!(etag.starts_with('"') && etag.ends_with('"'));
    let mut headers = HeaderMap::new();
    headers.insert(ETAG, HeaderValue::from_str(&etag).unwrap());
    assert_eq!(response_etag(&headers, body).unwrap(), etag);
    assert!(response_etag(&headers, b"different").is_err());
}

#[test]
fn provider_reuses_a_valid_signed_cache_on_transport_failure() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let body = signed_envelope(&signing, "test_root_1", &valid_payload());
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}{BOOTSTRAP_PATH}");
    let etag = strong_etag(&body);
    let response_body = body.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2_048];
        let _ = stream.read(&mut request).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .unwrap();
        stream.write_all(&response_body).unwrap();
    });
    let cache_root =
        std::env::temp_dir().join(format!("op-bootstrap-cache-{}-{}", std::process::id(), NOW));
    let _ = std::fs::remove_dir_all(&cache_root);
    std::fs::create_dir_all(&cache_root).unwrap();
    let provider = EnvironmentRelayBootstrapProvider {
        endpoint: Url::parse(&endpoint).unwrap(),
        roots: roots(&signing, "test_root_1"),
        development_http: true,
        cache_path: cache_root.join(BOOTSTRAP_CACHE_FILE),
    };
    assert_eq!(provider.load_inner(NOW).unwrap().generation, 7);
    server.join().unwrap();
    assert_eq!(provider.load_inner(NOW).unwrap().generation, 7);
    let _ = std::fs::remove_dir_all(cache_root);
}

#[test]
fn provider_sends_etag_and_accepts_only_matching_not_modified() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let body = signed_envelope(&signing, "test_root_1", &valid_payload());
    let etag = strong_etag(&body);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}{BOOTSTRAP_PATH}");
    let (seen, received) = mpsc::sync_channel(1);
    let response_body = body.clone();
    let response_etag = etag.clone();
    let server = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4_096];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_string();
            if index == 0 {
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nETag: {response_etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response_body.len()
                )
                .unwrap();
                stream.write_all(&response_body).unwrap();
            } else {
                seen.send(request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 304 Not Modified\r\nETag: {response_etag}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        }
    });
    let cache_root =
        std::env::temp_dir().join(format!("op-bootstrap-etag-{}-{}", std::process::id(), NOW));
    let _ = std::fs::remove_dir_all(&cache_root);
    std::fs::create_dir_all(&cache_root).unwrap();
    let provider = EnvironmentRelayBootstrapProvider {
        endpoint: Url::parse(&endpoint).unwrap(),
        roots: roots(&signing, "test_root_1"),
        development_http: true,
        cache_path: cache_root.join(BOOTSTRAP_CACHE_FILE),
    };
    provider.load_inner(NOW).unwrap();
    provider.load_inner(NOW).unwrap();
    let request = received.recv().unwrap();
    assert!(request
        .to_ascii_lowercase()
        .contains(&format!("if-none-match: {}", etag).to_ascii_lowercase()));
    server.join().unwrap();
    let _ = std::fs::remove_dir_all(cache_root);
}

/// A damaged cache degrades to "no floor" instead of blocking bootstrap.
///
/// This is a deliberate weakening relative to failing closed, and the test
/// states its cost plainly: with the floor gone, the lower-generation document
/// IS accepted. It is accepted because refusing bought no security — deleting
/// the cache file already achieves a missing floor and needs no more privilege
/// than corrupting it — while it did cost the ability to collaborate at all on
/// a machine with a damaged or unreadable cache. What must not happen is the
/// loss passing silently, so `rollback_floor_armed` has to report it.
fn assert_bad_cache_degrades_to_an_unarmed_rollback_floor(
    label: &str,
    damage_cache: impl FnOnce(&std::path::Path),
) {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let mut high_payload = valid_payload();
    high_payload.generation = 8;
    let high_body = signed_envelope(&signing, "test_root_1", &high_payload);
    let lower_body = signed_envelope(&signing, "test_root_1", &valid_payload());
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}{BOOTSTRAP_PATH}");
    let lower_etag = strong_etag(&lower_body);
    let (stop_sender, stop_receiver) = mpsc::sync_channel(1);
    let server = thread::spawn(move || loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut request = [0_u8; 2_048];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nETag: {lower_etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    lower_body.len()
                )
                .unwrap();
                stream.write_all(&lower_body).unwrap();
                return true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                match stop_receiver.recv_timeout(Duration::from_millis(10)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return false,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            Err(error) => panic!("bootstrap listener failed: {error}"),
        }
    });
    let cache_root = std::env::temp_dir().join(format!(
        "op-bootstrap-floor-{label}-{}-{NOW}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&cache_root);
    let _ = std::fs::remove_file(&cache_root);
    std::fs::create_dir_all(&cache_root).unwrap();
    let cache_path = cache_root.join(BOOTSTRAP_CACHE_FILE);
    write_cache(
        &cache_path,
        &BootstrapCache {
            endpoint: endpoint.clone(),
            etag: Some(strong_etag(&high_body)),
            body: String::from_utf8(high_body).unwrap(),
        },
    )
    .unwrap();
    damage_cache(&cache_path);
    let provider = EnvironmentRelayBootstrapProvider {
        endpoint: Url::parse(&endpoint).unwrap(),
        roots: roots(&signing, "test_root_1"),
        development_http: true,
        cache_path,
    };
    let loaded = provider
        .load_inner(NOW)
        .expect("a damaged cache degrades instead of blocking bootstrap");
    assert_eq!(
        loaded.generation,
        valid_payload().generation,
        "with no floor the lower-generation document is accepted — this is the cost of degrading"
    );
    assert!(
        !loaded.rollback_floor_armed,
        "a damaged cache must report the anti-rollback floor as unarmed"
    );
    stop_sender.send(()).ok();
    assert!(
        server.join().unwrap(),
        "degrading means the fetch proceeds rather than stopping at the damaged cache"
    );
    let _ = std::fs::remove_dir_all(cache_root);
}

#[test]
fn corrupt_cache_degrades_to_an_unarmed_rollback_floor() {
    assert_bad_cache_degrades_to_an_unarmed_rollback_floor("corrupt", |path| {
        std::fs::write(path, b"not json").unwrap();
    });
}

#[cfg(unix)]
#[test]
fn unreadable_cache_degrades_to_an_unarmed_rollback_floor() {
    use std::os::unix::fs::PermissionsExt as _;

    let probe = std::env::temp_dir().join(format!(
        "op-bootstrap-permission-probe-{}-{NOW}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&probe);
    std::fs::write(&probe, b"probe").unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o000)).unwrap();
    let can_still_read = std::fs::File::open(&probe).is_ok();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o600)).unwrap();
    let _ = std::fs::remove_file(&probe);
    // Root and unusual ACL environments can still read mode-000 files, so
    // they cannot exercise the permission-denied branch.
    if can_still_read {
        return;
    }
    assert_bad_cache_degrades_to_an_unarmed_rollback_floor("unreadable", |path| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
    });
}

#[test]
fn cache_persist_failure_degrades_without_failing_the_bootstrap() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let body = signed_envelope(&signing, "test_root_1", &valid_payload());
    let cache_root = std::env::temp_dir().join(format!(
        "op-bootstrap-unwritable-{}-{}",
        std::process::id(),
        NOW
    ));
    let _ = std::fs::remove_dir_all(&cache_root);
    let _ = std::fs::remove_file(&cache_root);
    std::fs::create_dir_all(&cache_root).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = format!("http://{address}{BOOTSTRAP_PATH}");
    let etag = strong_etag(&body);
    let response_body = body.clone();
    let blocked_cache_root = cache_root.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2_048];
        let _ = stream.read(&mut request).unwrap();
        // The cache is genuinely absent during the initial read. Replace its
        // parent only after the request arrives, so persistence fails without
        // conflating that failure with an unsafe cache read.
        std::fs::remove_dir(&blocked_cache_root).unwrap();
        std::fs::write(&blocked_cache_root, b"not a directory").unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_body.len()
        )
        .unwrap();
        stream.write_all(&response_body).unwrap();
    });
    let provider = EnvironmentRelayBootstrapProvider {
        endpoint: Url::parse(&endpoint).unwrap(),
        roots: roots(&signing, "test_root_1"),
        development_http: true,
        cache_path: cache_root.join(BOOTSTRAP_CACHE_FILE),
    };
    // An unwritable configuration directory must not mean "cannot
    // collaborate": the document this run fetched was fully verified, so it is
    // returned. What is lost is the persisted generation floor for the next
    // start, and that loss has to stay visible rather than passing silently.
    let loaded = provider
        .load_inner(NOW)
        .expect("a verified document is still usable when its cache cannot be written");
    assert_eq!(loaded.generation, valid_payload().generation);
    assert!(
        !loaded.rollback_floor_armed,
        "a failed cache write must report the anti-rollback floor as unarmed"
    );
    server.join().unwrap();
    let _ = std::fs::remove_file(cache_root);
}
