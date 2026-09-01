use std::sync::Arc;
use std::time::Instant;

use crate::Prompt;
use crate::client::ModelClientSession;
use crate::client_common::ResponseEvent;
use crate::context::world_state::WorldState;
use crate::hook_runtime::PostCompactHookOutcome;
use crate::hook_runtime::PreCompactHookOutcome;
use crate::hook_runtime::run_post_compact_hooks;
use crate::hook_runtime::run_pre_compact_hooks;
use crate::responses_metadata::CodexResponsesMetadata;
use crate::responses_metadata::CodexResponsesRequestKind;
use crate::responses_metadata::CompactionTurnMetadata;
#[cfg(test)]
use crate::session::PreviousTurnSettings;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn::get_last_assistant_message_from_turn;
use crate::session::turn_context::TurnContext;
use crate::util::backoff;
use codex_analytics::CodexCompactionEvent;
use codex_analytics::CompactionImplementation;
use codex_analytics::CompactionPhase;
use codex_analytics::CompactionReason;
use codex_analytics::CompactionStatus;
use codex_analytics::CompactionStrategy;
use codex_analytics::CompactionTrigger;
use codex_analytics::now_unix_seconds;
use codex_history::CodexHarnessMetadata;
use codex_history::CompactedItem;
use codex_history::ResponseItemEnvelope;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result as CodexResult;
use codex_protocol::items::ContextCompactionItem;
use codex_protocol::items::TurnItem;
use codex_protocol::models::ContentItem;
use codex_protocol::models::InternalChatMessageMetadataPassthrough;
use codex_protocol::models::ResponseInputItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::RawResponseCompletedEvent;
use codex_protocol::protocol::TurnContextItem;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use codex_rollout_trace::InferenceTraceContext;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::approx_token_count;
use codex_utils_output_truncation::truncate_text;
use futures::prelude::*;
use tracing::error;

use codex_model_provider_info::ModelProviderInfo;

pub use codex_prompts::SUMMARIZATION_PROMPT;
pub use codex_prompts::SUMMARY_PREFIX;
const COMPACT_USER_MESSAGE_MAX_TOKENS: usize = 20_000;

/// Controls whether compaction replacement history must include initial context.
///
/// Manual compaction uses `DoNotInject`: it replaces history with a summary and clears
/// `reference_context_item`, so the next regular turn will fully reinject initial context.
/// Pre-turn compaction uses `AfterSummary` to reinstall the already prepared current-turn context
/// and lossless user input after the compacted history.
///
/// Mid-turn compaction must use `BeforeLastUserMessage` because the model is trained to see the
/// compaction summary as the last item in history after mid-turn compaction; we therefore inject
/// initial context into the replacement history just above the last real user message.
#[derive(Clone, Debug)]
pub(crate) enum InitialContextInjection {
    BeforeLastUserMessage(Arc<WorldState>),
    AfterSummary {
        step_context: Arc<StepContext>,
        world_state: Arc<WorldState>,
        turn_items: Vec<ResponseItemEnvelope>,
    },
    DoNotInject,
}

impl InitialContextInjection {
    pub(crate) fn reference_context_item(
        &self,
        compaction_turn_context: &TurnContext,
    ) -> Option<TurnContextItem> {
        match self {
            Self::BeforeLastUserMessage(_) => Some(compaction_turn_context.to_turn_context_item()),
            Self::AfterSummary { step_context, .. } => {
                Some(step_context.turn.to_turn_context_item())
            }
            Self::DoNotInject => None,
        }
    }
}

pub(crate) async fn build_compaction_initial_context(
    sess: &Session,
    turn_context: &TurnContext,
    initial_context_injection: &InitialContextInjection,
) -> (Vec<ResponseItem>, Option<Arc<WorldState>>) {
    // Return the rendered state with its items so history and its baseline stay identical.
    match initial_context_injection {
        InitialContextInjection::BeforeLastUserMessage(world_state) => {
            let items = sess
                .build_initial_context_with_world_state(turn_context, world_state.as_ref())
                .await;
            (items, Some(Arc::clone(world_state)))
        }
        InitialContextInjection::AfterSummary {
            step_context,
            world_state,
            ..
        } => {
            let items = sess
                .build_reset_initial_context_for_step(step_context, world_state.as_ref())
                .await;
            (items, Some(Arc::clone(world_state)))
        }
        InitialContextInjection::DoNotInject => (Vec::new(), None),
    }
}

pub(crate) fn place_compaction_context(
    compacted_history: Vec<ResponseItem>,
    initial_context: Vec<ResponseItem>,
    turn_context: &TurnContext,
    initial_context_injection: &InitialContextInjection,
) -> Vec<ResponseItem> {
    place_annotated_compaction_context(
        compacted_history
            .into_iter()
            .map(ResponseItemEnvelope::new)
            .collect(),
        initial_context,
        turn_context,
        initial_context_injection,
    )
    .into_iter()
    .map(ResponseItemEnvelope::into_item)
    .collect()
}

fn place_annotated_compaction_context(
    mut compacted_history: Vec<ResponseItemEnvelope>,
    initial_context: Vec<ResponseItem>,
    turn_context: &TurnContext,
    initial_context_injection: &InitialContextInjection,
) -> Vec<ResponseItemEnvelope> {
    let initial_context = initial_context
        .into_iter()
        .map(ResponseItemEnvelope::new)
        .collect();
    match initial_context_injection {
        InitialContextInjection::BeforeLastUserMessage(_) => {
            insert_annotated_initial_context_before_last_real_user_or_summary(
                compacted_history,
                initial_context,
            )
        }
        InitialContextInjection::AfterSummary { turn_items, .. } => {
            compacted_history.retain(|envelope| {
                matches!(
                    &envelope.item,
                    ResponseItem::Compaction { .. } | ResponseItem::ContextCompaction { .. }
                ) || matches!(
                    crate::event_mapping::parse_turn_item(&envelope.item),
                    Some(TurnItem::UserMessage(user)) if is_summary_message(&user.message())
                ) || envelope.item.turn_id() != Some(turn_context.sub_id.as_str())
            });
            compacted_history.extend(initial_context);
            compacted_history.extend(turn_items.iter().cloned());
            compacted_history
        }
        InitialContextInjection::DoNotInject => compacted_history,
    }
}

pub(crate) fn should_use_remote_compact_task(provider: &ModelProviderInfo) -> bool {
    provider.supports_remote_compaction()
}

pub(crate) async fn run_inline_auto_compact_task(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    initial_context_injection: InitialContextInjection,
    reason: CompactionReason,
    phase: CompactionPhase,
) -> CodexResult<()> {
    let prompt = turn_context
        .config
        .compact_prompt
        .as_deref()
        .unwrap_or(SUMMARIZATION_PROMPT)
        .to_string();
    let input = vec![UserInput::Text {
        text: prompt,
        // Compaction prompt is synthesized; no UI element ranges to preserve.
        text_elements: Vec::new(),
    }];

    run_compact_task_inner(
        sess,
        turn_context,
        input,
        initial_context_injection,
        CompactionTrigger::Auto,
        reason,
        phase,
    )
    .await?;
    Ok(())
}

pub(crate) async fn run_compact_task(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<UserInput>,
) -> CodexResult<()> {
    let start_event = EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: turn_context.sub_id.clone(),
        trace_id: turn_context.trace_id.clone(),
        started_at: turn_context.turn_timing_state.started_at_unix_secs().await,
        model_context_window: turn_context.model_context_window(),
        collaboration_mode_kind: turn_context.collaboration_mode.mode,
    });
    sess.send_event(&turn_context, start_event).await;
    run_compact_task_inner(
        sess.clone(),
        turn_context,
        input,
        InitialContextInjection::DoNotInject,
        CompactionTrigger::Manual,
        CompactionReason::UserRequested,
        CompactionPhase::StandaloneTurn,
    )
    .await?;
    Ok(())
}

async fn run_compact_task_inner(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<UserInput>,
    initial_context_injection: InitialContextInjection,
    trigger: CompactionTrigger,
    reason: CompactionReason,
    phase: CompactionPhase,
) -> CodexResult<()> {
    let compaction_metadata =
        CompactionTurnMetadata::new(trigger, reason, CompactionImplementation::Responses, phase);
    let attempt = CompactionAnalyticsAttempt::begin(
        sess.as_ref(),
        turn_context.as_ref(),
        trigger,
        reason,
        CompactionImplementation::Responses,
        phase,
    )
    .await;
    let pre_compact_outcome = run_pre_compact_hooks(&sess, &turn_context, trigger).await;
    match pre_compact_outcome {
        PreCompactHookOutcome::Continue => {}
        PreCompactHookOutcome::Stopped => {
            let error = CodexErr::TurnAborted;
            attempt
                .track(
                    sess.as_ref(),
                    CompactionStatus::Interrupted,
                    Some(&error),
                    CompactionAnalyticsDetails::default(),
                )
                .await;
            return Err(error);
        }
    }
    let result = run_compact_task_inner_impl(
        Arc::clone(&sess),
        Arc::clone(&turn_context),
        input,
        initial_context_injection,
        compaction_metadata,
    )
    .await;
    let status = compaction_status_from_result(&result);
    let codex_error = result.as_ref().err();
    if result.is_ok() {
        let post_compact_outcome = run_post_compact_hooks(&sess, &turn_context, trigger).await;
        if let PostCompactHookOutcome::Stopped = post_compact_outcome {
            attempt
                .track(
                    sess.as_ref(),
                    status,
                    codex_error,
                    CompactionAnalyticsDetails::default(),
                )
                .await;
            return Err(CodexErr::TurnAborted);
        }
    }
    attempt
        .track(
            sess.as_ref(),
            status,
            codex_error,
            CompactionAnalyticsDetails::default(),
        )
        .await;
    match result {
        Ok(_) => Ok(()),
        Err(err @ CodexErr::TurnAborted) => Err(err),
        Err(err) => {
            sess.track_turn_codex_error(turn_context.as_ref(), &err);
            let event = EventMsg::Error(
                err.to_error_event(Some("Error running local compact task".to_string())),
            );
            sess.send_event(turn_context.as_ref(), event).await;
            Err(err)
        }
    }
}

async fn run_compact_task_inner_impl(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<UserInput>,
    initial_context_injection: InitialContextInjection,
    compaction_metadata: CompactionTurnMetadata,
) -> CodexResult<String> {
    let compaction_item = TurnItem::ContextCompaction(ContextCompactionItem::new());
    sess.emit_turn_item_started(&turn_context, &compaction_item)
        .await;
    let initial_input_for_turn: ResponseInputItem = ResponseInputItem::from(input);

    let mut history = sess.clone_history().await;
    history.record_items(
        &[initial_input_for_turn.into()],
        turn_context.model_info.truncation_policy.into(),
    );

    let max_retries = turn_context.provider.info().stream_max_retries();
    let mut retries = 0;
    let mut client_session = sess.services.model_client.new_session();
    // Reuse one client session so turn-scoped state (sticky routing, websocket incremental
    // request tracking)
    // survives retries within this compact turn.
    let window_id = sess.current_window_id().await;
    let responses_metadata = turn_context.turn_metadata_state.to_responses_metadata(
        sess.installation_id.clone(),
        window_id,
        CodexResponsesRequestKind::Compaction(compaction_metadata),
    );

    loop {
        // Clone is required because of the loop
        let turn_input = history
            .clone()
            .for_prompt(&turn_context.model_info.input_modalities);
        let turn_input_len = turn_input.len();
        let prompt = Prompt {
            input: turn_input,
            base_instructions: sess.get_base_instructions().await,
            ..Default::default()
        };
        let attempt_result = drain_to_completed(
            &sess,
            turn_context.as_ref(),
            &mut client_session,
            &responses_metadata,
            &prompt,
        )
        .await;

        match attempt_result {
            Ok(()) => {
                break;
            }
            Err(err @ (CodexErr::Interrupted | CodexErr::TurnAborted)) => {
                return Err(err);
            }
            Err(e @ CodexErr::SessionBudgetExceeded) => {
                return Err(e);
            }
            Err(e @ CodexErr::ContextWindowExceeded) => {
                if turn_input_len > 1 {
                    // Trim from the beginning to preserve cache (prefix-based) and keep recent messages intact.
                    error!(
                        "Context window exceeded while compacting; removing oldest history item. Error: {e}"
                    );
                    history.remove_first_item();
                    retries = 0;
                    continue;
                }
                sess.set_total_tokens_full(turn_context.as_ref()).await;
                return Err(e);
            }
            Err(e) => {
                if retries < max_retries {
                    retries += 1;
                    let delay = backoff(retries);
                    sess.notify_stream_error(
                        turn_context.as_ref(),
                        format!("Reconnecting... {retries}/{max_retries}"),
                        e,
                    )
                    .await;
                    tokio::time::sleep(delay).await;
                    continue;
                } else {
                    return Err(e);
                }
            }
        }
    }

    let history_snapshot = sess.clone_history().await;
    let history_items = history_snapshot.annotated_items();
    let summary_suffix =
        get_last_assistant_message_from_turn(history_snapshot.raw_items()).unwrap_or_default();
    let summary_text = format!("{SUMMARY_PREFIX}\n{summary_suffix}");
    let user_messages = collect_annotated_user_messages(history_items);

    let mut new_history =
        build_annotated_compacted_history(Vec::new(), &user_messages, &summary_text);
    if let Some(summary_item) = new_history.last_mut() {
        // This replacement history skips `record_conversation_items`; only the appended summary
        // belongs to this compaction turn.
        summary_item.set_turn_id_if_missing(&turn_context.sub_id);
    }
    let (window_number, window_ids) = sess.advance_auto_compact_window().await;

    let (initial_context, world_state_baseline) = build_compaction_initial_context(
        sess.as_ref(),
        turn_context.as_ref(),
        &initial_context_injection,
    )
    .await;
    new_history = place_annotated_compaction_context(
        new_history,
        initial_context,
        turn_context.as_ref(),
        &initial_context_injection,
    );
    let reference_context_item =
        initial_context_injection.reference_context_item(turn_context.as_ref());
    let compacted_item = CompactedItem {
        message: summary_text.clone(),
        replacement_history: None,
        window_number: Some(window_number),
        first_window_id: Some(window_ids.first_window_id.to_string()),
        previous_window_id: window_ids.previous_window_id.map(|id| id.to_string()),
        window_id: Some(window_ids.window_id.to_string()),
    };
    sess.replace_annotated_compacted_history(
        new_history,
        reference_context_item,
        world_state_baseline,
        compacted_item,
    )
    .await;
    sess.recompute_token_usage(&turn_context).await;

    sess.emit_turn_item_completed(&turn_context, compaction_item)
        .await;
    let warning = EventMsg::Warning(WarningEvent {
        message: "Heads up: Long threads and multiple compactions can cause the model to be less accurate. Start a new thread when possible to keep threads small and targeted.".to_string(),
    });
    sess.send_event(&turn_context, warning).await;
    Ok(summary_suffix)
}

pub(crate) struct CompactionAnalyticsAttempt {
    thread_id: String,
    turn_id: String,
    trigger: CompactionTrigger,
    reason: CompactionReason,
    implementation: CompactionImplementation,
    phase: CompactionPhase,
    active_context_tokens_before: i64,
    started_at: u64,
    start_instant: Instant,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CompactionAnalyticsDetails {
    pub(crate) active_context_tokens_before: Option<i64>,
    pub(crate) retained_image_count: Option<usize>,
    pub(crate) compaction_summary_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) cache_write_input_tokens: Option<i64>,
}

impl CompactionAnalyticsAttempt {
    pub(crate) async fn begin(
        sess: &Session,
        turn_context: &TurnContext,
        trigger: CompactionTrigger,
        reason: CompactionReason,
        implementation: CompactionImplementation,
        phase: CompactionPhase,
    ) -> Self {
        let active_context_tokens_before = sess.get_total_token_usage().await;
        Self {
            thread_id: sess.thread_id.to_string(),
            turn_id: turn_context.sub_id.clone(),
            trigger,
            reason,
            implementation,
            phase,
            active_context_tokens_before,
            started_at: now_unix_seconds(),
            start_instant: Instant::now(),
        }
    }

    pub(crate) async fn track(
        self,
        sess: &Session,
        status: CompactionStatus,
        codex_error: Option<&CodexErr>,
        details: CompactionAnalyticsDetails,
    ) {
        let CompactionAnalyticsDetails {
            active_context_tokens_before,
            retained_image_count,
            compaction_summary_tokens,
            cached_input_tokens,
            cache_write_input_tokens,
        } = details;
        let active_context_tokens_before =
            active_context_tokens_before.unwrap_or(self.active_context_tokens_before);
        let active_context_tokens_after = sess.get_total_token_usage().await;
        sess.services
            .analytics_events_client
            .track_compaction(CodexCompactionEvent {
                thread_id: self.thread_id,
                turn_id: self.turn_id,
                trigger: self.trigger,
                reason: self.reason,
                implementation: self.implementation,
                phase: self.phase,
                strategy: CompactionStrategy::Memento,
                status,
                codex_error_kind: codex_error.map(Into::into),
                codex_error_http_status_code: codex_error
                    .and_then(CodexErr::http_status_code_value),
                active_context_tokens_before,
                active_context_tokens_after,
                retained_image_count,
                compaction_summary_tokens,
                cached_input_tokens,
                cache_write_input_tokens,
                started_at: self.started_at,
                completed_at: now_unix_seconds(),
                duration_ms: Some(
                    u64::try_from(self.start_instant.elapsed().as_millis()).unwrap_or(u64::MAX),
                ),
            });
    }
}

pub(crate) fn compaction_status_from_result<T>(result: &CodexResult<T>) -> CompactionStatus {
    match result {
        Ok(_) => CompactionStatus::Completed,
        Err(CodexErr::Interrupted | CodexErr::TurnAborted) => CompactionStatus::Interrupted,
        Err(_) => CompactionStatus::Failed,
    }
}

pub fn content_items_to_text(content: &[ContentItem]) -> Option<String> {
    let mut pieces = Vec::new();
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                if !text.is_empty() {
                    pieces.push(text.as_str());
                }
            }
            ContentItem::InputImage { .. } | ContentItem::InputAudio { .. } => {}
        }
    }
    if pieces.is_empty() {
        None
    } else {
        Some(pieces.join("\n"))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CompactedUserMessage {
    id: Option<codex_protocol::ResponseItemId>,
    message: String,
    internal_chat_message_metadata_passthrough: Option<InternalChatMessageMetadataPassthrough>,
    harness_metadata: Option<CodexHarnessMetadata>,
}

#[cfg(test)]
pub(crate) fn collect_user_messages(items: &[ResponseItem]) -> Vec<CompactedUserMessage> {
    items
        .iter()
        .filter_map(|item| compacted_user_message(item, /*harness_metadata*/ None))
        .collect()
}

pub(crate) fn collect_annotated_user_messages(
    items: &[ResponseItemEnvelope],
) -> Vec<CompactedUserMessage> {
    items
        .iter()
        .filter_map(|envelope| compacted_user_message(&envelope.item, envelope.metadata.clone()))
        .collect()
}

fn compacted_user_message(
    item: &ResponseItem,
    harness_metadata: Option<CodexHarnessMetadata>,
) -> Option<CompactedUserMessage> {
    let Some(TurnItem::UserMessage(user)) = crate::event_mapping::parse_turn_item(item) else {
        return None;
    };
    if is_summary_message(&user.message()) {
        return None;
    }
    Some(CompactedUserMessage {
        id: if crate::context::is_explicit_user_message(item) {
            item.id().cloned()
        } else {
            None
        },
        message: user.message(),
        internal_chat_message_metadata_passthrough: match item {
            ResponseItem::Message {
                internal_chat_message_metadata_passthrough,
                ..
            } => internal_chat_message_metadata_passthrough.clone(),
            _ => None,
        },
        harness_metadata,
    })
}

pub(crate) fn is_summary_message(message: &str) -> bool {
    message.starts_with(format!("{SUMMARY_PREFIX}\n").as_str())
}

/// Inserts canonical initial context into compacted replacement history at the
/// model-expected boundary.
///
/// Placement rules:
/// - Prefer immediately before the last real user message.
/// - If no real user messages remain, insert before the compaction summary so
///   the summary stays last.
/// - If there are no user messages, insert before the last compaction item so
///   that item remains last (remote compaction may return only compaction items).
/// - If there are no user messages or compaction items, append the context.
pub(crate) fn insert_initial_context_before_last_real_user_or_summary(
    compacted_history: Vec<ResponseItem>,
    initial_context: Vec<ResponseItem>,
) -> Vec<ResponseItem> {
    insert_annotated_initial_context_before_last_real_user_or_summary(
        compacted_history
            .into_iter()
            .map(ResponseItemEnvelope::new)
            .collect(),
        initial_context
            .into_iter()
            .map(ResponseItemEnvelope::new)
            .collect(),
    )
    .into_iter()
    .map(ResponseItemEnvelope::into_item)
    .collect()
}

fn insert_annotated_initial_context_before_last_real_user_or_summary(
    mut compacted_history: Vec<ResponseItemEnvelope>,
    initial_context: Vec<ResponseItemEnvelope>,
) -> Vec<ResponseItemEnvelope> {
    let mut last_user_or_summary_index = None;
    let mut last_real_user_index = None;
    for (index, envelope) in compacted_history.iter().enumerate().rev() {
        let Some(TurnItem::UserMessage(user)) =
            crate::event_mapping::parse_turn_item(&envelope.item)
        else {
            continue;
        };
        last_user_or_summary_index.get_or_insert(index);
        if !is_summary_message(&user.message()) {
            last_real_user_index = Some(index);
            break;
        }
    }
    let last_compaction_index = compacted_history
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, envelope)| {
            matches!(
                &envelope.item,
                ResponseItem::Compaction { .. } | ResponseItem::ContextCompaction { .. }
            )
            .then_some(index)
        });
    let insertion_index = last_real_user_index
        .or(last_user_or_summary_index)
        .or(last_compaction_index);

    if let Some(insertion_index) = insertion_index {
        compacted_history.splice(insertion_index..insertion_index, initial_context);
    } else {
        compacted_history.extend(initial_context);
    }
    compacted_history
}

pub(crate) fn build_compacted_history(
    initial_context: Vec<ResponseItem>,
    user_messages: &[CompactedUserMessage],
    summary_text: &str,
) -> Vec<ResponseItem> {
    build_compacted_history_with_limit(
        initial_context,
        user_messages,
        summary_text,
        COMPACT_USER_MESSAGE_MAX_TOKENS,
    )
}

fn build_compacted_history_with_limit(
    history: Vec<ResponseItem>,
    user_messages: &[CompactedUserMessage],
    summary_text: &str,
    max_tokens: usize,
) -> Vec<ResponseItem> {
    build_annotated_compacted_history_with_limit(
        history.into_iter().map(ResponseItemEnvelope::new).collect(),
        user_messages,
        summary_text,
        max_tokens,
    )
    .into_iter()
    .map(ResponseItemEnvelope::into_item)
    .collect()
}

pub(crate) fn build_annotated_compacted_history(
    initial_context: Vec<ResponseItemEnvelope>,
    user_messages: &[CompactedUserMessage],
    summary_text: &str,
) -> Vec<ResponseItemEnvelope> {
    build_annotated_compacted_history_with_limit(
        initial_context,
        user_messages,
        summary_text,
        COMPACT_USER_MESSAGE_MAX_TOKENS,
    )
}

fn build_annotated_compacted_history_with_limit(
    mut history: Vec<ResponseItemEnvelope>,
    user_messages: &[CompactedUserMessage],
    summary_text: &str,
    max_tokens: usize,
) -> Vec<ResponseItemEnvelope> {
    let mut selected_messages: Vec<CompactedUserMessage> = Vec::new();
    if max_tokens > 0 {
        let mut remaining = max_tokens;
        for message in user_messages.iter().rev() {
            if remaining == 0 {
                break;
            }
            let tokens = approx_token_count(&message.message);
            if tokens <= remaining {
                selected_messages.push(message.clone());
                remaining = remaining.saturating_sub(tokens);
            } else {
                let truncated =
                    truncate_text(&message.message, TruncationPolicy::Tokens(remaining));
                selected_messages.push(CompactedUserMessage {
                    id: message.id.clone(),
                    message: truncated,
                    internal_chat_message_metadata_passthrough: message
                        .internal_chat_message_metadata_passthrough
                        .clone(),
                    harness_metadata: message.harness_metadata.clone(),
                });
                break;
            }
        }
        selected_messages.reverse();
    }

    for message in &selected_messages {
        history.push(ResponseItemEnvelope {
            item: ResponseItem::Message {
                id: message.id.clone(),
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: message.message.clone(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: message
                    .internal_chat_message_metadata_passthrough
                    .clone(),
            },
            metadata: message.harness_metadata.clone(),
        });
    }

    let summary_text = if summary_text.is_empty() {
        "(no summary available)".to_string()
    } else {
        summary_text.to_string()
    };

    history.push(ResponseItemEnvelope::new(ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText { text: summary_text }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    }));

    history
}

async fn drain_to_completed(
    sess: &Session,
    turn_context: &TurnContext,
    client_session: &mut ModelClientSession,
    responses_metadata: &CodexResponsesMetadata,
    prompt: &Prompt,
) -> CodexResult<()> {
    let mut stream = client_session
        .stream(
            prompt,
            &turn_context.model_info,
            &turn_context.session_telemetry,
            turn_context.reasoning_effort.clone(),
            turn_context.reasoning_summary,
            turn_context.config.service_tier.clone(),
            responses_metadata,
            // Rollout tracing currently models remote compaction only; local compaction streams
            // are left untraced until the reducer has a first-class local compaction lifecycle.
            &InferenceTraceContext::disabled(),
        )
        .await?;
    loop {
        let maybe_event = stream.next().await;
        let Some(event) = maybe_event else {
            return Err(CodexErr::Stream(
                "stream closed before response.completed".into(),
                None,
            ));
        };
        match event {
            Ok(ResponseEvent::OutputItemDone(item)) => {
                sess.record_conversation_items(turn_context, std::slice::from_ref(&item))
                    .await;
            }
            Ok(ResponseEvent::ServerReasoningIncluded(included)) => {
                sess.set_server_reasoning_included(included).await;
            }
            Ok(ResponseEvent::RateLimits(snapshot)) => {
                sess.update_rate_limits(turn_context, snapshot).await;
            }
            Ok(ResponseEvent::Completed {
                response_id,
                token_usage,
                ..
            }) => {
                sess.send_event(
                    turn_context,
                    EventMsg::RawResponseCompleted(RawResponseCompletedEvent {
                        response_id,
                        token_usage: token_usage.clone(),
                    }),
                )
                .await;
                sess.update_token_usage_info(turn_context, token_usage.as_ref())
                    .await?;
                return Ok(());
            }
            Ok(_) => continue,
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
#[path = "compact_tests.rs"]
mod tests;
