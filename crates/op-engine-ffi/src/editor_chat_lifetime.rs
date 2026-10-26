//! Reject stale mobile chat work before touching its bound document or run.
use super::*;

pub(super) fn workspace_epoch(state: &EditorState) -> Option<u64> {
    let workspace = &state.editor_ui.workspace;
    (workspace.active && workspace.phase == op_editor_core::WorkspacePhase::Generating)
        .then_some(workspace.run_epoch)
}

impl MobileChatHost {
    pub(super) fn drop_stale_turn(&mut self, host: &mut WidgetHostNative) -> bool {
        let live = (
            host.document_epoch(),
            host.editor_state().document_generation(),
        );
        if self
            .turn
            .as_ref()
            .is_some_and(|job| job.document_identity != live)
        {
            self.drop_turn();
            return false;
        }
        let Some(job) = self.turn.as_mut() else {
            return false;
        };
        let index = host
            .editor_state()
            .chat
            .tabs()
            .iter()
            .position(|tab| tab.instance_id() == job.running_tab_identity);
        let Some(index) = index else {
            let epoch = job.workspace_epoch;
            self.drop_turn();
            return settle_aborted_workspace(host, epoch, false);
        };
        job.running_tab = index;
        let auth_changed =
            !op_hosted_models::session_epoch_is_current(job.session.hosted_session_epoch());
        let page_changed = job
            .bound_page_id
            .as_ref()
            .is_some_and(|id| *id != page_identity(host.editor_state()));
        if !auth_changed && !page_changed {
            return false;
        }
        let locale = host.editor_state().editor_ui.locale;
        let message = if auth_changed {
            op_editor_core::hosted_model_catalog::hosted_transport_unavailable_message(locale)
        } else {
            op_chat_agent::workspace_edit::scope_unavailable_message(locale)
        };
        let epoch = job.workspace_epoch;
        let tab = &mut host.editor_state_mut().chat.tabs_mut()[index];
        if let Some(reply) = tab.messages.get_mut(job.running_message_index) {
            reply.streaming = false;
            if !reply.content.is_empty() {
                reply.content.push_str("\n\n");
            }
            reply.content.push_str(&format!("error: {message}"));
            let completion = reply
                .completion
                .get_or_insert(op_editor_core::ChatCompletion {
                    succeeded: 0,
                    failed: 0,
                    nodes: 0,
                });
            completion.failed = completion.failed.max(1);
        }
        tab.agents_running = (0, 0);
        self.drop_turn();
        settle_aborted_workspace(host, epoch, true);
        true
    }
}

fn settle_aborted_workspace(host: &mut WidgetHostNative, epoch: Option<u64>, failed: bool) -> bool {
    let Some(epoch) = epoch else {
        return false;
    };
    let workspace = &mut host.editor_state_mut().editor_ui.workspace;
    if !workspace.active || workspace.run_epoch != epoch {
        return false;
    }
    if failed {
        workspace.mark_failed(epoch)
    } else {
        workspace.mark_stopped(epoch)
    }
}
