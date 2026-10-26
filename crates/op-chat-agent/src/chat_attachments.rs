//! Bounded inline image content shared by mobile chat and design loops.
use base64::Engine as _;
use op_ai::chat_provider::ChatAttachment;
use op_editor_core::BuiltinAgentKind;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentError {
    UnsupportedImage,
    TooLarge,
    ModelIsTextOnly,
}
impl std::fmt::Display for AttachmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsupportedImage => "Reference images must be nonempty PNG, JPEG, GIF, or WebP files",
            Self::TooLarge => "Reference images exceed the attachment size or count limit",
            Self::ModelIsTextOnly => "The selected model does not support reference images. Choose an image-capable model or remove the attachments",
        })
    }
}
impl std::error::Error for AttachmentError {}

pub fn validate(attachments: &[ChatAttachment]) -> Result<(), AttachmentError> {
    if attachments.len() > op_editor_core::chat::MAX_ATTACHMENTS
        || attachments
            .iter()
            .any(|a| a.data.len() > op_editor_core::chat::MAX_ATTACHMENT_BYTES)
    {
        return Err(AttachmentError::TooLarge);
    }
    if attachments.iter().any(|a| {
        a.data.is_empty()
            || !matches!(
                a.media_type.as_str(),
                "image/png" | "image/jpeg" | "image/gif" | "image/webp"
            )
    }) {
        return Err(AttachmentError::UnsupportedImage);
    }
    Ok(())
}

pub fn validate_model(model: &str, attachments: &[ChatAttachment]) -> Result<(), AttachmentError> {
    validate(attachments)?;
    if !attachments.is_empty() && !crate::chat_agent_context::model_supports_inline_images(model) {
        return Err(AttachmentError::ModelIsTextOnly);
    }
    Ok(())
}

pub fn user_content(
    kind: BuiltinAgentKind,
    prompt: &str,
    attachments: &[ChatAttachment],
) -> Result<Value, AttachmentError> {
    validate(attachments)?;
    if attachments.is_empty() {
        return Ok(Value::String(prompt.to_owned()));
    }
    let mut blocks = vec![json!({"type": "text", "text": prompt})];
    for image in attachments {
        let data = base64::engine::general_purpose::STANDARD.encode(&image.data);
        blocks.push(match kind {
            BuiltinAgentKind::OpenAiCompat => json!({"type":"image_url", "image_url": {
                "url": format!("data:{};base64,{data}", image.media_type)}}),
            BuiltinAgentKind::Anthropic => json!({"type":"image", "source": {
                "type":"base64", "media_type":image.media_type, "data":data}}),
        });
    }
    Ok(Value::Array(blocks))
}
