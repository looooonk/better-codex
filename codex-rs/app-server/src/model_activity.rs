//! Observe generation without blocking inference or sending tool arguments to clients.
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadStateManager;
use codex_app_server_protocol::TurnModelActivity;
use codex_app_server_protocol::TurnModelActivityNotification;
use codex_extension_api::ModelRequestContributor;
use codex_extension_api::ModelRequestInput;
use codex_extension_api::ModelRequestKind;
use codex_extension_api::ModelResponseInterceptor;
use codex_extension_api::ModelResponseStream;
use codex_extension_api::ResponseEvent;
use codex_protocol::ThreadId;
use codex_protocol::models::ResponseItem;
use codex_protocol::output_tokens::estimate_tokens;
use futures::StreamExt;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;

pub(crate) struct ModelActivityObserver(pub(crate) ThreadStateManager);

impl std::fmt::Debug for ModelActivityObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ModelActivityObserver")
    }
}

impl ModelRequestContributor for ModelActivityObserver {
    fn request(&self, input: ModelRequestInput<'_>) -> Option<Box<dyn ModelResponseInterceptor>> {
        if input.kind != ModelRequestKind::Generation {
            return None;
        }
        let thread_id = ThreadId::from_string(input.thread_id).ok()?;
        let observer = RequestActivity {
            thread_id,
            turn_id: input.turn_id?.to_owned(),
            listener: self.0.current_listener_command_tx(thread_id)?,
        };
        observer.send(TurnModelActivity::Generating);
        Some(Box::new(observer))
    }
}

struct RequestActivity {
    thread_id: ThreadId,
    turn_id: String,
    listener: UnboundedSender<ThreadListenerCommand>,
}

impl RequestActivity {
    fn send(&self, activity: TurnModelActivity) {
        let _ = self.listener.send(ThreadListenerCommand::ModelActivity(
            TurnModelActivityNotification {
                thread_id: self.thread_id.to_string(),
                turn_id: self.turn_id.clone(),
                activity,
            },
        ));
    }
}

impl ModelResponseInterceptor for RequestActivity {
    fn intercept(self: Box<Self>, upstream: ModelResponseStream) -> ModelResponseStream {
        let mut calls_tool = false;
        let mut previous_item = String::new();
        let mut previous_char = '\0';
        let mut pending_tokens = 0.0;
        let mut last_sent = None;
        Box::pin(upstream.inspect(move |result| {
            let now = Instant::now();
            if let Ok(ResponseEvent::ToolCallInputDelta { item_id, delta, .. }) = result {
                if previous_item != *item_id {
                    previous_char = '\0';
                    previous_item.clone_from(item_id);
                }
                pending_tokens += estimate_tokens(delta, &mut previous_char);
                if last_sent.is_some_and(|at| now.duration_since(at) < Duration::from_millis(100)) {
                    return;
                }
            }

            // Flush before item/completion boundaries even if a bucket is still partial.
            if pending_tokens > 0.0 {
                self.send(TurnModelActivity::ToolInput {
                    estimated_tokens: std::mem::take(&mut pending_tokens),
                });
                last_sent = Some(now);
            }
            match result {
                Ok(ResponseEvent::OutputItemDone(item)) => {
                    calls_tool |= matches!(
                        item,
                        ResponseItem::FunctionCall { .. }
                            | ResponseItem::CustomToolCall { .. }
                            | ResponseItem::LocalShellCall { .. }
                    ) || matches!(item, ResponseItem::ToolSearchCall { execution, .. } if execution == "client");
                }
                Ok(ResponseEvent::Completed { .. }) if calls_tool => {
                    self.send(TurnModelActivity::WaitingForTool);
                }
                _ => {}
            }
        }))
    }
}

#[cfg(test)]
#[path = "model_activity_tests.rs"]
mod tests;
