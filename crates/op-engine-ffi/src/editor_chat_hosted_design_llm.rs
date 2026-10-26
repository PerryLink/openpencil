//! Mobile-safe text-completion adapter for the design orchestrator.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use futures::channel::mpsc;
use futures::stream::{BoxStream, Stream};
use op_ai::chat_provider::{
    ChatDelta, ChatProvider, ChatRequest, EffortLevel, StopReason, ThinkingMode,
};
use op_orchestrator::{CallRequest, LlmChunk, LlmClient, LlmError};

pub(super) struct ProviderLlm(pub Arc<dyn ChatProvider>);

/// Keep the desktop adapter's MiniMax-M3 reasoning budget. Model identity
/// affects capability policy only; Hosted owns the actual transport route.
pub(super) fn thinking_policy(model: Option<&str>) -> (ThinkingMode, u32) {
    let m3 = model
        .filter(|model| !op_orchestrator::is_acp_capability_marker(model))
        .is_some_and(|model| model.to_ascii_lowercase().contains("minimax-m3"));
    if m3 {
        (ThinkingMode::Adaptive, 24576)
    } else {
        (ThinkingMode::Disabled, 16384)
    }
}

/// Dropping a timed-out planner/subagent stream must cancel its individual
/// paid transport without aborting other concurrent screen-group requests.
struct CancellableStream {
    rx: mpsc::UnboundedReceiver<Result<LlmChunk, LlmError>>,
    cancel: Arc<AtomicBool>,
}
impl Stream for CancellableStream {
    type Item = Result<LlmChunk, LlmError>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.rx).poll_next(cx)
    }
}
impl Drop for CancellableStream {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl LlmClient for ProviderLlm {
    fn call(&self, req: CallRequest) -> BoxStream<'static, Result<LlmChunk, LlmError>> {
        let (tx, rx) = mpsc::unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let stream = CancellableStream {
            rx,
            cancel: cancel.clone(),
        };
        if req.abort.is_set() {
            let _ = tx.unbounded_send(Err(LlmError {
                message: "aborted".into(),
                aborted: true,
            }));
            return Box::pin(stream);
        }
        let (thinking, max_output_tokens) = thinking_policy(req.model.as_deref());
        let request = ChatRequest {
            system_prompt: req.system_prompt,
            user_message: req.user_prompt,
            history: Vec::new(),
            max_output_tokens,
            thinking,
            effort: EffortLevel::Low,
            attachments: Vec::new(),
            // Hosted routing/auth is encapsulated by the provider. An opaque
            // Hosted route must never become a provider-specific model flag.
            model: None,
        };
        let finished = Arc::new(AtomicBool::new(false));
        let watch_finished = finished.clone();
        let watch_cancel = cancel.clone();
        let watch_tx = tx.clone();
        op_chat_agent::runtime::shared_runtime().spawn(async move {
            let started = Instant::now();
            while !watch_finished.load(Ordering::Acquire) && !watch_cancel.load(Ordering::Acquire) {
                let aborted = req.abort.is_set();
                if aborted || started.elapsed() >= req.timeout {
                    watch_cancel.store(true, Ordering::Release);
                    let _ = watch_tx.unbounded_send(Err(LlmError {
                        message: if aborted {
                            "aborted"
                        } else {
                            "design request timed out"
                        }
                        .into(),
                        aborted,
                    }));
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        });
        let provider = self.0.clone();
        let failure_tx = tx.clone();
        let failure_finished = finished.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("op-mobile-design-llm".into())
            .spawn(move || {
                struct Finished(Arc<AtomicBool>);
                impl Drop for Finished {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::Release);
                    }
                }
                let _finished = Finished(finished);
                let mut terminal = false;
                for delta in provider.send_cancellable(request, cancel.clone()) {
                    if cancel.load(Ordering::Acquire) {
                        return;
                    }
                    let chunk = match delta {
                        ChatDelta::TextDelta(text) => Ok(LlmChunk::Text(text)),
                        ChatDelta::Thinking(text) => Ok(LlmChunk::Thinking(text)),
                        ChatDelta::Done {
                            stop_reason: StopReason::EndTurn,
                        } => {
                            terminal = true;
                            break;
                        }
                        ChatDelta::Done { stop_reason } => {
                            let _ = tx.unbounded_send(Err(LlmError {
                                message: format!("design completion stopped: {stop_reason:?}"),
                                aborted: stop_reason == StopReason::Aborted,
                            }));
                            return;
                        }
                        ChatDelta::Error(message) => {
                            let _ = tx.unbounded_send(Err(LlmError {
                                message,
                                aborted: false,
                            }));
                            return;
                        }
                        ChatDelta::ToolUse { .. } => {
                            let _ = tx.unbounded_send(Err(LlmError {
                                message: "design completion unexpectedly requested a tool".into(),
                                aborted: false,
                            }));
                            return;
                        }
                    };
                    if tx.unbounded_send(chunk).is_err() {
                        return;
                    }
                }
                if !terminal && !cancel.load(Ordering::Acquire) {
                    let _ = tx.unbounded_send(Err(LlmError {
                        message: "design stream ended before completion".into(),
                        aborted: false,
                    }));
                }
            })
        {
            failure_finished.store(true, Ordering::Release);
            let _ = failure_tx.unbounded_send(Err(LlmError {
                message: format!("failed to start design transport: {error}"),
                aborted: false,
            }));
        }
        Box::pin(stream)
    }
}
