//! Compact mobile rendering of the shared orchestrator's typed progress.

use op_editor_core::{
    ChatActivity, ChatActivityStatus as Status, ChatCompletion, ChatMessage, Locale,
};
use op_orchestrator::{OrchestratorError, Progress, RunSummary};

fn activity(msg: &mut ChatMessage, id: &str, title: &str, status: Status, detail: Option<String>) {
    if let Some(item) = msg.activities.iter_mut().find(|item| item.id == id) {
        if !title.is_empty() {
            item.title = title.into();
        }
        item.status = status;
        item.detail = detail;
    } else {
        msg.activities.push(ChatActivity {
            id: id.into(),
            title: title.into(),
            status,
            detail,
            content_offset: Some(msg.content.len().min(u32::MAX as usize) as u32),
        });
    }
}
fn narrate(msg: &mut ChatMessage, text: &str) {
    if !msg.content.is_empty() {
        msg.content.push_str("\n\n");
    }
    msg.content.push_str(text);
}
pub(super) fn fold(msg: &mut ChatMessage, event: &Progress, locale: Locale) {
    match event {
        Progress::Planning => {
            narrate(
                msg,
                op_i18n::translate(locale, "ai.designProgress.narration.planning"),
            );
            activity(
                msg,
                "__planning",
                op_i18n::translate(locale, "ai.designProgress.activity.planning"),
                Status::Running,
                None,
            );
        }
        Progress::Planned { subtasks } => {
            msg.activities.retain(|item| item.id != "__planning");
            for (id, title) in subtasks {
                activity(msg, id, title, Status::Pending, None);
            }
        }
        Progress::SubtaskStarted { id, label } => activity(msg, id, label, Status::Running, None),
        Progress::SubtaskDone { id, .. } => activity(msg, id, "", Status::Done, None),
        Progress::SubtaskFailed { id, error } => {
            activity(msg, id, "", Status::Error, Some(error.clone()))
        }
        Progress::SubtaskIncomplete { id, .. } | Progress::SubtaskLanguageMismatch { id, .. } => {
            activity(msg, id, "", Status::Error, None)
        }
        Progress::SubtaskRetry { id, attempt, .. } => activity(
            msg,
            id,
            "",
            Status::Running,
            Some(
                op_i18n::translate(locale, "ai.designProgress.detail.retrying")
                    .replace("{{attempt}}", &attempt.to_string()),
            ),
        ),
        Progress::WorkerScoped(worker) => fold(msg, &worker.event, locale),
        Progress::CleanupDone => narrate(
            msg,
            op_i18n::translate(locale, "ai.designProgress.narration.polishing"),
        ),
        Progress::RunAborted { reason } => narrate(msg, &format!("error: {reason}")),
        _ => {}
    }
}
pub(super) fn finish(
    msg: &mut ChatMessage,
    result: Option<&Result<RunSummary, OrchestratorError>>,
    locale: Locale,
) {
    msg.streaming = false;
    match result {
        Some(Ok(summary)) => {
            let succeeded = summary
                .subtasks
                .iter()
                .filter(|task| task.error.is_none())
                .count();
            let failed = summary.subtasks.len() - succeeded;
            let incomplete = failed > 0
                || summary.incomplete_subtask_failure
                || !summary.unfilled_screens.is_empty();
            msg.completion = Some(ChatCompletion {
                succeeded: succeeded as u32,
                failed: failed.max(usize::from(incomplete)) as u32,
                nodes: summary.total_nodes.min(u32::MAX as usize) as u32,
            });
            let key = if incomplete {
                "ai.designProgress.completion.issues"
            } else if succeeded == 1 {
                "ai.designProgress.completion.one"
            } else {
                "ai.designProgress.completion.many"
            };
            let text = op_i18n::translate(locale, key)
                .replace("{{count}}", &succeeded.to_string())
                .replace("{{completed}}", &succeeded.to_string())
                .replace(
                    "{{failed}}",
                    &failed.max(usize::from(incomplete)).to_string(),
                );
            narrate(msg, &text);
            if incomplete {
                narrate(msg, "error: design contains incomplete sections");
            }
        }
        Some(Err(error)) => {
            msg.completion = Some(ChatCompletion {
                succeeded: 0,
                failed: 1,
                nodes: 0,
            });
            narrate(msg, &format!("error: {error}"));
        }
        None => {
            msg.completion = Some(ChatCompletion {
                succeeded: 0,
                failed: 1,
                nodes: 0,
            });
            narrate(msg, "error: design worker disconnected before completion");
        }
    }
    for item in &mut msg.activities {
        if matches!(item.status, Status::Pending | Status::Running) {
            item.status = Status::Error;
        }
    }
}
