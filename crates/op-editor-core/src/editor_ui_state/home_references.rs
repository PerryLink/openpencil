//! Shared native/web transitions for Home's staged image references.

use crate::{EditorState, HomeHit};

impl EditorState {
    pub fn apply_home_reference_action(&mut self, hit: HomeHit) {
        let home = &mut self.editor_ui.home;
        match hit {
            HomeHit::References => {
                home.references_open = !self.chat.pending_attachments.is_empty();
                home.more_open = false;
                home.composer_focused = false;
            }
            HomeHit::ReferenceRemove(index) if home.references_open => {
                self.chat.remove_attachment(index);
                home.references_open = !self.chat.pending_attachments.is_empty();
            }
            HomeHit::ReferencesClose => home.references_open = false,
            _ => {}
        }
    }
}
