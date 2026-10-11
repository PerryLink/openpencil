//! Chat-attachment file picker — the rfd-backed host half.
//!
//! The headless attachment helpers (base64, temp-file spill, prompt
//! building, the Claude image-Read flow) live in
//! [`op_host_services::chat_attachment`]; this module keeps only the
//! native file picker that stages a chosen image on the chat state.

use std::{fs, path::Path};

use op_editor_core::chat::{ChatAttachment, MAX_ATTACHMENTS, MAX_ATTACHMENT_BYTES};
use op_host_native::WidgetHostNative;
use op_host_services::chat_attachment::media_type_for_path;

/// Drain `chat.pending_attachment_pick` (raised by the attach
/// button): open a native image picker and stage the chosen files on
/// `chat.pending_attachments`. Returns true when an attachment was
/// added (the caller redraws).
pub fn drain_attachment_pick(host: &mut WidgetHostNative) -> bool {
    if !std::mem::take(&mut host.editor_state_mut().chat.pending_attachment_pick) {
        return false;
    }
    let Some(paths) = rfd::FileDialog::new()
        .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
        .pick_files()
    else {
        return false;
    };
    stage_images(host, paths.iter().map(|path| path.as_path()))
}

fn stage_images<'a>(
    host: &mut WidgetHostNative,
    paths: impl IntoIterator<Item = &'a Path>,
) -> bool {
    let mut added = false;
    for path in paths {
        if host.editor_state().chat.pending_attachments.len() >= MAX_ATTACHMENTS {
            break;
        }
        added |= stage_image(host, path);
    }
    added
}

fn stage_image(host: &mut WidgetHostNative, path: &Path) -> bool {
    // Reject an oversized file before reading it fully into memory.
    if let Ok(meta) = fs::metadata(path) {
        if meta.len() as usize > MAX_ATTACHMENT_BYTES {
            return false;
        }
    }
    let Ok(data) = fs::read(path) else {
        return false;
    };
    if data.len() > MAX_ATTACHMENT_BYTES {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let media_type = media_type_for_path(path);
    // `add_attachment` enforces the per-turn attachment-count cap.
    host.editor_state_mut().chat.add_attachment(ChatAttachment {
        name,
        media_type,
        data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_images_skip_unreadable_and_oversized_files_and_keep_the_count_cap() {
        let root = std::env::temp_dir().join(format!("op-reference-batch-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let large = root.join("large.png");
        fs::File::create(&large)
            .unwrap()
            .set_len(MAX_ATTACHMENT_BYTES as u64 + 1)
            .unwrap();
        let mut paths = vec![root.join("missing.png"), large];
        for i in 0..MAX_ATTACHMENTS + 1 {
            let path = root.join(format!("image-{i}.png"));
            fs::write(&path, [1, 2, 3]).unwrap();
            paths.push(path);
        }
        let mut host = WidgetHostNative::new();
        assert!(stage_images(&mut host, paths.iter().map(|p| p.as_path())));
        let attachments = &host.editor_state().chat.pending_attachments;
        assert_eq!(attachments.len(), MAX_ATTACHMENTS);
        assert_eq!(attachments[0].name, "image-0.png");
        assert_eq!(attachments[3].name, "image-3.png");
        assert!(!stage_images(&mut host, paths.iter().map(|p| p.as_path())));
        fs::remove_dir_all(root).unwrap();
    }
}
