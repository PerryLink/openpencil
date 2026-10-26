use op_ai::chat_provider::{ChatProvider, ChatRequest};

#[test]
fn hosted_wire_has_one_authority_and_preserves_conversation() {
    let good = r#"{"hosted_route_id":"hosted-123","user":"next","history":[{"role":"user","content":"first"},{"role":"assistant","content":"reply"}]}"#;
    let request = crate::ai_proxy::parse_ai_stream_body(good).unwrap();
    assert!(crate::web_chat_standard::parse_standard_turn_body(good).is_none());
    assert_eq!(request.history.len(), 2);
    assert_eq!(request.hosted_route_id.as_deref(), Some("hosted-123"));
    for extra in [
        r#""model":"other""#,
        r#""provider":"codex""#,
        r#""credential":null"#,
        r#""builtinProviderId":"x""#,
        r#""endpoint":"https://evil.example""#,
        r#""hosted_route_id":"second""#,
    ] {
        let body = format!("{},{}{}", good.trim_end_matches('}'), extra, "}");
        assert!(
            crate::ai_proxy::parse_ai_stream_body(&body).is_none(),
            "{extra}"
        );
    }
}

#[test]
fn proxy_discards_buffered_hosted_deltas_after_session_invalidation() {
    struct StaleProvider;
    impl ChatProvider for StaleProvider {
        fn provider_label(&self) -> &str {
            "stale-hosted"
        }
        fn hosted_session_epoch(&self) -> Option<u64> {
            Some(0)
        }
        fn send(
            &self,
            _: ChatRequest,
        ) -> Box<dyn Iterator<Item = op_ai::chat_provider::ChatDelta> + Send> {
            Box::new(
                [op_ai::chat_provider::ChatDelta::TextDelta(
                    "old-account-output".into(),
                )]
                .into_iter(),
            )
        }
    }
    let request =
        crate::ai_proxy::parse_ai_stream_body(r#"{"hosted_route_id":"route","user":"hello"}"#)
            .unwrap();
    let mut output = Vec::new();
    crate::ai_proxy::stream_ai_response(&mut output, request, &StaleProvider, None).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(!output.contains("old-account-output"));
    assert!(output.contains("Hosted session changed"));
}
