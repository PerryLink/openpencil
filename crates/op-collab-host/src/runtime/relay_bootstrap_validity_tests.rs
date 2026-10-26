use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener};
use std::thread;

use ed25519_dalek::SigningKey;

use super::tests::{roots, signed_envelope, valid_payload, NOW};
use super::*;

const SEVEN_DAYS: u64 = 604_800;
const NINETY_DAYS: u64 = 7_776_000;

#[test]
fn go_signer_golden_envelope_verifies_byte_exactly() {
    const ENVELOPE: &str = include_str!("relay_bootstrap_testdata/golden-envelope.json");
    const PAYLOAD: &str = include_str!("relay_bootstrap_testdata/golden-payload.json");
    const GOLDEN_ROOT_X: &str = "A6EHv_POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg";
    let root = VerifyingKey::from_bytes(&decode_fixed::<32>(GOLDEN_ROOT_X).unwrap()).unwrap();
    let roots = HashMap::from([("test-collab-root-v1".to_owned(), root)]);
    let envelope = ENVELOPE.trim_end_matches('\n');
    let verified =
        verify_bootstrap(envelope.as_bytes(), &roots, 1_893_456_060, false, true).unwrap();
    let wire: BootstrapEnvelope = serde_json::from_str(envelope).unwrap();
    assert_eq!(
        decode_bounded(&wire.payload, MAX_PAYLOAD_BYTES).unwrap(),
        PAYLOAD.trim_end_matches('\n').as_bytes()
    );
    assert_eq!(verified.generation, 42);
    assert!(verified.region(RelayRegion::Cn).is_ok());
    assert!(verified.region(RelayRegion::Global).is_ok());
}

#[test]
fn accepts_seven_and_ninety_days_with_exclusive_expiry() {
    assert_eq!(MAX_VALIDITY_SECS, NINETY_DAYS);
    let signing = SigningKey::from_bytes(&[7; 32]);
    let roots = roots(&signing, "test_root_1");
    for lifetime in [SEVEN_DAYS, NINETY_DAYS] {
        let mut payload = valid_payload();
        payload.not_before_unix = NOW;
        payload.not_after_unix = NOW + lifetime;
        let body = signed_envelope(&signing, "test_root_1", &payload);
        for now in [NOW, payload.not_after_unix - 1] {
            assert!(verify_bootstrap(&body, &roots, now, false, true).is_ok());
        }
        assert_eq!(
            verify_bootstrap(&body, &roots, payload.not_after_unix, false, true).unwrap_err(),
            BootstrapError::Expired
        );
    }
}

#[test]
fn rejects_ninety_days_plus_one_even_as_an_expired_rollback_baseline() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let mut payload = valid_payload();
    payload.not_before_unix = NOW;
    payload.not_after_unix = NOW + NINETY_DAYS + 1;
    let body = signed_envelope(&signing, "test_root_1", &payload);
    for require_current in [true, false] {
        assert_eq!(
            verify_bootstrap(
                &body,
                &roots(&signing, "test_root_1"),
                NOW,
                false,
                require_current
            )
            .unwrap_err(),
            BootstrapError::InvalidPayload
        );
    }
}

#[test]
fn ninety_day_window_preserves_the_300_second_future_skew_limit() {
    assert_eq!(MAX_CLOCK_SKEW_SECS, 300);
    let signing = SigningKey::from_bytes(&[7; 32]);
    for (skew, accepted) in [(300, true), (301, false)] {
        let mut payload = valid_payload();
        payload.not_before_unix = NOW + skew;
        payload.not_after_unix = payload.not_before_unix + NINETY_DAYS;
        let body = signed_envelope(&signing, "test_root_1", &payload);
        let result = verify_bootstrap(&body, &roots(&signing, "test_root_1"), NOW, false, true);
        assert_eq!(result.is_ok(), accepted);
        if !accepted {
            assert_eq!(result.unwrap_err(), BootstrapError::Expired);
        }
    }
}

fn cached_provider(
    listener: &TcpListener,
    signing: &SigningKey,
    payload: &BootstrapPayload,
    label: &str,
) -> (EnvironmentRelayBootstrapProvider, PathBuf) {
    let endpoint = format!("http://{}{BOOTSTRAP_PATH}", listener.local_addr().unwrap());
    let cache_root = std::env::temp_dir().join(format!(
        "op-bootstrap-validity-{label}-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    std::fs::create_dir_all(&cache_root).unwrap();
    let provider = EnvironmentRelayBootstrapProvider {
        endpoint: Url::parse(&endpoint).unwrap(),
        roots: roots(signing, "test_root_1"),
        development_http: true,
        cache_path: cache_root.join(BOOTSTRAP_CACHE_FILE),
    };
    let body = signed_envelope(signing, "test_root_1", payload);
    write_cache(
        &provider.cache_path,
        &BootstrapCache {
            endpoint,
            etag: Some(strong_etag(&body)),
            body: String::from_utf8(body).unwrap(),
        },
    )
    .unwrap();
    (provider, cache_root)
}

#[test]
fn expired_seven_and_ninety_day_caches_are_not_reused_on_304_or_outage() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    for lifetime in [SEVEN_DAYS, NINETY_DAYS] {
        let mut payload = valid_payload();
        payload.not_before_unix = NOW - lifetime;
        payload.not_after_unix = NOW;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let (provider, cache_root) = cached_provider(&listener, &signing, &payload, "expired");
        let etag = strong_etag(&signed_envelope(&signing, "test_root_1", &payload));
        let server = thread::spawn(move || {
            for status in ["304 Not Modified", "503 Service Unavailable"] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 4_096];
                let _ = stream.read(&mut request).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nETag: {etag}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        assert_eq!(
            provider.load_inner(NOW).unwrap_err(),
            BootstrapError::InvalidResponse
        );
        assert_eq!(
            provider.load_inner(NOW).unwrap_err(),
            BootstrapError::Transport
        );
        server.join().unwrap();
        // A failed request after the server disappears cannot revive the cache either.
        assert_eq!(
            provider.load_inner(NOW).unwrap_err(),
            BootstrapError::Transport
        );
        std::fs::remove_dir_all(cache_root).unwrap();
    }
}

#[test]
fn expired_seven_day_cache_still_rejects_rollback_and_same_generation_renewal() {
    let signing = SigningKey::from_bytes(&[7; 32]);
    let mut expired = valid_payload();
    expired.not_before_unix = NOW - SEVEN_DAYS;
    expired.not_after_unix = NOW;
    expired.generation = 9;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let (provider, cache_root) = cached_provider(&listener, &signing, &expired, "floor");
    let server = thread::spawn(move || {
        for generation in [8, 9, 10] {
            let mut payload = valid_payload();
            payload.generation = generation;
            payload.not_before_unix = NOW;
            payload.not_after_unix = NOW + NINETY_DAYS;
            let body = signed_envelope(&signing, "test_root_1", &payload);
            let etag = strong_etag(&body);
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4_096];
            let _ = stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nETag: {etag}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    for _ in 0..2 {
        assert_eq!(
            provider.load_inner(NOW).unwrap_err(),
            BootstrapError::Rollback
        );
    }
    assert_eq!(provider.load_inner(NOW).unwrap().generation, 10);
    server.join().unwrap();
    std::fs::remove_dir_all(cache_root).unwrap();
}
