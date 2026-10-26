//! Mobile transport adapter for the shared structured-modification runner.
//! No design tool catalog, planning loop, layout normalization or finalize.

use super::*;
use op_ai::chat_provider::{ChatProvider, ChatRequest, ChatToolResult, ThinkingMode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug)]
pub(super) enum ModifyLaunchError {
    RuntimeUnavailable,
    TargetUnavailable,
}
impl std::fmt::Display for ModifyLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RuntimeUnavailable => "chat runtime is unavailable",
            Self::TargetUnavailable => "the page to edit is no longer available",
        })
    }
}
impl std::error::Error for ModifyLaunchError {}

struct MobileModifyProvider {
    turn: BuiltinChatTurn,
    runtime: &'static Runtime,
}

impl ChatProvider for MobileModifyProvider {
    fn provider_label(&self) -> &str {
        "mobile structured edit"
    }

    fn supports_cancellable_send(&self) -> bool {
        true
    }

    fn supports_evidence_only_send(&self) -> bool {
        true
    }

    fn send(&self, request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        self.send_cancellable(request, Arc::new(AtomicBool::new(false)))
    }

    fn send_cancellable(
        &self,
        request: ChatRequest,
        cancel: Arc<AtomicBool>,
    ) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        let (tx, rx) = mpsc::channel();
        let disable_thinking = request.thinking == ThinkingMode::Disabled;
        let turn = BuiltinChatTurn {
            kind: self.turn.kind,
            api_key: self.turn.api_key.clone(),
            model: request.model.unwrap_or_else(|| self.turn.model.clone()),
            base_url: self.turn.base_url.clone(),
            system_prompt: request.system_prompt,
            history: request.history,
            prompt: request.user_message,
            attachments: request.attachments,
            max_output_tokens: request.max_output_tokens,
        };
        let task = self
            .runtime
            .spawn(crate::editor_chat_turn::run_builtin_turn_with_thinking(
                turn,
                tx,
                disable_thinking,
            ));
        Box::new(ModifyStream {
            rx,
            cancel,
            abort: task.abort_handle(),
        })
    }
}

struct ModifyStream {
    rx: mpsc::Receiver<ChatDelta>,
    cancel: Arc<AtomicBool>,
    abort: AbortHandle,
}

impl Iterator for ModifyStream {
    type Item = ChatDelta;
    fn next(&mut self) -> Option<Self::Item> {
        while !self.cancel.load(Ordering::Acquire) {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(delta) => return Some(delta),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            }
        }
        self.abort.abort();
        None
    }
}

impl Drop for ModifyStream {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

pub(super) fn start_modify_turn(
    state: &EditorState,
    turn: BuiltinChatTurn,
    instruction: &str,
    target_frame_ids: Vec<String>,
    running_tab: usize,
) -> Result<ChatTurnJob, String> {
    let runtime = chat_runtime().map_err(|error| format!("error: {error}"))?;
    let attachments = turn.attachments.clone();
    let model = Some(turn.model.clone());
    let provider = Box::new(MobileModifyProvider { turn, runtime });
    start_provider_modify_turn(
        state,
        provider,
        instruction,
        target_frame_ids,
        attachments,
        model,
        running_tab,
    )
    .map_err(|error| format!("error: {error}"))
}

pub(super) fn start_provider_modify_turn(
    state: &EditorState,
    provider: Box<dyn ChatProvider>,
    instruction: &str,
    target_frame_ids: Vec<String>,
    attachments: Vec<op_ai::chat_provider::ChatAttachment>,
    model: Option<String>,
    running_tab: usize,
) -> Result<ChatTurnJob, ModifyLaunchError> {
    let plan = op_chat_agent::chat_modify::build_modify_plan_for_targets(
        state,
        instruction,
        target_frame_ids,
    )
    .ok_or(ModifyLaunchError::TargetUnavailable)?;
    let runtime = chat_runtime().map_err(|_| ModifyLaunchError::RuntimeUnavailable)?;
    let hosted_epoch = provider.hosted_session_epoch();
    let request = ChatRequest {
        system_prompt: plan.system_prompt,
        user_message: plan.user_message,
        model,
        attachments,
        max_output_tokens: 8192,
        thinking: ThinkingMode::Disabled,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let (executor, tool_rx) = chat_tool_channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let epoch = op_editor_core::agent_indicators::begin();
    let task = runtime.spawn_blocking(move || {
        op_chat_agent::chat_modify::run_modify_turn_cancellable(
            provider.as_ref(),
            request,
            &tx,
            &executor,
            plan.target_frame_ids,
            worker_cancel,
        );
    });
    Ok(ChatTurnJob {
        session: ChatSession::from_channels_with_cancel(rx, Some(tool_rx), cancel)
            .with_hosted_session_epoch(hosted_epoch),
        document_identity: (0, 0),
        running_tab,
        running_tab_identity: 0,
        running_message_index: 0,
        workspace_epoch: None,
        abort: Some(task.abort_handle()),
        indicator_epoch: Some(epoch),
        page_edit_fence: None,
        bound_page_id: None,
        document_mutated: false,
    })
}

pub(super) fn apply_modification(state: &mut EditorState, args: &str) -> (ChatToolResult, bool) {
    use op_chat_agent::chat_canvas_tools::*;
    let nodes = parse_design_modification_ops_arg(args);
    let targets = parse_design_modification_target_frame_ids_arg(args);
    let (count, mutated) = apply_design_modification(state, &nodes, &targets);
    (
        ChatToolResult {
            content: serde_json::json!({"success": true, "count": count}).to_string(),
            is_error: false,
        },
        mutated,
    )
}

#[cfg(test)]
#[path = "editor_chat_modify_tests.rs"]
mod tests;
