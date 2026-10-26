use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::StreamExt;
use op_ai::chat_provider::{ChatDelta, ChatRequest, StopReason};
use op_editor_core::{ChatMessage, EditorCommand, NodeId, PenNodeExt};
use op_orchestrator::{CallRequest, LlmClient, RunSummary};

const VIEWPORT: (f32, f32) = (390.0, 760.0);

fn host() -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    host.editor_state_mut().active_children_mut().clear();
    host.editor_state_mut().chat.messages = vec![
        ChatMessage::user("create a page"),
        ChatMessage::assistant_streaming(),
    ];
    host
}
fn insert_command() -> EditorCommand {
    EditorCommand::InsertSubtree { nodes: vec![serde_json::from_str(r##"{"type":"frame","id":"mobile-result","name":"Hosted result","width":390,"height":844,"fill":[{"type":"solid","color":"#ffffff"}]}"##).unwrap()], parent_id: NodeId::NONE, page_id: None }
}
fn channels(
    host: &WidgetHostNative,
    epoch: Option<u64>,
) -> (
    MobileHostedDesignHost,
    mpsc::Sender<DesignDelta>,
    mpsc::Sender<DesignCmdReq>,
    AbortFlag,
) {
    let (delta_tx, delta_rx) = mpsc::channel();
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let abort = AbortFlag::new();
    let session =
        DesignSession::from_channels_with_epoch_and_abort(delta_rx, cmd_rx, 0, abort.clone())
            .with_hosted_session_epoch(epoch);
    (
        MobileHostedDesignHost {
            job: Some(Job {
                session,
                document_identity: document_identity(host),
                tab_identity: host.editor_state().chat.instance_id(),
                message_index: 1,
                target_page_id: target_page_id(host.editor_state()),
                run_epoch: host.editor_state().editor_ui.workspace.run_epoch,
            }),
        },
        delta_tx,
        cmd_tx,
        abort,
    )
}
fn queue_command(tx: &mpsc::Sender<DesignCmdReq>, page: &str) -> mpsc::Receiver<DesignCmdAck> {
    let (ack, rx) = mpsc::sync_channel(1);
    tx.send(DesignCmdReq {
        op: DesignCmdOp::Apply(insert_command()),
        target_page_id: Some(page.into()),
        ack,
    })
    .unwrap();
    rx
}

#[test]
fn real_command_and_terminal_summary_fold_into_bound_tab() {
    let mut host = host();
    let (mut pump, delta_tx, cmd_tx, _) = channels(&host, None);
    let rx = queue_command(&cmd_tx, &target_page_id(host.editor_state()));
    host.editor_state_mut().chat.new_tab();
    delta_tx
        .send(DesignDelta::Done(Ok(RunSummary {
            root_frame_id: "root".into(),
            subtasks: Vec::new(),
            total_nodes: 1,
            unfilled_screens: Vec::new(),
            incomplete_subtask_failure: false,
        })))
        .unwrap();
    assert!(pump.pump(&mut host, VIEWPORT));
    assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().applied);
    assert_eq!(
        host.editor_state().active_children()[0]
            .base()
            .name
            .as_deref(),
        Some("Hosted result")
    );
    assert!(!host.editor_state().chat.tabs()[0].messages[1].streaming);
    assert!(host.editor_state().chat.messages.is_empty());
    assert!(!pump.is_running());
}

#[test]
fn stale_account_discards_queued_writes_and_stops_original_bubble() {
    let mut host = host();
    let (mut pump, _delta_tx, cmd_tx, abort) = channels(&host, Some(u64::MAX));
    let rx = queue_command(&cmd_tx, &target_page_id(host.editor_state()));
    pump.pump(&mut host, VIEWPORT);
    assert!(abort.is_set());
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    assert!(host.editor_state().active_children().is_empty());
    assert!(!host.editor_state().chat.messages[1].streaming);
}

#[test]
fn closed_tab_does_not_fall_back_to_the_next_tab() {
    let mut host = host();
    let (mut pump, _delta_tx, cmd_tx, abort) = channels(&host, None);
    let rx = queue_command(&cmd_tx, &target_page_id(host.editor_state()));
    host.editor_state_mut().chat.new_tab();
    host.editor_state_mut().chat.messages = vec![
        ChatMessage::user("create a page"),
        ChatMessage::assistant_streaming(),
    ];
    host.editor_state_mut().chat.close_tab(0);
    let messages = host.editor_state().chat.messages.clone();
    pump.pump(&mut host, VIEWPORT);
    assert!(abort.is_set());
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    assert_eq!(host.editor_state().chat.messages, messages);
    assert!(host.editor_state().active_children().is_empty());
}

#[test]
fn replaced_document_discards_work_without_transcript_cleanup() {
    let mut host = host();
    let (mut pump, _delta_tx, cmd_tx, abort) = channels(&host, None);
    let rx = queue_command(&cmd_tx, &target_page_id(host.editor_state()));
    pump.job.as_mut().unwrap().document_identity.0 = u64::MAX;
    let before = host.editor_state().chat.messages.clone();
    pump.pump(&mut host, VIEWPORT);
    assert!(abort.is_set());
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    assert_eq!(host.editor_state().chat.messages, before);
    assert!(host.editor_state().active_children().is_empty());
}

#[test]
fn disconnected_worker_terminates_spinner_with_error() {
    let mut host = host();
    let (mut pump, delta_tx, _cmd_tx, _) = channels(&host, None);
    drop(delta_tx);
    pump.pump(&mut host, VIEWPORT);
    assert!(!pump.is_running());
    let message = &host.editor_state().chat.messages[1];
    assert!(!message.streaming);
    assert!(message.content.contains("error:"));
}

struct ScriptedProvider {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<ChatRequest>>>,
}
impl ChatProvider for ScriptedProvider {
    fn provider_label(&self) -> &str {
        "fixture"
    }
    fn supports_cancellable_send(&self) -> bool {
        true
    }
    fn supports_evidence_only_send(&self) -> bool {
        true
    }
    fn send(&self, _: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        panic!("must use cancellable send")
    }
    fn send_cancellable(
        &self,
        request: ChatRequest,
        _: Arc<AtomicBool>,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.seen.lock().unwrap().push(request);
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let text = if index == 0 {
            r##"{"sharedContext":{},"rootFrame":{"id":"root","name":"Page","width":390,"height":844,"layout":"vertical","gap":0,"fill":[{"type":"solid","color":"#FFFFFF"}]},"subtasks":[{"id":"hero","label":"Hero","region":{"width":390,"height":300}}]}"##.to_string()
        } else {
            r##"I(null, {"type":"frame","name":"Content","width":390,"height":300,"children":[{"type":"text","content":"Hosted canvas output","fontSize":24}]});"##.to_string()
        };
        Box::new(
            vec![
                ChatDelta::TextDelta(text),
                ChatDelta::Done {
                    stop_reason: StopReason::EndTurn,
                },
            ]
            .into_iter(),
        )
    }
}

#[test]
fn generic_provider_runs_real_orchestrator_and_changes_canvas() {
    let mut host = host();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider = ScriptedProvider {
        calls: calls.clone(),
        seen: seen.clone(),
    };
    let mut design = MobileHostedDesignHost::default();
    design
        .start(
            &mut host,
            Box::new(provider),
            "Create a mobile page with a hero".into(),
            Vec::new(),
            0,
        )
        .unwrap();
    let start = Instant::now();
    while design.is_running() {
        design.pump(&mut host, VIEWPORT);
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "design worker did not terminate"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(calls.load(Ordering::SeqCst) >= 2);
    let document = serde_json::to_string(&host.editor_state().doc).unwrap();
    assert!(document.contains("Hosted canvas output"), "{document}");
    assert!(!host.editor_state().chat.messages[1].streaming);
    assert!(host.editor_state().chat.messages[1].completion.is_some());
    for request in seen.lock().unwrap().iter() {
        assert!(request.model.is_none());
        assert!(!request.system_prompt.is_empty());
    }
}

struct SilentProvider {
    started: mpsc::Sender<()>,
    stopped: mpsc::Sender<()>,
}
impl ChatProvider for SilentProvider {
    fn provider_label(&self) -> &str {
        "silent"
    }
    fn supports_cancellable_send(&self) -> bool {
        true
    }
    fn supports_evidence_only_send(&self) -> bool {
        true
    }
    fn send(&self, _: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        panic!("must use cancellation")
    }
    fn send_cancellable(
        &self,
        _: ChatRequest,
        cancel: Arc<AtomicBool>,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.started.send(()).unwrap();
        let stopped = self.stopped.clone();
        Box::new(std::iter::from_fn(move || {
            while !cancel.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(1));
            }
            let _ = stopped.send(());
            None
        }))
    }
}
fn request(abort: AbortFlag) -> CallRequest {
    CallRequest {
        system_prompt: "planner".into(),
        user_prompt: "brief".into(),
        model: None,
        provider: None,
        timeout: Duration::from_secs(2),
        abort,
        no_text_timeout: None,
        first_text_timeout: None,
    }
}

#[test]
fn dropping_adapter_stream_cancels_silent_transport() {
    let (started, start_rx) = mpsc::channel();
    let (stopped, stop_rx) = mpsc::channel();
    let provider = llm::ProviderLlm(Arc::new(SilentProvider { started, stopped }));
    let stream = provider.call(request(AbortFlag::new()));
    start_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    drop(stream);
    stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn design_abort_cancels_silent_transport_and_finishes_stream() {
    let (started, start_rx) = mpsc::channel();
    let (stopped, stop_rx) = mpsc::channel();
    let abort = AbortFlag::new();
    let provider = llm::ProviderLlm(Arc::new(SilentProvider { started, stopped }));
    let stream = provider.call(request(abort.clone()));
    start_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    abort.set();
    stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let chunks: Vec<_> = op_chat_agent::runtime::block_on_anywhere(stream.collect());
    assert!(chunks
        .iter()
        .any(|chunk| matches!(chunk, Err(error) if error.aborted)));
}

#[test]
fn stop_whole_design_cancels_silent_paid_request() {
    let mut host = host();
    let (started, start_rx) = mpsc::channel();
    let (stopped, stop_rx) = mpsc::channel();
    let mut design = MobileHostedDesignHost::default();
    design
        .start(
            &mut host,
            Box::new(SilentProvider { started, stopped }),
            "Create a mobile page".into(),
            Vec::new(),
            0,
        )
        .unwrap();
    start_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(design.cancel(&mut host));
    stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(!design.is_running());
    assert!(!host.editor_state().chat.messages[1].streaming);
    assert!(host.editor_state().active_children().is_empty());
}

#[test]
fn unsupported_reference_image_is_rejected_before_worker_or_document_mutation() {
    let mut host = host();
    let (started, start_rx) = mpsc::channel();
    let (stopped, _stop_rx) = mpsc::channel();
    let before = host.editor_state().doc.clone();
    let mut design = MobileHostedDesignHost::default();
    let attachment = ChatAttachment {
        name: "reference.png".into(),
        media_type: "image/png".into(),
        data: vec![1, 2, 3],
    };
    assert!(design
        .start(
            &mut host,
            Box::new(SilentProvider { started, stopped }),
            "Use this reference".into(),
            vec![attachment],
            0
        )
        .is_err());
    assert!(!design.is_running());
    assert!(start_rx.try_recv().is_err());
    assert_eq!(host.editor_state().doc, before);
}

#[test]
fn total_deadline_cancels_silent_paid_request() {
    let (started, start_rx) = mpsc::channel();
    let (stopped, stop_rx) = mpsc::channel();
    let provider = llm::ProviderLlm(Arc::new(SilentProvider { started, stopped }));
    let mut req = request(AbortFlag::new());
    req.timeout = Duration::from_millis(60);
    let stream = provider.call(req);
    start_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    stop_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let chunks: Vec<_> = op_chat_agent::runtime::block_on_anywhere(stream.collect());
    assert!(chunks.iter().any(
        |chunk| matches!(chunk, Err(error) if error.message.contains("timed out") && !error.aborted)
    ));
}

#[test]
fn minimax_m3_policy_matches_desktop_without_transport_model_override() {
    use op_ai::chat_provider::ThinkingMode;
    for (model, thinking, budget) in [
        (Some("MiniMax-M3"), ThinkingMode::Adaptive, 24576),
        (Some("minimax/MiniMax-M3"), ThinkingMode::Adaptive, 24576),
        (Some("MiniMax-M2.7"), ThinkingMode::Disabled, 16384),
        (Some("deepseek-chat"), ThinkingMode::Disabled, 16384),
        (
            Some("acp:minimax-m3-wrapper"),
            ThinkingMode::Disabled,
            16384,
        ),
        (None, ThinkingMode::Disabled, 16384),
    ] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let provider = llm::ProviderLlm(Arc::new(ScriptedProvider {
            calls: Arc::new(AtomicUsize::new(0)),
            seen: seen.clone(),
        }));
        let mut req = request(AbortFlag::new());
        req.model = model.map(str::to_owned);
        let _: Vec<_> = op_chat_agent::runtime::block_on_anywhere(provider.call(req).collect());
        let sent = seen.lock().unwrap();
        assert_eq!(sent[0].thinking, thinking, "{model:?}");
        assert_eq!(sent[0].max_output_tokens, budget, "{model:?}");
        assert!(
            sent[0].model.is_none(),
            "model identity must remain capability-only"
        );
    }
}

fn workspace_host() -> WidgetHostNative {
    let mut host = host();
    host.editor_state_mut().doc.pages = Some(serde_json::from_str(r##"[
      {"id":"original-page","name":"Original","children":[{"type":"frame","id":"output","name":"Hosted result","width":390,"height":844,"children":[{"type":"text","id":"copy","content":"Done"}]}]},
      {"id":"other-page","name":"Other","children":[]}
    ]"##).unwrap());
    let workspace = &mut host.editor_state_mut().editor_ui.workspace;
    workspace.active = true;
    workspace.phase = op_editor_core::WorkspacePhase::Generating;
    workspace.run_epoch = 99;
    host
}

#[test]
fn background_tab_error_after_narration_settles_run_failed() {
    let mut host = workspace_host();
    host.editor_state_mut().chat.messages[1].content = "Preparing the layout".into();
    let (mut pump, delta_tx, _cmd_tx, _) = channels(&host, None);
    host.editor_state_mut().chat.new_tab();
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant("unrelated successful reply"));
    delta_tx
        .send(DesignDelta::Done(Err(OrchestratorError::AllFailed(
            "fixture failure".into(),
        ))))
        .unwrap();
    pump.pump(&mut host, VIEWPORT);
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Failed
    );
    let message = &host.editor_state().chat.tabs()[0].messages[1];
    assert!(message.content.starts_with("Preparing"));
    assert_eq!(message.completion.unwrap().failed, 1);
    host.editor_state_mut().chat.switch_to(0);
    assert!(op_editor_core::workspace_run::last_assistant_failed(
        host.editor_state()
    ));
}

#[test]
fn background_page_success_uses_origin_boards_and_not_active_tab_error() {
    let mut host = workspace_host();
    let (mut pump, delta_tx, _cmd_tx, _) = channels(&host, None);
    host.editor_state_mut().chat.new_tab();
    host.editor_state_mut()
        .chat
        .messages
        .push(ChatMessage::assistant("error: unrelated failure"));
    host.editor_state_mut().ui.active_page_index = 1;
    let viewport = host.editor_state().viewport;
    delta_tx
        .send(DesignDelta::Done(Ok(RunSummary {
            root_frame_id: "output".into(),
            subtasks: Vec::new(),
            total_nodes: 2,
            unfilled_screens: Vec::new(),
            incomplete_subtask_failure: false,
        })))
        .unwrap();
    pump.pump(&mut host, VIEWPORT);
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Done
    );
    assert_eq!(host.editor_state().ui.active_page_index, 1);
    assert_eq!(host.editor_state().viewport, viewport);
    assert!(!host.editor_state().chat.tabs()[0].messages[1].streaming);
}

#[test]
fn command_ack_keeps_origin_page_when_user_views_another_page() {
    let mut host = workspace_host();
    let (mut pump, _delta_tx, cmd_tx, _) = channels(&host, None);
    let rx = queue_command(&cmd_tx, "original-page");
    host.editor_state_mut().ui.active_page_index = 1;
    assert!(!pump.targets_visible_page(&host));
    pump.pump(&mut host, VIEWPORT);
    let ack = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(ack.applied);
    assert_eq!(ack.new_state.ui.active_page_index, 0);
    assert_eq!(host.editor_state().ui.active_page_index, 1);
    assert!(host.editor_state().active_children().is_empty());
    assert_eq!(
        host.editor_state().doc.pages.as_ref().unwrap()[0]
            .children
            .len(),
        2
    );
}

#[test]
fn deleted_origin_page_retires_job_without_numeric_index_fallback() {
    let mut host = workspace_host();
    let (mut pump, _delta_tx, cmd_tx, abort) = channels(&host, None);
    let rx = queue_command(&cmd_tx, "original-page");
    host.editor_state_mut()
        .doc
        .pages
        .as_mut()
        .unwrap()
        .remove(0);
    pump.pump(&mut host, VIEWPORT);
    assert!(abort.is_set());
    assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    assert!(host.editor_state().active_children().is_empty());
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Failed
    );
}

#[test]
fn closed_running_tab_stops_only_its_own_workspace_epoch() {
    for current_epoch in [99, 100, 0] {
        let mut host = workspace_host();
        let (mut pump, _delta_tx, cmd_tx, abort) = channels(&host, None);
        let rx = queue_command(&cmd_tx, "original-page");
        host.editor_state_mut().chat.new_tab();
        host.editor_state_mut().chat.close_tab(0);
        host.editor_state_mut()
            .editor_ui
            .workspace
            .resume_generating(current_epoch);
        let ghost = Some([10.0, 20.0, 390.0, 844.0]);
        host.editor_state_mut().editor_ui.starter_ghost = ghost;
        let viewport = host.editor_state().viewport;
        pump.pump(&mut host, VIEWPORT);
        assert!(abort.is_set());
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
        assert!(host.editor_state().chat.messages.is_empty());
        assert_eq!(host.editor_state().viewport, viewport);
        assert_eq!(
            host.editor_state().editor_ui.starter_ghost,
            if current_epoch == 99 { None } else { ghost }
        );
        assert_eq!(
            host.editor_state().editor_ui.workspace.phase,
            if current_epoch != 99 {
                op_editor_core::WorkspacePhase::Generating
            } else {
                op_editor_core::WorkspacePhase::Stopped
            }
        );
    }
}

#[test]
fn old_completion_cannot_settle_a_new_unstamped_workspace_run() {
    for fail in [false, true] {
        for background_page in [false, true] {
            let mut host = workspace_host();
            let (mut pump, delta_tx, _cmd_tx, _) = channels(&host, None);
            host.editor_state_mut()
                .editor_ui
                .workspace
                .resume_generating(0);
            let ghost = Some([10.0, 20.0, 390.0, 844.0]);
            host.editor_state_mut().editor_ui.starter_ghost = ghost;
            if background_page {
                host.editor_state_mut().ui.active_page_index = 1;
            }
            let viewport = host.editor_state().viewport;
            let result = if fail {
                Err(OrchestratorError::AllFailed("old run failed".into()))
            } else {
                Ok(RunSummary {
                    root_frame_id: "output".into(),
                    subtasks: Vec::new(),
                    total_nodes: 2,
                    unfilled_screens: Vec::new(),
                    incomplete_subtask_failure: false,
                })
            };
            delta_tx.send(DesignDelta::Done(result)).unwrap();
            pump.pump(&mut host, VIEWPORT);
            assert!(!pump.is_running());
            assert_eq!(
                host.editor_state().editor_ui.workspace.phase,
                op_editor_core::WorkspacePhase::Generating
            );
            assert_eq!(host.editor_state().editor_ui.workspace.run_epoch, 0);
            assert_eq!(host.editor_state().viewport, viewport);
            assert_eq!(host.editor_state().editor_ui.starter_ghost, ghost);
        }
    }
}

#[test]
fn stale_account_cannot_clear_a_new_unstamped_run_ghost() {
    let mut host = workspace_host();
    let (mut pump, _delta_tx, _cmd_tx, abort) = channels(&host, Some(u64::MAX));
    host.editor_state_mut()
        .editor_ui
        .workspace
        .resume_generating(0);
    let ghost = Some([10.0, 20.0, 390.0, 844.0]);
    host.editor_state_mut().editor_ui.starter_ghost = ghost;
    let viewport = host.editor_state().viewport;
    pump.pump(&mut host, VIEWPORT);
    assert!(abort.is_set());
    assert!(!pump.is_running());
    assert_eq!(
        host.editor_state().editor_ui.workspace.phase,
        op_editor_core::WorkspacePhase::Generating
    );
    assert_eq!(host.editor_state().editor_ui.workspace.run_epoch, 0);
    assert_eq!(host.editor_state().editor_ui.starter_ghost, ghost);
    assert_eq!(host.editor_state().viewport, viewport);
}
