//! Engine-thread tests for the mobile chat pump: a built-in provider send
//! must stream into the transcript, and every failure path must land an
//! error in the assistant bubble instead of leaving it on "Thinking…".

use super::*;
use std::io::Write;
use std::net::TcpListener;
use std::time::{Duration, Instant};

use op_editor_core::BuiltinAgentKind;

const TEST_VIEWPORT: (f32, f32) = (402.0, 782.0);

fn host_with_builtin_provider(base_url: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
    settings.builtin_agents.clear();
    settings.add_builtin_agent_config(
        "DeepSeek",
        "sk-mobile-chat",
        "deepseek-chat",
        BuiltinAgentKind::OpenAiCompat,
        base_url,
    );
    host.editor_state_mut().rebuild_chat_models();
    let chat = &mut host.editor_state_mut().chat;
    let builtin_index = chat
        .available_models
        .iter()
        .position(|entry| entry.builtin_provider_id.is_some())
        .expect("a ready builtin agent must surface a chat model entry");
    chat.selected_model = builtin_index;
    host
}

fn send_user_message(host: &mut WidgetHostNative, text: &str) {
    let chat = &mut host.editor_state_mut().chat;
    chat.set_input_text(text);
    assert!(chat.begin_send(), "begin_send must queue the turn");
    assert!(chat.pending_send.is_some());
}

/// Serve one canned HTTP response for one connection, returning the raw
/// request bytes for assertions.
fn spawn_chat_server(response: String) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local chat server");
    let address = listener.local_addr().expect("local chat address");
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept chat request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("fixture deadline");
        let request =
            crate::test_http::read_request(&mut stream).expect("read complete chat request");
        stream
            .write_all(response.as_bytes())
            .expect("write chat response");
        request
    });
    (format!("http://{address}"), handle)
}

fn sse_ok_response(events: &[&str]) -> String {
    let body: String = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Pump until the in-flight turn retires (pump returns no wake deadline).
fn pump_to_completion(chat_host: &mut MobileChatHost, host: &mut WidgetHostNative) {
    let started = Instant::now();
    let mut now_ms = 10;
    loop {
        let wake = chat_host.pump(host, now_ms, TEST_VIEWPORT);
        if wake.is_none() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "chat turn did not complete within the test deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
        now_ms += STREAM_POLL_INTERVAL_MS;
    }
}

fn last_assistant(host: &WidgetHostNative) -> op_editor_core::ChatMessage {
    host.editor_state()
        .chat
        .messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::Assistant)
        .expect("transcript holds an assistant bubble")
        .clone()
}

#[test]
fn builtin_send_streams_reply_into_transcript() {
    let (base_url, server) = spawn_chat_server(sse_ok_response(&[
        r#"{"choices":[{"delta":{"content":"Hello"}}]}"#,
        r#"{"choices":[{"delta":{"content":", world"}}]}"#,
        "[DONE]",
    ]));
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "hi there");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    let reply = last_assistant(&host);
    assert_eq!(reply.content, "Hello, world");
    assert!(!reply.streaming, "a finished turn must stop streaming");
    assert!(host.editor_state().chat.pending_send.is_none());

    let request = server.join().expect("chat server exits");
    assert!(request.starts_with("POST /chat/completions HTTP/1.1"));
    let authorization = request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("authorization")
            .then_some(value.trim())
    });
    assert!(
        authorization == Some("Bearer sk-mobile-chat"),
        "fixture authorization mismatch; header names: {:?}",
        request
            .lines()
            .filter_map(|line| line.split_once(':').map(|(name, _)| name))
            .collect::<Vec<_>>()
    );
    assert!(request.contains("\"model\":\"deepseek-chat\""));
}

#[test]
fn provider_http_error_lands_in_bubble_instead_of_eternal_thinking() {
    let (base_url, server) = spawn_chat_server(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_string(),
    );
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "hi there");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    let reply = last_assistant(&host);
    assert!(
        reply.content.starts_with("error: "),
        "provider failure must surface in the transcript, got {:?}",
        reply.content
    );
    assert!(reply.content.contains("http 500"));
    assert!(!reply.streaming, "an aborted turn must stop streaming");
    let _ = server.join();
}

#[test]
fn non_builtin_selection_reports_honest_error() {
    let mut host = WidgetHostNative::new();
    // No builtin agents configured — the selection resolves to a CLI
    // provider, which cannot exist on a phone.
    send_user_message(&mut host, "hi there");

    let mut chat_host = MobileChatHost::default();
    assert!(
        chat_host.pump(&mut host, 10, TEST_VIEWPORT).is_none(),
        "no turn launches"
    );

    let reply = last_assistant(&host);
    assert!(
        reply.content.starts_with("error: "),
        "unroutable selection must land an error, got {:?}",
        reply.content
    );
    assert!(reply.content.contains("not available on this device"));
    assert!(!reply.streaming);
}

#[test]
fn stop_request_drops_the_inflight_turn() {
    // A server that never responds — the turn would stream forever.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled server");
    let base_url = format!("http://{}", listener.local_addr().expect("local address"));
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "hi there");

    let mut chat_host = MobileChatHost::default();
    assert!(
        chat_host.pump(&mut host, 10, TEST_VIEWPORT).is_some(),
        "a launched turn schedules the streaming repoll"
    );
    host.editor_state_mut().chat.pending_stop_chat = true;
    assert!(
        chat_host.pump(&mut host, 43, TEST_VIEWPORT).is_none(),
        "stop must retire the in-flight turn"
    );
    drop(listener);
}

fn reference_image() -> op_ai::chat_provider::ChatAttachment {
    op_ai::chat_provider::ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![1, 2, 3, 4],
    }
}

#[test]
fn reference_images_reach_both_plain_provider_wires_and_do_not_leak_to_next_turn() {
    for kind in [BuiltinAgentKind::OpenAiCompat, BuiltinAgentKind::Anthropic] {
        let events = match kind {
            BuiltinAgentKind::OpenAiCompat => vec!["[DONE]"],
            BuiltinAgentKind::Anthropic => vec![r#"{"type":"message_stop"}"#],
        };
        let (base_url, server) = spawn_chat_server(sse_ok_response(&events));
        let mut host = host_with_builtin_provider(&base_url);
        let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
        settings.builtin_agents[0].kind = kind;
        settings.builtin_agents[0].models = vec!["vision-test".into()];
        host.editor_state_mut().rebuild_chat_models();
        host.editor_state_mut().chat.selected_model = host
            .editor_state()
            .chat
            .available_models
            .iter()
            .position(|entry| entry.builtin_provider_id.is_some())
            .unwrap();
        assert!(host
            .editor_state_mut()
            .chat
            .add_attachment(reference_image()));
        send_user_message(&mut host, "describe this reference");
        let mut chat_host = MobileChatHost::default();
        pump_to_completion(&mut chat_host, &mut host);
        assert!(host.editor_state().chat.pending_attachments.is_empty());
        let request = server.join().unwrap();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        let content = &body["messages"].as_array().unwrap().last().unwrap()["content"];
        assert_eq!(content[0]["text"], "describe this reference");
        match kind {
            BuiltinAgentKind::OpenAiCompat => assert_eq!(
                content[1]["image_url"]["url"],
                "data:image/png;base64,AQIDBA=="
            ),
            BuiltinAgentKind::Anthropic => {
                assert_eq!(content[1]["source"]["media_type"], "image/png");
                assert_eq!(content[1]["source"]["data"], "AQIDBA==");
            }
        }
        let next = prepare_builtin_turn(host.editor_state(), "next turn").unwrap();
        assert!(next.attachments.is_empty());
    }
}

#[test]
fn text_only_model_rejection_preserves_draft_and_images_without_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut host =
        host_with_builtin_provider(&format!("http://{}", listener.local_addr().unwrap()));
    host.editor_state_mut()
        .chat
        .add_attachment(reference_image());
    send_user_message(&mut host, "look at this reference");
    let mut runtime = MobileChatHost::default();
    assert!(runtime.pump(&mut host, 0, TEST_VIEWPORT).is_none());
    assert_eq!(
        host.editor_state().chat.input.text(),
        "look at this reference"
    );
    assert_eq!(
        host.editor_state().chat.pending_attachments,
        vec![reference_image()]
    );
    assert!(!last_assistant(&host).streaming);
    assert!(!last_assistant(&host).content.is_empty());
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn hosted_reference_rejection_preserves_draft_before_resolving_runtime_route() {
    use op_editor_core::{HostedModelCapabilities, HostedModelRoute, HostedProtocol};
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().editor_ui.account =
        op_editor_core::AccountState::signed_in_profile("fixture".into(), None);
    let route = HostedModelRoute::new(
        "hosted-mobile-test",
        "test-model",
        "Test Hosted",
        HostedProtocol::OpenAiCompatible,
        HostedModelCapabilities::default(),
    )
    .unwrap();
    host.editor_state_mut()
        .replace_hosted_model_catalog(vec![route])
        .unwrap();
    host.editor_state_mut().chat.selected_model = host
        .editor_state()
        .chat
        .available_models
        .iter()
        .position(|entry| entry.hosted_route_id().is_some())
        .unwrap();
    host.editor_state_mut()
        .chat
        .add_attachment(reference_image());
    send_user_message(&mut host, "inspect reference");
    let mut runtime = MobileChatHost::default();
    assert!(runtime.pump(&mut host, 0, TEST_VIEWPORT).is_none());
    assert_eq!(
        host.editor_state().chat.pending_attachments,
        vec![reference_image()]
    );
    assert_eq!(host.editor_state().chat.input.text(), "inspect reference");
    assert!(!last_assistant(&host).streaming);
}

#[test]
fn generic_hosted_request_carries_images_and_no_model_override() {
    use op_ai::chat_provider::{ChatProvider, ChatRequest};
    struct Capture(std::sync::Mutex<mpsc::Sender<ChatRequest>>);
    impl ChatProvider for Capture {
        fn provider_label(&self) -> &str {
            "Hosted"
        }
        fn send(&self, request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
            self.0.lock().unwrap().send(request).unwrap();
            Box::new(
                [ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                }]
                .into_iter(),
            )
        }
    }
    let state = EditorState::starter();
    let (tx, rx) = mpsc::channel();
    let _job = hosted::start_provider_turn(
        &state,
        Box::new(Capture(std::sync::Mutex::new(tx))),
        "inspect",
        vec![reference_image()],
        0,
    );
    let request = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(request.attachments, vec![reference_image()]);
    assert!(request.model.is_none());
}

#[test]
fn hosted_account_epoch_discards_queued_output_and_tool_writes() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let mut host = WidgetHostNative::new();
    send_user_message(&mut host, "hello");
    host.editor_state_mut().chat.pending_send = None;
    host.editor_state_mut()
        .chat
        .messages
        .last_mut()
        .unwrap()
        .content = "partial narration".into();
    let before = host.editor_state().doc.clone();
    let (tx, rx) = mpsc::channel();
    tx.send(ChatDelta::TextDelta("stale-account-output".into()))
        .unwrap();
    let (tool_tx, tool_rx) = mpsc::channel();
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    tool_tx
        .send(ChatToolRequest {
            name: "batch_design".into(),
            args_json: "{}".into(),
            ack: ack_tx,
        })
        .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    host.editor_state_mut().editor_ui.workspace.active = true;
    host.editor_state_mut()
        .editor_ui
        .workspace
        .resume_generating(73);
    let mut runtime = MobileChatHost::default();
    runtime.turn = Some(ChatTurnJob {
        session: ChatSession::from_channels_with_cancel(rx, Some(tool_rx), cancel.clone())
            .with_hosted_session_epoch(Some(0)),
        document_identity: (
            host.document_epoch(),
            host.editor_state().document_generation(),
        ),
        running_tab: host.editor_state().chat.active_index(),
        running_tab_identity: host.editor_state().chat.instance_id(),
        running_message_index: host.editor_state().chat.messages.len().saturating_sub(1),
        workspace_epoch: lifetime::workspace_epoch(host.editor_state()),
        abort: None,
        indicator_epoch: None,
        page_edit_fence: None,
        bound_page_id: None,
        document_mutated: false,
    });
    assert!(runtime.pump(&mut host, 0, TEST_VIEWPORT).is_none());
    assert!(runtime.turn.is_none());
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Failed
    );
    assert!(cancel.load(Ordering::Acquire));
    assert_eq!(host.editor_state().doc, before);
    assert!(!last_assistant(&host)
        .content
        .contains("stale-account-output"));
    assert!(!last_assistant(&host).streaming);
    assert!(op_editor_core::workspace_run::last_assistant_failed(
        host.editor_state()
    ));
    assert!(matches!(
        ack_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn closed_conversation_never_receives_a_previous_tabs_buffered_reply() {
    let mut host = WidgetHostNative::new();
    send_user_message(&mut host, "old tab");
    host.editor_state_mut().chat.pending_send = None;
    let old_identity = host.editor_state().chat.instance_id();
    let (tx, rx) = mpsc::channel();
    tx.send(ChatDelta::TextDelta("old reply".into())).unwrap();
    host.editor_state_mut().editor_ui.workspace.active = true;
    host.editor_state_mut()
        .editor_ui
        .workspace
        .resume_generating(73);
    let mut runtime = MobileChatHost::default();
    runtime.turn = Some(ChatTurnJob {
        session: ChatSession::from_channels(rx, None),
        document_identity: (
            host.document_epoch(),
            host.editor_state().document_generation(),
        ),
        running_tab: 0,
        running_tab_identity: old_identity,
        running_message_index: host.editor_state().chat.messages.len().saturating_sub(1),
        workspace_epoch: lifetime::workspace_epoch(host.editor_state()),
        abort: None,
        indicator_epoch: None,
        page_edit_fence: None,
        bound_page_id: None,
        document_mutated: false,
    });
    host.editor_state_mut().chat.close_tab(0);
    send_user_message(&mut host, "new tab");
    host.editor_state_mut().chat.pending_send = None;
    let before = host.editor_state().chat.messages.clone();
    assert_ne!(host.editor_state().chat.instance_id(), old_identity);
    runtime.pump(&mut host, 0, TEST_VIEWPORT);
    assert!(runtime.turn.is_none());
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Stopped
    );
    assert_eq!(host.editor_state().chat.messages, before);
}

#[test]
fn rejected_reference_keeps_pinned_design_and_refine_routes_for_retry() {
    use op_editor_core::LaunchRoute;
    for route in [LaunchRoute::Orchestrator, LaunchRoute::Refine] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut host =
            host_with_builtin_provider(&format!("http://{}", listener.local_addr().unwrap()));
        let board = serde_json::from_value(serde_json::json!({
            "type":"frame", "id":"reference-board", "name":"Card", "width":390,"height":844,
            "children": [{"type":"text","id":"heading","content":"Before"}]
        }))
        .unwrap();
        host.editor_state_mut().active_children_mut().clear();
        host.editor_state_mut().active_children_mut().push(board);
        host.editor_state_mut()
            .set_single_selection(op_editor_core::NodeId::new("reference-board"));
        host.editor_state_mut().chat.launch_route = route;
        host.editor_state_mut()
            .chat
            .add_attachment(reference_image());
        send_user_message(&mut host, "Five cards about summer");
        let mut runtime = MobileChatHost::default();
        assert!(runtime.pump(&mut host, 0, TEST_VIEWPORT).is_none());
        assert_eq!(host.editor_state().chat.launch_route, route);
        host.editor_state_mut().chat.pending_attachments.clear();
        assert!(host.editor_state_mut().chat.begin_send());
        assert!(runtime.pump(&mut host, 1, TEST_VIEWPORT).is_some());
        let job = runtime.turn.as_ref().unwrap();
        match route {
            LaunchRoute::Orchestrator => assert!(job.session.is_design_loop()),
            LaunchRoute::Refine => assert_eq!(
                job.page_edit_fence.as_deref(),
                Some(&["reference-board".to_owned()][..])
            ),
            _ => unreachable!(),
        }
        assert_eq!(host.editor_state().chat.launch_route, LaunchRoute::Auto);
        runtime.retire_turn(&mut host);
    }
}

#[test]
fn scoped_modify_discards_queued_write_after_switching_pages() {
    let doc = serde_json::from_value(serde_json::json!({"version":"1.0", "pages":[
        {"id":"first", "name":"First", "children":[]},
        {"id":"second", "name":"Second", "children":[]}
    ]}))
    .unwrap();
    let mut host = WidgetHostNative::new();
    host.replace_editor_state(EditorState::from_document(doc));
    send_user_message(&mut host, "change this page");
    host.editor_state_mut().chat.pending_send = None;
    let (_tx, rx) = mpsc::channel();
    let (tool_tx, tool_rx) = mpsc::channel();
    let (ack_tx, ack_rx) = mpsc::sync_channel(1);
    tool_tx
        .send(ChatToolRequest {
            name: "batch_design".into(),
            args_json: "{}".into(),
            ack: ack_tx,
        })
        .unwrap();
    host.editor_state_mut().editor_ui.workspace.active = true;
    host.editor_state_mut()
        .editor_ui
        .workspace
        .resume_generating(73);
    let mut runtime = MobileChatHost::default();
    runtime.turn = Some(ChatTurnJob {
        session: ChatSession::from_channels(rx, Some(tool_rx)),
        document_identity: (
            host.document_epoch(),
            host.editor_state().document_generation(),
        ),
        running_tab: 0,
        running_tab_identity: host.editor_state().chat.instance_id(),
        running_message_index: host.editor_state().chat.messages.len().saturating_sub(1),
        workspace_epoch: lifetime::workspace_epoch(host.editor_state()),
        abort: None,
        indicator_epoch: None,
        page_edit_fence: Some(vec!["board".into()]),
        bound_page_id: Some(page_identity(host.editor_state())),
        document_mutated: false,
    });
    host.editor_state_mut().ui.active_page_index = 1;
    let before = host.editor_state().doc.clone();
    runtime.pump(&mut host, 0, TEST_VIEWPORT);
    assert!(runtime.turn.is_none());
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Failed
    );
    assert_eq!(host.editor_state().doc, before);
    assert!(matches!(
        ack_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(!last_assistant(&host).streaming);
}
