//! Current-turn reference images survive tool screenshot replacement and retries.
use super::*;

fn reference() -> op_ai::chat_provider::ChatAttachment {
    op_ai::chat_provider::ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![1, 2, 3, 4],
    }
}
fn openai_tool_turn() -> String {
    [
        r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"screenshot","type":"function","function":{"name":"get_screenshot","arguments":"{\"nodeId\":\"root\"}"}}]}}]}"#,
        "", r#"data: {"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
        "", "data: [DONE]", "", "",
    ].join("\n")
}
fn openai_done() -> String {
    [
        r#"data: {"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]}"#,
        "",
        "data: [DONE]",
        "",
        "",
    ]
    .join("\n")
}
fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[test]
fn reference_images_survive_later_tool_screenshot_and_corrective_round() {
    for anthropic in [false, true] {
        let tool = if anthropic {
            anthropic_get_screenshot_turn()
        } else {
            openai_tool_turn()
        };
        let done = if anthropic {
            anthropic_text_turn()
        } else {
            openai_done()
        };
        let (base, requests) = serve_sse_script(vec![tool, done.clone(), done]);
        let executor = ScriptedExecutor::ok(
            &json!({"success":true,"data":{
                "image_base64":TINY_PNG_B64,"format":"png"
            }})
            .to_string(),
        );
        let cfg = AgentLoopConfig {
            url: format!(
                "{base}/{}",
                if anthropic {
                    "v1/messages"
                } else {
                    "chat/completions"
                }
            ),
            api_key: "fixture".into(),
            model: "vision-test".into(),
            system_prompt: String::new(),
            history: Vec::new(),
            user_prompt: "use the reference".into(),
            attachments: vec![reference()],
            max_output_tokens: 512,
            tools: vec![get_screenshot_tool_def()],
            executor,
            max_turns: 5,
            finalize_on_exit: true,
            disable_thinking: false,
            dial_policy: crate::provider_dial::EndpointDialPolicy::Trusted,
        };
        let (result, _) = run_loop_collect(cfg, anthropic);
        assert_eq!(result, Ok(true));
        for _ in 0..3 {
            let payload = body(&requests.recv_timeout(Duration::from_secs(2)).unwrap());
            let content = &payload["messages"][0]["content"];
            assert_eq!(content[0]["text"], "use the reference");
            if anthropic {
                assert_eq!(content[1]["source"]["data"], "AQIDBA==");
            } else {
                assert_eq!(
                    content[1]["image_url"]["url"],
                    "data:image/png;base64,AQIDBA=="
                );
            }
        }
    }
}

#[test]
fn unsupported_reference_fails_before_the_design_loop_dials() {
    for model in ["deepseek-chat", "glm-5.2"] {
        let cfg = AgentLoopConfig {
            url: "http://127.0.0.1:1/chat/completions".into(),
            api_key: "fixture".into(),
            model: model.into(),
            system_prompt: String::new(),
            history: Vec::new(),
            user_prompt: "use reference".into(),
            attachments: vec![reference()],
            max_output_tokens: 512,
            tools: vec![get_screenshot_tool_def()],
            executor: ScriptedExecutor::ok("{}"),
            max_turns: 1,
            finalize_on_exit: false,
            disable_thinking: false,
            dial_policy: crate::provider_dial::EndpointDialPolicy::Trusted,
        };
        let (result, _) = run_loop_collect(cfg, false);
        assert_eq!(
            result,
            Err(BuiltinHttpError::Attachments(
                crate::chat_attachments::AttachmentError::ModelIsTextOnly
            ))
        );
    }
}
