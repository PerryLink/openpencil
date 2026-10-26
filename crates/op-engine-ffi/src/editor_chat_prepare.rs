//! Selected mobile provider snapshots and labels.
use super::*;

/// Resolve the selected chat model into a runnable built-in turn snapshot.
/// `None` when the selection is not a ready built-in (API-key) entry.
pub(super) fn prepare_builtin_turn(
    state: &EditorState,
    user_text: &str,
) -> Option<BuiltinChatTurn> {
    let entry = state.chat.selected_model_entry()?;
    let id = entry.builtin_provider_id.as_deref()?;
    let selected_model = entry.builtin_model_id()?;
    let config = state
        .editor_ui
        .agent_settings
        .builtin_agents
        .iter()
        .find(|agent| agent.id == id && agent.ready())?;
    if !config.has_model(selected_model) {
        return None;
    }
    let base_url = if config.base_url.trim().is_empty() {
        config.kind.default_base_url().to_string()
    } else {
        config.base_url.trim().to_string()
    };
    // Per-turn knobs the chat panel carries. The BuiltIn wire has no
    // separate effort channel, so a non-default effort rides in-band as a
    // leading directive (desktop parity).
    let mut prompt = user_text.to_string();
    let effort = state.chat.effort_level;
    if effort != EffortLevel::Low {
        prompt = format!("Apply {} reasoning effort.\n\n{prompt}", effort.as_str());
    }
    // Prior transcript turns, trimmed by the shared sliding-window policy.
    // `begin_send` already pushed this turn's user message + empty
    // assistant bubble; the transcript mapper excludes both.
    let history = trim_chat_history(
        &chat_history_from_transcript(&state.chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    Some(BuiltinChatTurn {
        kind: config.kind,
        api_key: config.api_key.trim().to_string(),
        model: selected_model.to_string(),
        base_url,
        // Plain turns carry no system prompt (the desktop's context-rich
        // chat prompt builder lives in op-host-services, which mobile
        // deliberately does not link). Design turns swap in the shared
        // design-agent prompt at the launch site.
        system_prompt: String::new(),
        history,
        prompt,
        attachments: state.chat.pending_attachments.clone(),
        max_output_tokens: CHAT_MAX_OUTPUT_TOKENS,
    })
}

/// Display name for the honest-error path, mirroring the desktop's
/// `selected_provider_label`.
pub(super) fn selected_provider_label(state: &EditorState) -> String {
    if let Some(entry) = state.chat.selected_model_entry() {
        if let Some(id) = entry.builtin_provider_id.as_deref() {
            if let Some(agent) = state
                .editor_ui
                .agent_settings
                .builtin_agents
                .iter()
                .find(|agent| agent.id == id)
            {
                return agent.display_name.clone();
            }
        }
        if let Some(id) = entry.acp_agent_id() {
            if let Some(agent) = state
                .editor_ui
                .agent_settings
                .acp_agents
                .iter()
                .find(|agent| agent.id == id)
            {
                return agent.display_name.clone();
            }
        }
    }
    op_editor_core::AgentProvider::ALL
        .get(state.editor_ui.chat_selected_agent)
        .map(|agent| agent.name().to_string())
        .unwrap_or_else(|| "This agent".into())
}

#[derive(Debug)]
pub(super) enum AttachmentSelectionError {
    Image(op_chat_agent::chat_attachments::AttachmentError),
    UnsupportedSelection,
}
impl AttachmentSelectionError {
    pub(super) fn message(&self, locale: op_i18n::Locale) -> &'static str {
        use op_chat_agent::chat_attachments::AttachmentError;
        let key = match self {
            Self::Image(AttachmentError::UnsupportedImage) => "ai.referenceImagesInvalid",
            Self::Image(AttachmentError::TooLarge) => "ai.referenceImagesTooLarge",
            Self::Image(AttachmentError::ModelIsTextOnly) | Self::UnsupportedSelection => {
                "ai.referenceImagesUnsupported"
            }
        };
        op_i18n::translate(locale, key)
    }
}
impl From<op_chat_agent::chat_attachments::AttachmentError> for AttachmentSelectionError {
    fn from(value: op_chat_agent::chat_attachments::AttachmentError) -> Self {
        Self::Image(value)
    }
}

/// Reject unsupported image turns before consuming staged bytes or contacting a provider.
pub(super) fn validate_selected_attachments(
    state: &EditorState,
) -> Result<(), AttachmentSelectionError> {
    let attachments = &state.chat.pending_attachments;
    op_chat_agent::chat_attachments::validate(attachments)?;
    if attachments.is_empty() {
        return Ok(());
    }
    let entry = state
        .chat
        .selected_model_entry()
        .ok_or(AttachmentSelectionError::UnsupportedSelection)?;
    if let Some(route_id) = entry.hosted_route_id() {
        let capable = state
            .editor_ui
            .hosted_model_routes()
            .iter()
            .find(|route| route.route_id() == route_id)
            .is_some_and(|route| route.capabilities().image_input);
        return if capable {
            Ok(())
        } else {
            Err(AttachmentSelectionError::UnsupportedSelection)
        };
    }
    if let Some(model) = entry.builtin_model_id() {
        op_chat_agent::chat_attachments::validate_model(model, attachments)?;
    }
    Ok(())
}
