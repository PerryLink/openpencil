//! Process-local identity for asynchronous conversation ownership.

use std::sync::atomic::{AtomicU64, Ordering};

use super::ChatState;

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

/// Opaque, non-persistent conversation identity. Cloning a state for a snapshot
/// preserves its identity; starting another conversation allocates a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatInstanceId(u64);

impl Default for ChatInstanceId {
    fn default() -> Self {
        Self(
            NEXT_INSTANCE
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                    value.checked_add(1)
                })
                .expect("chat instance identity space exhausted"),
        )
    }
}

impl ChatState {
    pub fn instance_id(&self) -> u64 {
        self.runtime_instance.0
    }
}

#[cfg(test)]
mod tests {
    use crate::chat_sessions::ChatSessions;

    #[test]
    fn closing_a_preceding_tab_does_not_retarget_the_surviving_conversation() {
        let mut chats = ChatSessions::default();
        let removed = chats.instance_id();
        chats.new_tab();
        let running = chats.instance_id();
        assert_ne!(removed, running);
        chats.close_tab(0);
        assert_eq!(chats.instance_id(), running);
        assert!(!chats.tabs().iter().any(|tab| tab.instance_id() == removed));
    }

    #[test]
    fn replacing_the_last_tab_or_starting_new_chat_invalidates_old_identity() {
        let mut chats = ChatSessions::default();
        let original = chats.instance_id();
        assert_eq!(chats.clone().instance_id(), original);
        chats.close_tab(0);
        let replacement = chats.instance_id();
        assert_ne!(replacement, original);
        chats.new_chat();
        assert_ne!(chats.instance_id(), replacement);
    }
}
