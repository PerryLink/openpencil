//! Hosted whole-design orchestration without desktop daemons or CLI agents.
//!
//! The worker owns a snapshot. Only the engine thread can apply its commands,
//! fenced by the account, document lifetime, chat-tab identity and target page.

use std::sync::{mpsc, Arc};

use op_ai::chat_provider::{ChatAttachment, ChatProvider};
use op_editor_core::{ChatRole, EditorState};
use op_editor_host_core::design::{
    DesignCmdAck, DesignCmdOp, DesignCmdReq, DesignDelta, DesignSession, RemoteDocSink,
};
use op_host_native::WidgetHostNative;
use op_orchestrator::{
    AbortFlag, DesignRequest, Orchestrator, OrchestratorError, SkippedPreValidator,
    SkippedScreenshotProvider, SkippedVisionLlmClient, ValidationProviders,
};

use crate::editor_chat_design::{
    clear_fresh_starter_frame_for_design, reconcile_starter_ghost, root_seed_hints,
};
use crate::editor_chat_workspace as workspace;

#[path = "editor_chat_hosted_design_llm.rs"]
mod llm;
#[path = "editor_chat_hosted_design_progress.rs"]
mod progress;

struct Job {
    session: DesignSession,
    document_identity: (u64, u64),
    tab_identity: u64,
    message_index: usize,
    target_page_id: String,
    run_epoch: u64,
}

#[derive(Default)]
pub(crate) struct MobileHostedDesignHost {
    job: Option<Job>,
}

fn document_identity(host: &WidgetHostNative) -> (u64, u64) {
    (
        host.document_epoch(),
        host.editor_state().document_generation(),
    )
}
fn tab_index(state: &EditorState, identity: u64) -> Option<usize> {
    state
        .chat
        .tabs()
        .iter()
        .position(|tab| tab.instance_id() == identity)
}
fn page_index(state: &EditorState, id: &str) -> Option<usize> {
    match state.doc.pages.as_ref() {
        Some(pages) if !pages.is_empty() => pages.iter().position(|page| page.id == id),
        _ if id == "0" => Some(0),
        _ => None,
    }
}
fn target_page_id(state: &EditorState) -> String {
    state
        .doc
        .pages
        .as_ref()
        .and_then(|pages| pages.get(state.ui.active_page_index))
        .map(|page| page.id.clone())
        .unwrap_or_else(|| "0".into())
}

impl MobileHostedDesignHost {
    pub(crate) fn is_running(&self) -> bool {
        self.job.is_some()
    }

    pub(crate) fn targets_visible_page(&self, host: &WidgetHostNative) -> bool {
        self.job.as_ref().is_some_and(|job| {
            page_index(host.editor_state(), &job.target_page_id)
                == Some(host.editor_state().ui.active_page_index)
        })
    }

    pub(crate) fn start(
        &mut self,
        host: &mut WidgetHostNative,
        provider: Box<dyn ChatProvider>,
        prompt: String,
        attachments: Vec<ChatAttachment>,
        running_tab: usize,
    ) -> Result<(), OrchestratorError> {
        if !attachments.is_empty() {
            return Err(OrchestratorError::Internal(
                op_hosted_models::HostedRuntimeError::UnsupportedAttachments.to_string(),
            ));
        }
        if !provider.supports_cancellable_send() || !provider.supports_evidence_only_send() {
            return Err(OrchestratorError::Internal(
                "Hosted design requires a cancellable text-only provider".into(),
            ));
        }
        let hosted_epoch = provider.hosted_session_epoch();
        if !op_hosted_models::session_epoch_is_current(hosted_epoch) {
            return Err(OrchestratorError::Internal("Hosted session changed".into()));
        }
        if !allow_write(host) {
            return Err(OrchestratorError::Internal(
                "design generation is unavailable for this collaboration session".into(),
            ));
        }
        let state = host.editor_state();
        let tab = state.chat.tabs().get(running_tab).ok_or_else(|| {
            OrchestratorError::Internal("design conversation is no longer available".into())
        })?;
        let message_index = tab
            .messages
            .iter()
            .rposition(|message| message.role == ChatRole::Assistant && message.streaming)
            .ok_or_else(|| {
                OrchestratorError::Internal("design conversation is no longer available".into())
            })?;
        let tab_identity = tab.instance_id();
        let target_page_id = target_page_id(state);
        let request = DesignRequest {
            prompt: prompt.clone(),
            // This is only model-capability policy, never a transport override.
            model: tab
                .selected_model_entry()
                .and_then(|entry| entry.hosted.as_ref())
                .map(|route| route.model_id().into()),
            design_md: state.doc.design_md.clone(),
            concurrency: tab.agent_team_size,
            pinned_style_guide: state.editor_ui.pinned_style_guide.clone(),
            // This mobile transport has no screenshot/vision validation input.
            validation_enabled: false,
            ..Default::default()
        };
        let mut snapshot =
            op_editor_core::request_snapshot::narrowed_snapshot(host.editor_state_mut());
        clear_fresh_starter_frame_for_design(&mut snapshot);
        let hints = root_seed_hints(&snapshot, &prompt);
        let indicator_epoch =
            op_editor_core::agent_indicators::begin_with_root_seed_hint(hints.0, hints.1);
        let abort = AbortFlag::new();
        let worker_abort = abort.clone();
        let (delta_tx, delta_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let provider = llm::ProviderLlm(Arc::from(provider));
        if let Err(error) = std::thread::Builder::new()
            .name("op-mobile-hosted-design".into())
            .spawn(move || {
                let mut sink = RemoteDocSink::new(cmd_tx, snapshot);
                let validation = ValidationProviders {
                    pre_validator: &SkippedPreValidator,
                    screenshot: &SkippedScreenshotProvider,
                    vision: &SkippedVisionLlmClient,
                    system_prompt: String::new(),
                };
                let result = op_chat_agent::runtime::block_on_anywhere(async {
                    Orchestrator::new()
                        .with_indicator_epoch(indicator_epoch)
                        .run(
                            request,
                            &mut sink,
                            &provider,
                            &mut |event| {
                                let _ = delta_tx.send(DesignDelta::Progress(event));
                            },
                            &worker_abort,
                            &validation,
                        )
                        .await
                });
                let _ = delta_tx.send(DesignDelta::Done(result));
            })
        {
            op_editor_core::agent_indicators::end_if_epoch(indicator_epoch);
            return Err(OrchestratorError::Internal(format!(
                "failed to start design worker: {error}"
            )));
        }
        clear_fresh_starter_frame_for_design(host.editor_state_mut());
        if let Some(tab) = host.editor_state_mut().chat.tabs_mut().get_mut(running_tab) {
            tab.agents_running = (1, 1);
        }
        workspace::stamp_run_epoch(host, Some(indicator_epoch));
        self.job = Some(Job {
            session: DesignSession::from_channels_with_epoch_and_abort(
                delta_rx,
                cmd_rx,
                indicator_epoch,
                abort,
            )
            .with_hosted_session_epoch(hosted_epoch),
            document_identity: document_identity(host),
            tab_identity,
            message_index,
            target_page_id,
            run_epoch: indicator_epoch,
        });
        host.mark_editor_state_dirty();
        Ok(())
    }

    /// Reject queued writes before draining either channel. A new document
    /// must not receive even transcript cleanup from the retired document.
    pub(crate) fn drop_stale(&mut self, host: &mut WidgetHostNative) -> bool {
        let Some(job) = self.job.as_ref() else {
            return false;
        };
        if job.document_identity != document_identity(host) {
            self.job = None;
            return false;
        }
        if tab_index(host.editor_state(), job.tab_identity).is_none() {
            let job = self.job.take().expect("checked above");
            let workspace = &mut host.editor_state_mut().editor_ui.workspace;
            let owns_run = workspace.active
                && workspace.phase == op_editor_core::WorkspacePhase::Generating
                && workspace.run_epoch == job.run_epoch;
            if owns_run {
                workspace.mark_stopped(job.run_epoch);
                reconcile_starter_ghost(host.editor_state_mut(), false);
            }
            return owns_run;
        }
        if !op_hosted_models::session_epoch_is_current(job.session.hosted_session_epoch()) {
            return self.retire(host, Some("Hosted session changed"));
        }
        if page_index(host.editor_state(), &job.target_page_id).is_none() {
            return self.retire(host, Some("design page is no longer available"));
        }
        false
    }

    pub(crate) fn cancel(&mut self, host: &mut WidgetHostNative) -> bool {
        self.retire(host, None)
    }

    fn retire(&mut self, host: &mut WidgetHostNative, error: Option<&str>) -> bool {
        let Some(job) = self.job.take() else {
            return false;
        };
        job.session.abort();
        if job.document_identity != document_identity(host) {
            return false;
        }
        let locale = host.editor_state().editor_ui.locale;
        if let Some(index) = tab_index(host.editor_state(), job.tab_identity) {
            let tab = &mut host.editor_state_mut().chat.tabs_mut()[index];
            tab.agents_running = (0, 0);
            if let Some(message) = tab.messages.get_mut(job.message_index) {
                message.streaming = false;
                if let Some(error) = error {
                    message.content.push_str(&format!("\n\nerror: {error}"));
                    message.completion = Some(op_editor_core::ChatCompletion {
                        succeeded: 0,
                        failed: 1,
                        nodes: 0,
                    });
                }
                for item in &mut message.activities {
                    if matches!(
                        item.status,
                        op_editor_core::ChatActivityStatus::Running
                            | op_editor_core::ChatActivityStatus::Pending
                    ) {
                        item.status = op_editor_core::ChatActivityStatus::Error;
                        item.detail = Some(
                            op_i18n::translate(locale, "ai.designProgress.detail.stoppedByUser")
                                .into(),
                        );
                    }
                }
            }
        }
        if error.is_some() {
            settle_run(host, &job, true, (0.0, 0.0));
        }
        if owns_starter_ghost(host, &job) {
            reconcile_starter_ghost(host.editor_state_mut(), false);
        }
        true
    }

    pub(crate) fn pump(&mut self, host: &mut WidgetHostNative, viewport: (f32, f32)) -> bool {
        let mut changed = self.drop_stale(host);
        let Some(job) = self.job.as_mut() else {
            return changed;
        };
        for req in job.session.drain_cmd_requests() {
            // Authentication can change on another thread during a frame.
            if !op_hosted_models::session_epoch_is_current(job.session.hosted_session_epoch()) {
                drop(req);
                changed |= self.retire(host, Some("Hosted session changed"));
                return changed;
            }
            changed |= apply_request(host, req, viewport);
        }
        if !op_hosted_models::session_epoch_is_current(job.session.hosted_session_epoch()) {
            return self.retire(host, Some("Hosted session changed")) || changed;
        }
        let poll = job.session.poll_progress();
        let locale = host.editor_state().editor_ui.locale;
        if let Some(index) = tab_index(host.editor_state(), job.tab_identity) {
            let tab = &mut host.editor_state_mut().chat.tabs_mut()[index];
            if let Some(message) = tab.messages.get_mut(job.message_index) {
                for event in &poll.progress {
                    progress::fold(message, event, locale);
                }
                if poll.finished {
                    progress::finish(message, poll.summary.as_ref(), locale);
                }
                changed |= !poll.progress.is_empty() || poll.finished;
            }
            if poll.finished {
                tab.agents_running = (0, 0);
            }
        }
        if owns_starter_ghost(host, job) {
            changed |= reconcile_starter_ghost(host.editor_state_mut(), !poll.finished);
        }
        if poll.finished {
            let failed = match poll.summary.as_ref() {
                Some(Ok(summary)) => {
                    summary.incomplete_subtask_failure
                        || !summary.unfilled_screens.is_empty()
                        || summary.subtasks.iter().any(|task| task.error.is_some())
                }
                Some(Err(_)) | None => true,
            };
            changed |= settle_run(host, job, failed, viewport);
            self.job = None;
        }
        changed
    }
}

fn owns_starter_ghost(host: &WidgetHostNative, job: &Job) -> bool {
    let workspace = &host.editor_state().editor_ui.workspace;
    // Ordinary editor chat has no workspace run. While a workspace is active,
    // its ghost belongs exclusively to the stamped job, including epoch zero.
    !workspace.active || workspace.run_epoch == job.run_epoch
}

fn settle_run(host: &mut WidgetHostNative, job: &Job, failed: bool, viewport: (f32, f32)) -> bool {
    let workspace = &host.editor_state().editor_ui.workspace;
    // This host stamps every started job. Zero now denotes a different run
    // queued by Home, not permission for a retired worker to settle it.
    if !workspace.active
        || workspace.phase != op_editor_core::WorkspacePhase::Generating
        || workspace.run_epoch != job.run_epoch
    {
        return false;
    }
    let Some(target) = page_index(host.editor_state(), &job.target_page_id) else {
        return host
            .editor_state_mut()
            .editor_ui
            .workspace
            .mark_failed(job.run_epoch);
    };
    let original = host.editor_state().ui.active_page_index;
    host.editor_state_mut().ui.active_page_index = target;
    let boards = op_editor_core::workspace_run::produced_board_count(host.editor_state());
    host.editor_state_mut().ui.active_page_index = original;
    if target == original {
        host.settle_workspace_idle_edge(job.run_epoch, boards, failed, viewport.0, viewport.1)
    } else {
        // A background-page result may settle its run, but must not move the
        // user's camera or fit the unrelated page currently being viewed.
        let workspace = &mut host.editor_state_mut().editor_ui.workspace;
        if !workspace.active || workspace.phase != op_editor_core::WorkspacePhase::Generating {
            return false;
        }
        if failed || boards == 0 {
            workspace.mark_failed(job.run_epoch)
        } else {
            workspace.mark_done(job.run_epoch)
        }
    }
}

fn apply_request(host: &mut WidgetHostNative, req: DesignCmdReq, viewport: (f32, f32)) -> bool {
    let target = req
        .target_page_id
        .as_deref()
        .and_then(|id| page_index(host.editor_state(), id));
    let old_page = host.editor_state().ui.active_page_index;
    let old_selection = host.editor_state().selection.clone();
    let available = req.target_page_id.is_none() || target.is_some();
    if let Some(target) = target {
        let state = host.editor_state_mut();
        state.ui.active_page_index = target;
        if target != old_page {
            state.clear_selection();
        }
    }
    let applied = if !available {
        false
    } else {
        match req.op {
            DesignCmdOp::Apply(command) => {
                let allowed = allow_write(host);
                allowed && host.editor_state_mut().apply(command)
            }
            // The shared editor does not yet expose batched history boundaries.
            DesignCmdOp::BeginUndoBatch | DesignCmdOp::EndUndoBatch => true,
        }
    };
    let snapshot = op_editor_core::request_snapshot::narrowed_snapshot(host.editor_state_mut());
    if target.is_some_and(|target| target != old_page) {
        host.editor_state_mut().ui.active_page_index = old_page;
        host.editor_state_mut().selection = old_selection;
    }
    let _ = req.ack.send(DesignCmdAck {
        applied,
        new_state: snapshot,
    });
    if applied {
        host.mark_editor_state_dirty();
        if target.is_none_or(|target| target == old_page) {
            host.fit_content_to_viewport(viewport.0, viewport.1);
        }
    }
    applied
}

fn allow_write(host: &mut WidgetHostNative) -> bool {
    host.gate_collaboration_action(
        op_editor_core::CollabGateAction::Document(
            op_editor_core::CollabDocumentMutation::Unsupported(
                op_editor_core::CollabUnsupportedFeature::BulkWrite,
            ),
        ),
        op_editor_core::CollabEditSource::Ai,
    )
}

#[cfg(test)]
#[path = "editor_chat_hosted_design_tests.rs"]
mod tests;
