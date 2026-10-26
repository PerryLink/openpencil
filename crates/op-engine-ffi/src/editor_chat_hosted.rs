//! Hosted selection stays separate from saved API-key and external-agent routes.
use super::*;
use op_ai::chat_provider::{ChatAttachment, ChatProvider, ChatRequest};

#[allow(clippy::too_many_arguments)]
pub(super) fn launch_if_selected(
    owner: &mut MobileChatHost,
    host: &mut WidgetHostNative,
    user_text: &str,
    prompt: &str,
    fence: Option<Vec<String>>,
    launch_route: op_editor_core::LaunchRoute,
    running_tab: usize,
) -> bool {
    let Some(route_id) = host
        .editor_state()
        .chat
        .selected_model_entry()
        .and_then(|entry| entry.hosted_route_id())
        .map(str::to_owned)
    else {
        return false;
    };
    let provider = match op_hosted_models::provider_for_route(&route_id) {
        Ok(provider) => provider,
        Err(message) => {
            host.editor_state_mut().chat.set_input_text(user_text);
            write_turn_error(host, running_tab, message.to_string());
            return true;
        }
    };
    let attachments = host.editor_state().chat.pending_attachments.clone();
    let design = fence.is_some()
        || launch_route.implies_design_intent()
        || (design_intent(user_text) && run_ws::permits_design(host, user_text));
    if design && fence.is_none() {
        match owner
            .hosted_design
            .start(host, provider, prompt.to_owned(), attachments, running_tab)
        {
            Ok(()) => {
                host.editor_state_mut().chat.pending_attachments.clear();
                host.editor_state_mut().chat.launch_route = Default::default();
            }
            Err(message) => {
                host.editor_state_mut().chat.set_input_text(user_text);
                write_turn_error(host, running_tab, message.to_string());
            }
        }
        return true;
    }
    let launched = if let Some(targets) = fence.as_ref() {
        modify::start_provider_modify_turn(
            host.editor_state(),
            provider,
            user_text,
            targets.clone(),
            attachments,
            None,
            running_tab,
        )
    } else {
        Ok(start_provider_turn(
            host.editor_state(),
            provider,
            prompt,
            attachments,
            running_tab,
        ))
    };
    match launched {
        Ok(mut job) => {
            host.editor_state_mut().chat.pending_attachments.clear();
            host.editor_state_mut().chat.launch_route = Default::default();
            job.running_tab_identity = host.editor_state().chat.instance_id();
            job.running_message_index = host.editor_state().chat.messages.len().saturating_sub(1);
            job.document_identity = (
                host.document_epoch(),
                host.editor_state().document_generation(),
            );
            job.bound_page_id = fence.as_ref().map(|_| page_identity(host.editor_state()));
            job.page_edit_fence = fence;
            run_ws::stamp_run_epoch(host, job.indicator_epoch);
            job.workspace_epoch = lifetime::workspace_epoch(host.editor_state());
            owner.turn = Some(job);
        }
        Err(message) => {
            host.editor_state_mut().chat.set_input_text(user_text);
            write_turn_error(host, running_tab, message.to_string());
        }
    }
    true
}

pub(super) fn start_provider_turn(
    state: &EditorState,
    provider: Box<dyn ChatProvider>,
    prompt: &str,
    attachments: Vec<ChatAttachment>,
    running_tab: usize,
) -> ChatTurnJob {
    let request = ChatRequest {
        user_message: prompt.to_owned(),
        history: trim_chat_history(
            &chat_history_from_transcript(&state.chat.messages),
            DEFAULT_MAX_MESSAGES,
            DEFAULT_MAX_CHARS,
        ),
        max_output_tokens: CHAT_MAX_OUTPUT_TOKENS,
        thinking: state.chat.thinking_mode,
        effort: state.chat.effort_level,
        attachments,
        // Hosted identity resolves only through its opaque catalog route.
        model: None,
        ..Default::default()
    };
    ChatTurnJob {
        session: ChatSession::start(provider, request),
        document_identity: (0, 0),
        running_tab,
        running_tab_identity: state.chat.instance_id(),
        running_message_index: state.chat.messages.len().saturating_sub(1),
        workspace_epoch: lifetime::workspace_epoch(state),
        abort: None,
        indicator_epoch: None,
        page_edit_fence: None,
        bound_page_id: None,
        document_mutated: false,
    }
}
