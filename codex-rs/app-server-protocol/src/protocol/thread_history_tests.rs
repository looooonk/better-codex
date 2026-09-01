use super::*;
use crate::protocol::v2::CommandExecutionSource;
use codex_extension_items::ExtensionItem as CoreExtensionItem;
use codex_history::CompactedItem;
use codex_protocol::ThreadId;
use codex_protocol::dynamic_tools::DynamicToolCallOutputContentItem as CoreDynamicToolCallOutputContentItem;
use codex_protocol::items::CommandExecutionItem as CoreCommandExecutionItem;
use codex_protocol::items::CommandExecutionStatus as CoreCommandExecutionStatus;
use codex_protocol::items::EnteredReviewModeItem as CoreEnteredReviewModeItem;
use codex_protocol::items::ExitedReviewModeItem as CoreExitedReviewModeItem;
use codex_protocol::items::HookPromptFragment as CoreHookPromptFragment;
use codex_protocol::items::SleepItem as CoreSleepItem;
use codex_protocol::items::TurnItem as CoreTurnItem;
use codex_protocol::items::UserMessageItem as CoreUserMessageItem;
use codex_protocol::items::build_hook_prompt_message;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::models::ImageDetail;
use codex_protocol::models::MessagePhase as CoreMessagePhase;
use codex_protocol::models::WebSearchAction as CoreWebSearchAction;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::AgentMessageEvent;
use codex_protocol::protocol::AgentReasoningEvent;
use codex_protocol::protocol::AgentReasoningRawContentEvent;
use codex_protocol::protocol::ApplyPatchApprovalRequestEvent;
use codex_protocol::protocol::CodexErrorInfo;
use codex_protocol::protocol::DynamicToolCallResponseEvent;
use codex_protocol::protocol::EnteredReviewModeEvent;
use codex_protocol::protocol::ExecCommandBeginEvent;
use codex_protocol::protocol::ExecCommandEndEvent;
use codex_protocol::protocol::ExecCommandSource;
use codex_protocol::protocol::ExitedReviewModeEvent;
use codex_protocol::protocol::ItemStartedEvent;
use codex_protocol::protocol::McpInvocation;
use codex_protocol::protocol::McpToolCallEndEvent;
use codex_protocol::protocol::PatchApplyBeginEvent;
use codex_protocol::protocol::ReviewTarget;
use codex_protocol::protocol::ThreadRolledBackEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::TurnCompleteEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::UserMessageEvent;
use codex_protocol::protocol::WebSearchBeginEvent;
use codex_protocol::protocol::WebSearchEndEvent;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

#[test]
fn builds_multiple_turns_with_reasoning_items() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "First turn".into(),
            images: Some(vec!["https://example.com/one.png".into()]),
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "Hi there".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::AgentReasoning(AgentReasoningEvent {
            text: "thinking".into(),
        }),
        EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent {
            text: "full reasoning".into(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Second turn".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "Reply two".into(),
            phase: None,
            memory_citation: None,
        }),
    ];

    let mut builder = ThreadHistoryBuilder::new();
    for event in &events {
        builder.handle_event(event);
    }
    let turns = builder.finish();
    assert_eq!(turns.len(), 2);

    let first = &turns[0];
    assert!(Uuid::parse_str(&first.id).is_ok());
    assert_eq!(first.status, TurnStatus::Completed);
    assert_eq!(first.items.len(), 3);
    assert_eq!(
        first.items[0],
        ThreadItem::UserMessage {
            id: "item-1".into(),
            client_id: None,
            content: vec![
                UserInput::Text {
                    text: "First turn".into(),
                    text_elements: Vec::new(),
                },
                UserInput::Image {
                    url: "https://example.com/one.png".into(),
                    detail: None,
                }
            ],
        }
    );
    assert_eq!(
        first.items[1],
        ThreadItem::AgentMessage {
            id: "item-2".into(),
            text: "Hi there".into(),
            phase: None,
            memory_citation: None,
        }
    );
    assert_eq!(
        first.items[2],
        ThreadItem::Reasoning {
            id: "item-3".into(),
            summary: vec!["thinking".into()],
            content: vec!["full reasoning".into()],
        }
    );

    let second = &turns[1];
    assert!(Uuid::parse_str(&second.id).is_ok());
    assert_ne!(first.id, second.id);
    assert_eq!(second.items.len(), 2);
    assert_eq!(
        second.items[0],
        ThreadItem::UserMessage {
            id: "item-4".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "Second turn".into(),
                text_elements: Vec::new(),
            }],
        }
    );
    assert_eq!(
        second.items[1],
        ThreadItem::AgentMessage {
            id: "item-5".into(),
            text: "Reply two".into(),
            phase: None,
            memory_citation: None,
        }
    );
}

#[test]
fn review_mode_events_replay_persisted_ids() {
    let events = vec![
        EventMsg::EnteredReviewMode(EnteredReviewModeEvent {
            target: ReviewTarget::Custom {
                instructions: "review this".into(),
            },
            user_facing_hint: Some("Review requested.".into()),
            turn_id: Some("turn-1".into()),
            item_id: Some("entered-review".into()),
        }),
        EventMsg::ExitedReviewMode(ExitedReviewModeEvent {
            turn_id: Some("turn-1".into()),
            item_id: Some("exited-review".into()),
            review_output: None,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let mut builder = ThreadHistoryBuilder::new();
    for event in &events {
        builder.handle_event(event);
    }
    let turns = builder.finish();

    assert_eq!(turns[0].id, "turn-1");
    assert_eq!(
        turns[0].items,
        vec![
            ThreadItem::EnteredReviewMode {
                id: "entered-review".into(),
                review: "Review requested.".into(),
            },
            ThreadItem::ExitedReviewMode {
                id: "exited-review".into(),
                review: REVIEW_FALLBACK_MESSAGE.into(),
            },
        ]
    );
}

#[test]
fn review_mode_items_replay_without_turn_started() {
    let thread_id = ThreadId::new();
    let entered = CoreTurnItem::EnteredReviewMode(CoreEnteredReviewModeItem {
        id: "entered-review".into(),
        target: ReviewTarget::Custom {
            instructions: "review this".into(),
        },
        user_facing_hint: "Review requested.".into(),
    });
    let exited = CoreTurnItem::ExitedReviewMode(CoreExitedReviewModeItem {
        id: "exited-review".into(),
        review_output: None,
    });
    let events = vec![
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: "turn-1".into(),
            item: entered,
            completed_at_ms: 0,
        }),
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: "turn-1".into(),
            item: exited,
            completed_at_ms: 0,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let mut builder = ThreadHistoryBuilder::new();
    for event in &events {
        builder.handle_event(event);
    }
    let turns = builder.finish();

    assert_eq!(turns[0].id, "turn-1");
    assert_eq!(
        turns[0].items,
        vec![
            ThreadItem::EnteredReviewMode {
                id: "entered-review".into(),
                review: "Review requested.".into(),
            },
            ThreadItem::ExitedReviewMode {
                id: "exited-review".into(),
                review: REVIEW_FALLBACK_MESSAGE.into(),
            },
        ]
    );
}

#[test]
fn rebuilds_user_message_image_details_from_legacy_events() {
    let local_path = PathBuf::from("/tmp/local.png");
    let events = vec![RolloutItem::EventMsg(EventMsg::UserMessage(
        UserMessageEvent {
            client_id: None,
            message: "inspect these".into(),
            images: Some(vec!["https://example.com/image.png".into()]),
            image_details: vec![Some(ImageDetail::Original)],
            local_images: vec![local_path.clone()],
            local_image_details: vec![Some(ImageDetail::Original)],
            text_elements: Vec::new(),
        },
    ))];

    let turns = build_turns_from_rollout_items(&events);

    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items[0],
        ThreadItem::UserMessage {
            id: "item-1".into(),
            client_id: None,
            content: vec![
                UserInput::Text {
                    text: "inspect these".into(),
                    text_elements: Vec::new(),
                },
                UserInput::Image {
                    url: "https://example.com/image.png".into(),
                    detail: Some(ImageDetail::Original),
                },
                UserInput::LocalImage {
                    path: local_path,
                    detail: Some(ImageDetail::Original),
                },
            ],
        }
    );
}

#[test]
fn ignores_user_message_item_lifecycle_events() {
    let turn_id = "turn-1";
    let thread_id = ThreadId::new();
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::ItemStarted(ItemStartedEvent {
            thread_id,
            turn_id: turn_id.to_string(),
            item: CoreTurnItem::UserMessage(CoreUserMessageItem {
                id: "user-item-id".to_string(),
                client_id: None,
                content: Vec::new(),
            }),
            started_at_ms: 0,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: turn_id.to_string(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 1);
    assert_eq!(
        turns[0].items[0],
        ThreadItem::UserMessage {
            id: "item-1".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }],
        }
    );
}

#[test]
fn rebuilds_sleep_item_from_persisted_completion() {
    let turn_id = "turn-1";
    let thread_id = ThreadId::new();
    let sleep_item = CoreTurnItem::Sleep(CoreSleepItem {
        id: "sleep-1".to_string(),
        duration_ms: 1_000,
    });
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: turn_id.to_string(),
            item: sleep_item,
            completed_at_ms: 1_000,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: turn_id.to_string(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items,
        vec![ThreadItem::Sleep {
            id: "sleep-1".to_string(),
            duration_ms: 1_000,
        }]
    );
}

#[test]
fn rebuilds_extension_image_generation_item_from_persisted_completion() {
    let turn_id = "turn-1";
    let thread_id = ThreadId::new();
    let saved_path = test_path_buf("/tmp/image-1.png").abs();
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: turn_id.to_string(),
            item: CoreTurnItem::Extension(CoreExtensionItem::ImageGeneration(
                ImageGenerationItem {
                    id: "image-1".to_string(),
                    status: "completed".to_string(),
                    revised_prompt: Some("A blue square".to_string()),
                    result: "cG5n".to_string(),
                    saved_path: Some(saved_path.clone()),
                },
            )),
            completed_at_ms: 1_000,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: turn_id.to_string(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];
    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(
        turns[0].items,
        vec![ThreadItem::ImageGeneration(ImageGenerationItem {
            id: "image-1".to_string(),
            status: "completed".to_string(),
            revised_prompt: Some("A blue square".to_string()),
            result: "cG5n".to_string(),
            saved_path: Some(saved_path),
        })]
    );
}

#[test]
fn redacts_command_secrets_across_live_and_replayed_upserts() {
    let turn_id = "turn-1";
    let thread_id = ThreadId::new();
    let command = vec![
        "git".to_string(),
        "-c".to_string(),
        "http.extraHeader=Authorization: Bearer example_synthetic_bearer_token_123456".to_string(),
        "push".to_string(),
    ];
    let parsed_cmd = vec![ParsedCommand::Unknown {
        cmd: "git -c 'http.extraHeader=Authorization: Bearer example_synthetic_bearer_token_123456' push"
            .to_string(),
    }];
    let command_item = CoreTurnItem::CommandExecution(CoreCommandExecutionItem {
        id: "exec-1".to_string(),
        process_id: Some("pid-1".to_string()),
        command: command.clone(),
        cwd: test_path_buf("/tmp").abs().into(),
        parsed_cmd: parsed_cmd.clone(),
        source: ExecCommandSource::Agent,
        interaction_input: None,
        status: CoreCommandExecutionStatus::Completed,
        stdout: Some("hello world\n".to_string()),
        stderr: Some(String::new()),
        aggregated_output: Some("hello world\n".to_string()),
        exit_code: Some(0),
        duration: Some(Duration::from_millis(12)),
        formatted_output: Some("hello world\n".to_string()),
    });
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::ExecCommandBegin(ExecCommandBeginEvent {
            call_id: "exec-1".to_string(),
            process_id: Some("pid-1".to_string()),
            turn_id: turn_id.to_string(),
            started_at_ms: 0,
            command: command.clone(),
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd: parsed_cmd.clone(),
            source: ExecCommandSource::Agent,
            interaction_input: None,
        }),
        EventMsg::ItemCompleted(ItemCompletedEvent {
            thread_id,
            turn_id: turn_id.to_string(),
            item: command_item,
            completed_at_ms: 1_000,
        }),
        EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "exec-1".to_string(),
            process_id: Some("pid-1".to_string()),
            turn_id: turn_id.to_string(),
            completed_at_ms: 1_000,
            command,
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd,
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: "hello world\n".to_string(),
            stderr: String::new(),
            aggregated_output: "hello world\n".to_string(),
            exit_code: 0,
            duration: Duration::from_millis(12),
            formatted_output: "hello world\n".to_string(),
            status: CoreExecCommandStatus::Completed,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: turn_id.to_string(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();

    assert_eq!(
        build_turns_from_rollout_items(&items[..2])[0].items,
        vec![ThreadItem::CommandExecution {
            id: "exec-1".to_string(),
            command: "git -c 'http.extraHeader=Authorization: Bearer [REDACTED_SECRET]' push"
                .to_string(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: Some("pid-1".to_string()),
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::InProgress,
            command_actions: vec![CommandAction::Unknown {
                command: "git -c 'http.extraHeader=Authorization: Bearer [REDACTED_SECRET]' push"
                    .to_string(),
            }],
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        }]
    );
    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items,
        vec![ThreadItem::CommandExecution {
            id: "exec-1".to_string(),
            command: "git -c 'http.extraHeader=Authorization: Bearer [REDACTED_SECRET]' push"
                .to_string(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: Some("pid-1".to_string()),
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::Completed,
            command_actions: vec![CommandAction::Unknown {
                command: "git -c 'http.extraHeader=Authorization: Bearer [REDACTED_SECRET]' push"
                    .to_string(),
            }],
            aggregated_output: Some("hello world\n".to_string()),
            exit_code: Some(0),
            duration_ms: Some(12),
        }]
    );
}

#[test]
fn preserves_user_message_client_id_from_legacy_event() {
    let turn_id = "turn-1";
    let thread_id = ThreadId::new();
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::ItemStarted(ItemStartedEvent {
            thread_id,
            turn_id: turn_id.to_string(),
            item: CoreTurnItem::UserMessage(CoreUserMessageItem {
                id: "user-item-id".to_string(),
                client_id: Some("client-message-1".to_string()),
                content: vec![codex_protocol::user_input::UserInput::Text {
                    text: "hello".into(),
                    text_elements: Vec::new(),
                }],
            }),
            started_at_ms: 0,
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: Some("client-message-1".to_string()),
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: turn_id.to_string(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items,
        vec![ThreadItem::UserMessage {
            id: "item-1".into(),
            client_id: Some("client-message-1".to_string()),
            content: vec![UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }],
        }]
    );
}

#[test]
fn preserves_agent_message_phase_in_history() {
    let events = vec![EventMsg::AgentMessage(AgentMessageEvent {
        message: "Final reply".into(),
        phase: Some(CoreMessagePhase::FinalAnswer),
        memory_citation: None,
    })];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items[0],
        ThreadItem::AgentMessage {
            id: "item-1".into(),
            text: "Final reply".into(),
            phase: Some(MessagePhase::FinalAnswer),
            memory_citation: None,
        }
    );
}

#[test]
fn replays_image_generation_end_events_into_turn_history() {
    let items = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-image".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "generate an image".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        })),
        RolloutItem::EventMsg(EventMsg::ImageGenerationEnd(ImageGenerationEndEvent {
            call_id: "ig_123".into(),
            status: "completed".into(),
            revised_prompt: Some("final prompt".into()),
            result: "Zm9v".into(),
            saved_path: Some(test_path_buf("/tmp/ig_123.png").abs()),
        })),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-image".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    ];

    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0],
        Turn {
            id: "turn-image".into(),
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            items_view: TurnItemsView::Full,
            items: vec![
                ThreadItem::UserMessage {
                    id: "item-1".into(),
                    client_id: None,
                    content: vec![UserInput::Text {
                        text: "generate an image".into(),
                        text_elements: Vec::new(),
                    }],
                },
                ThreadItem::ImageGeneration(ImageGenerationItem {
                    id: "ig_123".into(),
                    status: "completed".into(),
                    revised_prompt: Some("final prompt".into()),
                    result: "Zm9v".into(),
                    saved_path: Some(test_path_buf("/tmp/ig_123.png").abs()),
                }),
            ],
        }
    );
}

#[test]
fn splits_reasoning_when_interleaved() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Turn start".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentReasoning(AgentReasoningEvent {
            text: "first summary".into(),
        }),
        EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent {
            text: "first content".into(),
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "interlude".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::AgentReasoning(AgentReasoningEvent {
            text: "second summary".into(),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    let turn = &turns[0];
    assert_eq!(turn.items.len(), 4);

    assert_eq!(
        turn.items[1],
        ThreadItem::Reasoning {
            id: "item-2".into(),
            summary: vec!["first summary".into()],
            content: vec!["first content".into()],
        }
    );
    assert_eq!(
        turn.items[3],
        ThreadItem::Reasoning {
            id: "item-4".into(),
            summary: vec!["second summary".into()],
            content: Vec::new(),
        }
    );
}

#[test]
fn marks_turn_as_interrupted_when_aborted() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Please do the thing".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "Working...".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some("turn-1".into()),
            started_at: None,
            reason: TurnAbortReason::Replaced,
            completed_at: None,
            duration_ms: None,
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Let's try again".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "Second attempt complete.".into(),
            phase: None,
            memory_citation: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 2);

    let first_turn = &turns[0];
    assert_eq!(first_turn.status, TurnStatus::Interrupted);
    assert_eq!(first_turn.items.len(), 2);
    assert_eq!(
        first_turn.items[0],
        ThreadItem::UserMessage {
            id: "item-1".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "Please do the thing".into(),
                text_elements: Vec::new(),
            }],
        }
    );
    assert_eq!(
        first_turn.items[1],
        ThreadItem::AgentMessage {
            id: "item-2".into(),
            text: "Working...".into(),
            phase: None,
            memory_citation: None,
        }
    );

    let second_turn = &turns[1];
    assert_eq!(second_turn.status, TurnStatus::Completed);
    assert_eq!(second_turn.items.len(), 2);
    assert_eq!(
        second_turn.items[0],
        ThreadItem::UserMessage {
            id: "item-3".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "Let's try again".into(),
                text_elements: Vec::new(),
            }],
        }
    );
    assert_eq!(
        second_turn.items[1],
        ThreadItem::AgentMessage {
            id: "item-4".into(),
            text: "Second attempt complete.".into(),
            phase: None,
            memory_citation: None,
        }
    );
}

#[test]
fn drops_last_turns_on_thread_rollback() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "First".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "A1".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Second".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "A2".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::ThreadRolledBack(ThreadRolledBackEvent { num_turns: 1 }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Third".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "A3".into(),
            phase: None,
            memory_citation: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "rollout-0");
    assert_eq!(turns[1].id, "rollout-5");
    assert_ne!(turns[0].id, turns[1].id);
    assert_eq!(turns[0].status, TurnStatus::Completed);
    assert_eq!(turns[1].status, TurnStatus::Completed);
    assert_eq!(
        turns[0].items,
        vec![
            ThreadItem::UserMessage {
                id: "item-1".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "First".into(),
                    text_elements: Vec::new(),
                }],
            },
            ThreadItem::AgentMessage {
                id: "item-2".into(),
                text: "A1".into(),
                phase: None,
                memory_citation: None,
            },
        ]
    );
    assert_eq!(
        turns[1].items,
        vec![
            ThreadItem::UserMessage {
                id: "item-3".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "Third".into(),
                    text_elements: Vec::new(),
                }],
            },
            ThreadItem::AgentMessage {
                id: "item-4".into(),
                text: "A3".into(),
                phase: None,
                memory_citation: None,
            },
        ]
    );
}

#[test]
fn thread_rollback_clears_all_turns_when_num_turns_exceeds_history() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "One".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "A1".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Two".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "A2".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::ThreadRolledBack(ThreadRolledBackEvent { num_turns: 99 }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns, Vec::<Turn>::new());
}

#[test]
fn uses_explicit_turn_boundaries_for_mid_turn_steering() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Start".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "Steer".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(
        turns[0].items,
        vec![
            ThreadItem::UserMessage {
                id: "item-1".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "Start".into(),
                    text_elements: Vec::new(),
                }],
            },
            ThreadItem::UserMessage {
                id: "item-2".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "Steer".into(),
                    text_elements: Vec::new(),
                }],
            },
        ]
    );
}

#[test]
fn reconstructs_tool_items_from_persisted_completion_events() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "run tools".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::WebSearchEnd(WebSearchEndEvent {
            call_id: "search-1".into(),
            query: "codex".into(),
            action: CoreWebSearchAction::Search {
                query: Some("codex".into()),
                queries: None,
            },
            results: Some(vec![serde_json::json!({
                "type": "text_result",
                "ref_id": "turn0search0",
                "url": "https://example.com/codex",
            })]),
        }),
        EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "exec-1".into(),
            process_id: Some("pid-1".into()),
            turn_id: "turn-1".into(),
            completed_at_ms: 0,
            command: vec!["echo".into(), "hello world".into()],
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd: vec![ParsedCommand::Unknown {
                cmd: "echo hello world".into(),
            }],
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: String::new(),
            stderr: String::new(),
            aggregated_output: "hello world\n".into(),
            exit_code: 0,
            duration: Duration::from_millis(12),
            formatted_output: String::new(),
            status: CoreExecCommandStatus::Completed,
        }),
        EventMsg::McpToolCallEnd(McpToolCallEndEvent {
            call_id: "mcp-1".into(),
            invocation: McpInvocation {
                server: "docs".into(),
                tool: "lookup".into(),
                arguments: Some(serde_json::json!({"id":"123"})),
            },
            connector_id: None,
            mcp_app_resource_uri: None,
            link_id: None,
            app_name: None,
            template_id: None,
            action_name: None,
            plugin_id: None,
            duration: Duration::from_millis(8),
            result: Err("boom".into()),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 4);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::WebSearch(WebSearchItem {
            id: "search-1".into(),
            query: "codex".into(),
            action: Some(WebSearchAction::Search {
                query: Some("codex".into()),
                queries: None,
            }),
            results: Some(vec![serde_json::json!({
                "type": "text_result",
                "ref_id": "turn0search0",
                "url": "https://example.com/codex",
            })]),
        })
    );
    assert_eq!(
        turns[0].items[2],
        ThreadItem::CommandExecution {
            id: "exec-1".into(),
            command: "echo 'hello world'".into(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: Some("pid-1".into()),
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::Completed,
            command_actions: vec![CommandAction::Unknown {
                command: "echo hello world".into(),
            }],
            aggregated_output: Some("hello world\n".into()),
            exit_code: Some(0),
            duration_ms: Some(12),
        }
    );
    assert_eq!(
        turns[0].items[3],
        ThreadItem::McpToolCall {
            id: "mcp-1".into(),
            server: "docs".into(),
            tool: "lookup".into(),
            status: McpToolCallStatus::Failed,
            arguments: serde_json::json!({"id":"123"}),
            app_context: None,
            mcp_app_resource_uri: None,
            plugin_id: None,
            result: None,
            error: Some(McpToolCallError {
                message: "boom".into(),
            }),
            duration_ms: Some(8),
        }
    );
}

#[test]
fn reconstructs_mcp_tool_result_meta_from_persisted_completion_events() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::McpToolCallEnd(McpToolCallEndEvent {
            call_id: "mcp-1".into(),
            invocation: McpInvocation {
                server: "docs".into(),
                tool: "lookup".into(),
                arguments: Some(serde_json::json!({"id":"123"})),
            },
            connector_id: Some("calendar".into()),
            mcp_app_resource_uri: Some("ui://widget/lookup.html".into()),
            link_id: Some("link_calendar".into()),
            app_name: Some("Calendar".into()),
            template_id: Some("calendar_template".into()),
            action_name: Some("lookup".into()),
            plugin_id: Some("sample@test".into()),
            duration: Duration::from_millis(8),
            result: Ok(CallToolResult {
                content: vec![serde_json::json!({
                    "type": "text",
                    "text": "result"
                })],
                structured_content: Some(serde_json::json!({"id":"123"})),
                is_error: Some(false),
                meta: Some(serde_json::json!({
                    "ui/resourceUri": "ui://widget/lookup.html"
                })),
            }),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0].items[0],
        ThreadItem::McpToolCall {
            id: "mcp-1".into(),
            server: "docs".into(),
            tool: "lookup".into(),
            status: McpToolCallStatus::Completed,
            arguments: serde_json::json!({"id":"123"}),
            app_context: Some(McpToolCallAppContext {
                connector_id: "calendar".into(),
                link_id: Some("link_calendar".into()),
                resource_uri: Some("ui://widget/lookup.html".into()),
                app_name: Some("Calendar".into()),
                template_id: Some("calendar_template".into()),
                action_name: Some("lookup".into()),
            }),
            mcp_app_resource_uri: Some("ui://widget/lookup.html".into()),
            plugin_id: Some("sample@test".into()),
            result: Some(Box::new(McpToolCallResult {
                content: vec![serde_json::json!({
                    "type": "text",
                    "text": "result"
                })],
                structured_content: Some(serde_json::json!({"id":"123"})),
                meta: Some(serde_json::json!({
                    "ui/resourceUri": "ui://widget/lookup.html"
                })),
            })),
            error: None,
            duration_ms: Some(8),
        }
    );
}

#[test]
fn reconstructs_dynamic_tool_items_from_request_and_response_events() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "run dynamic tool".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::DynamicToolCallRequest(codex_protocol::dynamic_tools::DynamicToolCallRequest {
            call_id: "dyn-1".into(),
            turn_id: "turn-1".into(),
            started_at_ms: 0,
            namespace: Some("codex_app".into()),
            tool: "lookup_ticket".into(),
            arguments: serde_json::json!({"id":"ABC-123"}),
        }),
        EventMsg::DynamicToolCallResponse(DynamicToolCallResponseEvent {
            call_id: "dyn-1".into(),
            turn_id: "turn-1".into(),
            completed_at_ms: 0,
            namespace: Some("codex_app".into()),
            tool: "lookup_ticket".into(),
            arguments: serde_json::json!({"id":"ABC-123"}),
            content_items: vec![CoreDynamicToolCallOutputContentItem::InputText {
                text: "Ticket is open".into(),
            }],
            success: true,
            error: None,
            duration: Duration::from_millis(42),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::DynamicToolCall {
            id: "dyn-1".into(),
            namespace: Some("codex_app".into()),
            tool: "lookup_ticket".into(),
            arguments: serde_json::json!({"id":"ABC-123"}),
            status: DynamicToolCallStatus::Completed,
            content_items: Some(vec![DynamicToolCallOutputContentItem::InputText {
                text: "Ticket is open".into(),
            }]),
            success: Some(true),
            duration_ms: Some(42),
        }
    );
}

#[test]
fn reconstructs_declined_exec_and_patch_items() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "run tools".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "exec-declined".into(),
            process_id: Some("pid-2".into()),
            turn_id: "turn-1".into(),
            completed_at_ms: 0,
            command: vec!["ls".into()],
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd: vec![ParsedCommand::Unknown { cmd: "ls".into() }],
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: String::new(),
            stderr: "exec command rejected by user".into(),
            aggregated_output: "exec command rejected by user".into(),
            exit_code: -1,
            duration: Duration::ZERO,
            formatted_output: String::new(),
            status: CoreExecCommandStatus::Declined,
        }),
        EventMsg::PatchApplyEnd(PatchApplyEndEvent {
            call_id: "patch-declined".into(),
            turn_id: "turn-1".into(),
            stdout: String::new(),
            stderr: "patch rejected by user".into(),
            success: false,
            changes: [(
                PathBuf::from("README.md"),
                codex_protocol::protocol::FileChange::Add {
                    content: "hello\n".into(),
                },
            )]
            .into_iter()
            .collect(),
            status: CorePatchApplyStatus::Declined,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 3);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CommandExecution {
            id: "exec-declined".into(),
            command: "ls".into(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: Some("pid-2".into()),
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::Declined,
            command_actions: vec![CommandAction::Unknown {
                command: "ls".into(),
            }],
            aggregated_output: Some("exec command rejected by user".into()),
            exit_code: Some(-1),
            duration_ms: Some(0),
        }
    );
    assert_eq!(
        turns[0].items[2],
        ThreadItem::FileChange {
            id: "patch-declined".into(),
            changes: vec![FileUpdateChange {
                path: "README.md".into(),
                kind: PatchChangeKind::Add,
                diff: "hello\n".into(),
            }],
            status: PatchApplyStatus::Declined,
        }
    );
}

#[test]
fn reconstructs_declined_guardian_command_item() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "review this command".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::GuardianAssessment(GuardianAssessmentEvent {
            id: "review-guardian-exec".into(),
            target_item_id: Some("guardian-exec".into()),
            turn_id: "turn-1".into(),
            started_at_ms: 1_000,
            completed_at_ms: None,
            status: GuardianAssessmentStatus::InProgress,
            risk_level: None,
            user_authorization: None,
            rationale: None,
            decision_source: None,
            action: serde_json::from_value(serde_json::json!({
                "type": "command",
                "source": "shell",
                "command": "rm -rf /tmp/guardian",
                "cwd": test_path_buf("/tmp"),
            }))
            .expect("guardian action"),
        }),
        EventMsg::GuardianAssessment(GuardianAssessmentEvent {
            id: "review-guardian-exec".into(),
            target_item_id: Some("guardian-exec".into()),
            turn_id: "turn-1".into(),
            started_at_ms: 1_000,
            completed_at_ms: Some(1_042),
            status: GuardianAssessmentStatus::Denied,
            risk_level: Some(codex_protocol::protocol::GuardianRiskLevel::High),
            user_authorization: Some(codex_protocol::protocol::GuardianUserAuthorization::Low),
            rationale: Some("Would delete user data.".into()),
            decision_source: Some(
                codex_protocol::protocol::GuardianAssessmentDecisionSource::Agent,
            ),
            action: serde_json::from_value(serde_json::json!({
                "type": "command",
                "source": "shell",
                "command": "rm -rf /tmp/guardian",
                "cwd": test_path_buf("/tmp"),
            }))
            .expect("guardian action"),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CommandExecution {
            id: "guardian-exec".into(),
            command: "rm -rf /tmp/guardian".into(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: None,
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::Declined,
            command_actions: vec![CommandAction::Unknown {
                command: "rm -rf /tmp/guardian".into(),
            }],
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        }
    );
}

#[test]
fn reconstructs_in_progress_guardian_execve_item() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "run a subcommand".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::GuardianAssessment(GuardianAssessmentEvent {
            id: "review-guardian-execve".into(),
            target_item_id: Some("guardian-execve".into()),
            turn_id: "turn-1".into(),
            started_at_ms: 2_000,
            completed_at_ms: None,
            status: GuardianAssessmentStatus::InProgress,
            risk_level: None,
            user_authorization: None,
            rationale: None,
            decision_source: None,
            action: serde_json::from_value(serde_json::json!({
                "type": "execve",
                "source": "shell",
                "program": "/bin/rm",
                "argv": ["/usr/bin/rm", "-f", "/tmp/file.sqlite"],
                "cwd": test_path_buf("/tmp"),
            }))
            .expect("guardian action"),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CommandExecution {
            id: "guardian-execve".into(),
            command: "/bin/rm -f /tmp/file.sqlite".into(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: None,
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::InProgress,
            command_actions: vec![CommandAction::Unknown {
                command: "/bin/rm -f /tmp/file.sqlite".into(),
            }],
            aggregated_output: None,
            exit_code: None,
            duration_ms: None,
        }
    );
}

#[test]
fn assigns_late_exec_completion_to_original_turn() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "first".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-b".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "second".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "exec-late".into(),
            process_id: Some("pid-42".into()),
            turn_id: "turn-a".into(),
            completed_at_ms: 0,
            command: vec!["echo".into(), "done".into()],
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd: vec![ParsedCommand::Unknown {
                cmd: "echo done".into(),
            }],
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: "done\n".into(),
            stderr: String::new(),
            aggregated_output: "done\n".into(),
            exit_code: 0,
            duration: Duration::from_millis(5),
            formatted_output: "done\n".into(),
            status: CoreExecCommandStatus::Completed,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-b".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(turns[1].id, "turn-b");
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(turns[1].items.len(), 1);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CommandExecution {
            id: "exec-late".into(),
            command: "echo done".into(),
            cwd: test_path_buf("/tmp").abs().into(),
            process_id: Some("pid-42".into()),
            source: CommandExecutionSource::Agent,
            status: CommandExecutionStatus::Completed,
            command_actions: vec![CommandAction::Unknown {
                command: "echo done".into(),
            }],
            aggregated_output: Some("done\n".into()),
            exit_code: Some(0),
            duration_ms: Some(5),
        }
    );
}

#[test]
fn drops_late_turn_scoped_item_for_unknown_turn_id() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "first".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-b".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "second".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id: "exec-unknown-turn".into(),
            process_id: Some("pid-42".into()),
            turn_id: "turn-missing".into(),
            completed_at_ms: 0,
            command: vec!["echo".into(), "done".into()],
            cwd: test_path_buf("/tmp").abs().into(),
            parsed_cmd: vec![ParsedCommand::Unknown {
                cmd: "echo done".into(),
            }],
            source: ExecCommandSource::Agent,
            interaction_input: None,
            stdout: "done\n".into(),
            stderr: String::new(),
            aggregated_output: "done\n".into(),
            exit_code: 0,
            duration: Duration::from_millis(5),
            formatted_output: "done\n".into(),
            status: CoreExecCommandStatus::Completed,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-b".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let mut builder = ThreadHistoryBuilder::new();
    for event in &events {
        builder.handle_event(event);
    }
    let turns = builder.finish();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(turns[1].id, "turn-b");
    assert_eq!(turns[0].items.len(), 1);
    assert_eq!(turns[1].items.len(), 1);
    assert_eq!(
        turns[1].items[0],
        ThreadItem::UserMessage {
            id: "item-2".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "second".into(),
                text_elements: Vec::new(),
            }],
        }
    );
}

#[test]
fn patch_apply_begin_updates_active_turn_snapshot_with_file_change() {
    let turn_id = "turn-1";
    let mut builder = ThreadHistoryBuilder::new();
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "apply patch".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
            call_id: "patch-call".into(),
            turn_id: turn_id.to_string(),
            auto_approved: false,
            changes: [(
                PathBuf::from("README.md"),
                codex_protocol::protocol::FileChange::Add {
                    content: "hello\n".into(),
                },
            )]
            .into_iter()
            .collect(),
        }),
    ];

    for event in &events {
        builder.handle_event(event);
    }

    let snapshot = builder
        .active_turn_snapshot()
        .expect("active turn snapshot");
    assert_eq!(snapshot.id, turn_id);
    assert_eq!(snapshot.status, TurnStatus::InProgress);
    assert_eq!(
        snapshot.items,
        vec![
            ThreadItem::UserMessage {
                id: "item-1".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "apply patch".into(),
                    text_elements: Vec::new(),
                }],
            },
            ThreadItem::FileChange {
                id: "patch-call".into(),
                changes: vec![FileUpdateChange {
                    path: "README.md".into(),
                    kind: PatchChangeKind::Add,
                    diff: "hello\n".into(),
                }],
                status: PatchApplyStatus::InProgress,
            },
        ]
    );
}

#[test]
fn apply_patch_approval_request_updates_active_turn_snapshot_with_file_change() {
    let turn_id = "turn-1";
    let mut builder = ThreadHistoryBuilder::new();
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: turn_id.to_string(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "apply patch".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
            call_id: "patch-call".into(),
            turn_id: turn_id.to_string(),
            started_at_ms: 0,
            changes: [(
                PathBuf::from("README.md"),
                codex_protocol::protocol::FileChange::Add {
                    content: "hello\n".into(),
                },
            )]
            .into_iter()
            .collect(),
            reason: None,
            grant_root: None,
        }),
    ];

    for event in &events {
        builder.handle_event(event);
    }

    let snapshot = builder
        .active_turn_snapshot()
        .expect("active turn snapshot");
    assert_eq!(snapshot.id, turn_id);
    assert_eq!(snapshot.status, TurnStatus::InProgress);
    assert_eq!(
        snapshot.items,
        vec![
            ThreadItem::UserMessage {
                id: "item-1".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "apply patch".into(),
                    text_elements: Vec::new(),
                }],
            },
            ThreadItem::FileChange {
                id: "patch-call".into(),
                changes: vec![FileUpdateChange {
                    path: "README.md".into(),
                    kind: PatchChangeKind::Add,
                    diff: "hello\n".into(),
                }],
                status: PatchApplyStatus::InProgress,
            },
        ]
    );
}

#[test]
fn late_turn_complete_does_not_close_active_turn() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "first".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-b".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "second".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "still in b".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-b".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(turns[1].id, "turn-b");
    assert_eq!(turns[1].items.len(), 2);
}

#[test]
fn late_turn_aborted_does_not_interrupt_active_turn() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "first".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-b".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "second".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some("turn-a".into()),
            started_at: None,
            reason: TurnAbortReason::Replaced,
            completed_at: None,
            duration_ms: None,
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "still in b".into(),
            phase: None,
            memory_citation: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(turns[1].id, "turn-b");
    assert_eq!(turns[1].status, TurnStatus::InProgress);
    assert_eq!(turns[1].items.len(), 2);
}

#[test]
fn preserves_compaction_only_turn() {
    let items = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-compact".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::Compacted(CompactedItem {
            message: String::new(),
            replacement_history: None,
            window_number: None,
            first_window_id: None,
            previous_window_id: None,
            window_id: None,
        }),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-compact".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    ];

    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(
        turns,
        vec![Turn {
            id: "turn-compact".into(),
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            items_view: TurnItemsView::Full,
            items: Vec::new(),
        }]
    );
}

#[test]
fn reconstructs_collab_resume_end_item() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "resume agent".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::CollabResumeEnd(codex_protocol::protocol::CollabResumeEndEvent {
            call_id: "resume-1".into(),
            completed_at_ms: 0,
            sender_thread_id: ThreadId::try_from("00000000-0000-0000-0000-000000000001")
                .expect("valid sender thread id"),
            receiver_thread_id: ThreadId::try_from("00000000-0000-0000-0000-000000000002")
                .expect("valid receiver thread id"),
            receiver_agent_nickname: None,
            receiver_agent_role: None,
            status: AgentStatus::Completed(None),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CollabAgentToolCall {
            id: "resume-1".into(),
            tool: CollabAgentTool::ResumeAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: "00000000-0000-0000-0000-000000000001".into(),
            receiver_thread_ids: vec!["00000000-0000-0000-0000-000000000002".into()],
            prompt: None,
            model: None,
            reasoning_effort: None,
            agents_states: [(
                "00000000-0000-0000-0000-000000000002".into(),
                CollabAgentState {
                    status: crate::protocol::v2::CollabAgentStatus::Completed,
                    message: None,
                },
            )]
            .into_iter()
            .collect(),
        }
    );
}

#[test]
fn reconstructs_collab_spawn_end_item_with_model_metadata() {
    let sender_thread_id =
        ThreadId::try_from("00000000-0000-0000-0000-000000000001").expect("valid sender thread id");
    let spawned_thread_id = ThreadId::try_from("00000000-0000-0000-0000-000000000002")
        .expect("valid receiver thread id");
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "spawn agent".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::CollabAgentSpawnEnd(codex_protocol::protocol::CollabAgentSpawnEndEvent {
            call_id: "spawn-1".into(),
            completed_at_ms: 0,
            sender_thread_id,
            new_thread_id: Some(spawned_thread_id),
            new_agent_nickname: Some("Scout".into()),
            new_agent_role: Some("explorer".into()),
            prompt: "inspect the repo".into(),
            model: "gpt-5.4-mini".into(),
            reasoning_effort: codex_protocol::openai_models::ReasoningEffort::Medium,
            status: AgentStatus::Running,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CollabAgentToolCall {
            id: "spawn-1".into(),
            tool: CollabAgentTool::SpawnAgent,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: "00000000-0000-0000-0000-000000000001".into(),
            receiver_thread_ids: vec!["00000000-0000-0000-0000-000000000002".into()],
            prompt: Some("inspect the repo".into()),
            model: Some("gpt-5.4-mini".into()),
            reasoning_effort: Some(codex_protocol::openai_models::ReasoningEffort::Medium),
            agents_states: [(
                "00000000-0000-0000-0000-000000000002".into(),
                CollabAgentState {
                    status: crate::protocol::v2::CollabAgentStatus::Running,
                    message: None,
                },
            )]
            .into_iter()
            .collect(),
        }
    );
}

#[test]
fn reconstructs_interrupted_send_input_as_completed_collab_call() {
    // `send_input(interrupt=true)` first stops the child's active turn, then redirects it with
    // new input. The transient interrupted status should remain visible in agent state, but the
    // collab tool call itself is still a successful redirect rather than a failed operation.
    let sender =
        ThreadId::try_from("00000000-0000-0000-0000-000000000001").expect("valid sender thread id");
    let receiver = ThreadId::try_from("00000000-0000-0000-0000-000000000002")
        .expect("valid receiver thread id");
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "redirect".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::CollabAgentInteractionBegin(
            codex_protocol::protocol::CollabAgentInteractionBeginEvent {
                call_id: "send-1".into(),
                started_at_ms: 0,
                sender_thread_id: sender,
                receiver_thread_id: receiver,
                prompt: "new task".into(),
            },
        ),
        EventMsg::CollabAgentInteractionEnd(
            codex_protocol::protocol::CollabAgentInteractionEndEvent {
                call_id: "send-1".into(),
                completed_at_ms: 0,
                sender_thread_id: sender,
                receiver_thread_id: receiver,
                receiver_agent_nickname: None,
                receiver_agent_role: None,
                prompt: "new task".into(),
                status: AgentStatus::Interrupted,
            },
        ),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::CollabAgentToolCall {
            id: "send-1".into(),
            tool: CollabAgentTool::SendInput,
            status: CollabAgentToolCallStatus::Completed,
            sender_thread_id: sender.to_string(),
            receiver_thread_ids: vec![receiver.to_string()],
            prompt: Some("new task".into()),
            model: None,
            reasoning_effort: None,
            agents_states: [(
                receiver.to_string(),
                CollabAgentState {
                    status: crate::protocol::v2::CollabAgentStatus::Interrupted,
                    message: None,
                },
            )]
            .into_iter()
            .collect(),
        }
    );
}

#[test]
fn rollback_failed_error_does_not_mark_turn_failed() {
    let events = vec![
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::AgentMessage(AgentMessageEvent {
            message: "done".into(),
            phase: None,
            memory_citation: None,
        }),
        EventMsg::Error(ErrorEvent {
            message: "rollback failed".into(),
            codex_error_info: Some(CodexErrorInfo::ThreadRollbackFailed),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].status, TurnStatus::Completed);
    assert_eq!(turns[0].error, None);
}

#[test]
fn out_of_turn_error_does_not_create_or_fail_a_turn() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
        EventMsg::Error(ErrorEvent {
            message: "request-level failure".into(),
            codex_error_info: Some(CodexErrorInfo::BadRequest),
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(
        turns[0],
        Turn {
            id: "turn-a".into(),
            status: TurnStatus::Completed,
            error: None,
            started_at: None,
            completed_at: None,
            duration_ms: None,
            items_view: TurnItemsView::Full,
            items: vec![ThreadItem::UserMessage {
                id: "item-1".into(),
                client_id: None,
                content: vec![UserInput::Text {
                    text: "hello".into(),
                    text_elements: Vec::new(),
                }],
            }],
        }
    );
}

#[test]
fn error_then_turn_complete_preserves_failed_status() {
    let events = vec![
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
        EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
        EventMsg::Error(ErrorEvent {
            message: "stream failure".into(),
            codex_error_info: Some(CodexErrorInfo::ResponseStreamDisconnected {
                http_status_code: Some(502),
            }),
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        }),
    ];

    let items = events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].id, "turn-a");
    assert_eq!(turns[0].status, TurnStatus::Failed);
    assert_eq!(
        turns[0].error,
        Some(TurnError {
            message: "stream failure".into(),
            codex_error_info: Some(
                crate::protocol::v2::CodexErrorInfo::ResponseStreamDisconnected {
                    http_status_code: Some(502),
                }
            ),
            additional_details: None,
        })
    );
}

#[test]
fn rebuilds_hook_prompt_items_from_rollout_response_items() {
    let hook_prompt = build_hook_prompt_message(&[
        CoreHookPromptFragment::from_single_hook("Retry with tests.", "hook-run-1"),
        CoreHookPromptFragment::from_single_hook("Then summarize cleanly.", "hook-run-2"),
    ])
    .expect("hook prompt message");
    let items = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        })),
        RolloutItem::ResponseItem(hook_prompt.into()),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    ];

    let turns = build_turns_from_rollout_items(&items);

    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].items.len(), 2);
    assert_eq!(
        turns[0].items[1],
        ThreadItem::HookPrompt {
            id: turns[0].items[1].id().to_string(),
            fragments: vec![
                crate::protocol::v2::HookPromptFragment {
                    text: "Retry with tests.".into(),
                    hook_run_id: "hook-run-1".into(),
                },
                crate::protocol::v2::HookPromptFragment {
                    text: "Then summarize cleanly.".into(),
                    hook_run_id: "hook-run-2".into(),
                },
            ],
        }
    );
}

#[test]
fn canonical_hook_prompt_completion_updates_turn_history() {
    let hook_prompt = CoreTurnItem::HookPrompt(codex_protocol::items::HookPromptItem {
        id: "hook-prompt-1".into(),
        fragments: vec![CoreHookPromptFragment::from_single_hook(
            "Retry with tests.",
            "hook-run-1",
        )],
    });
    let expected_item = ThreadItem::from(hook_prompt.clone());
    let mut builder = ThreadHistoryBuilder::new();
    builder.handle_event(&EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: "turn-a".into(),
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }));
    builder.handle_event(&EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: ThreadId::new(),
        turn_id: "turn-a".into(),
        item: hook_prompt,
        completed_at_ms: 0,
    }));

    assert_eq!(
        builder.active_turn_snapshot().expect("active turn").items,
        vec![expected_item]
    );
}

#[test]
fn ignores_plain_user_response_items_in_rollout_replay() {
    let items = vec![
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::ResponseItem(
            codex_protocol::models::ResponseItem::Message {
                id: Some(codex_protocol::ResponseItemId::with_suffix("msg", "1")),
                role: "user".into(),
                content: vec![codex_protocol::models::ContentItem::InputText {
                    text: "plain text".into(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }
            .into(),
        ),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: None,
            duration_ms: None,
            time_to_first_token_ms: None,
        })),
    ];

    let turns = build_turns_from_rollout_items(&items);
    assert_eq!(turns.len(), 1);
    assert!(turns[0].items.is_empty());
}

#[test]
fn changed_rollout_item_reports_new_item_snapshot() {
    let mut builder = ThreadHistoryBuilder::new();

    let changes = builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(
        EventMsg::UserMessage(UserMessageEvent {
            client_id: Some("client-message-1".into()),
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        }),
    ));
    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: "rollout-0".into(),
                item: ThreadItem::UserMessage {
                    id: "item-1".into(),
                    client_id: Some("client-message-1".into()),
                    content: vec![UserInput::Text {
                        text: "hello".into(),
                        text_elements: Vec::new(),
                    }],
                },
            }],
            changed_turns: vec![ThreadHistoryTurnChange {
                turn_id: "rollout-0".into(),
                status: TurnStatus::Completed,
                abort_reason: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            }],
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_item_reports_updated_existing_item_snapshot() {
    let mut builder = ThreadHistoryBuilder::new();
    builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(EventMsg::WebSearchBegin(
        WebSearchBeginEvent {
            call_id: "search-1".into(),
        },
    )));

    let changes = builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(
        EventMsg::WebSearchEnd(WebSearchEndEvent {
            call_id: "search-1".into(),
            query: "codex".into(),
            action: CoreWebSearchAction::Search {
                query: Some("codex".into()),
                queries: None,
            },
            results: None,
        }),
    ));
    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: "rollout-0".into(),
                item: ThreadItem::WebSearch(WebSearchItem {
                    id: "search-1".into(),
                    query: "codex".into(),
                    action: Some(WebSearchAction::Search {
                        query: Some("codex".into()),
                        queries: None,
                    }),
                    results: None,
                }),
            }],
            changed_turns: Vec::new(),
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_item_reports_streaming_item_mutation() {
    let mut builder = ThreadHistoryBuilder::new();
    builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(EventMsg::AgentReasoning(
        AgentReasoningEvent {
            text: "summary".into(),
        },
    )));

    let changes = builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(
        EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent {
            text: "raw content".into(),
        }),
    ));
    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: "rollout-0".into(),
                item: ThreadItem::Reasoning {
                    id: "item-1".into(),
                    summary: vec!["summary".into()],
                    content: vec!["raw content".into()],
                },
            }],
            changed_turns: Vec::new(),
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_item_reports_turn_completion_metadata() {
    let mut builder = ThreadHistoryBuilder::new();

    let start_changes = builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: Some(10),
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        }),
    ));
    assert_eq!(
        start_changes,
        ThreadHistoryChangeSet {
            changed_items: Vec::new(),
            changed_turns: vec![ThreadHistoryTurnChange {
                turn_id: "turn-a".into(),
                status: TurnStatus::InProgress,
                abort_reason: None,
                error: None,
                started_at: Some(10),
                completed_at: None,
                duration_ms: None,
            }],
            removed_turn_ids: Vec::new(),
        }
    );

    builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(EventMsg::UserMessage(
        UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        },
    )));
    let complete_changes = builder.handle_rollout_item_with_changes(&RolloutItem::EventMsg(
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: Some(20),
            duration_ms: Some(123),
            time_to_first_token_ms: None,
        }),
    ));

    assert_eq!(
        complete_changes,
        ThreadHistoryChangeSet {
            changed_items: Vec::new(),
            changed_turns: vec![ThreadHistoryTurnChange {
                turn_id: "turn-a".into(),
                status: TurnStatus::Completed,
                abort_reason: None,
                error: None,
                started_at: Some(10),
                completed_at: Some(20),
                duration_ms: Some(123),
            }],
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_items_dedupe_updated_item_snapshots() {
    let mut builder = ThreadHistoryBuilder::new();
    let changes = builder.handle_rollout_items_with_changes(&[
        RolloutItem::EventMsg(EventMsg::WebSearchBegin(WebSearchBeginEvent {
            call_id: "search-1".into(),
        })),
        RolloutItem::EventMsg(EventMsg::WebSearchEnd(WebSearchEndEvent {
            call_id: "search-1".into(),
            query: "codex".into(),
            action: CoreWebSearchAction::Search {
                query: Some("codex".into()),
                queries: None,
            },
            results: None,
        })),
    ]);
    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: vec![ThreadHistoryItemChange {
                turn_id: "rollout-0".into(),
                item: ThreadItem::WebSearch(WebSearchItem {
                    id: "search-1".into(),
                    query: "codex".into(),
                    action: Some(WebSearchAction::Search {
                        query: Some("codex".into()),
                        queries: None,
                    }),
                    results: None,
                }),
            }],
            changed_turns: vec![ThreadHistoryTurnChange {
                turn_id: "rollout-0".into(),
                status: TurnStatus::Completed,
                abort_reason: None,
                error: None,
                started_at: None,
                completed_at: None,
                duration_ms: None,
            }],
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_items_dedupe_turn_metadata_snapshots() {
    let mut builder = ThreadHistoryBuilder::new();
    let changes = builder.handle_rollout_items_with_changes(&[
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: Some(10),
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-a".into(),
            started_at: None,
            last_agent_message: None,
            error: None,
            completed_at: Some(20),
            duration_ms: Some(123),
            time_to_first_token_ms: None,
        })),
    ]);

    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: Vec::new(),
            changed_turns: vec![ThreadHistoryTurnChange {
                turn_id: "turn-a".into(),
                status: TurnStatus::Completed,
                abort_reason: None,
                error: None,
                started_at: Some(10),
                completed_at: Some(20),
                duration_ms: Some(123),
            }],
            removed_turn_ids: Vec::new(),
        }
    );
}

#[test]
fn changed_rollout_items_drop_prior_changes_for_removed_turns() {
    let mut builder = ThreadHistoryBuilder::new();
    let changes = builder.handle_rollout_items_with_changes(&[
        RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-a".into(),
            trace_id: None,
            started_at: None,
            model_context_window: None,
            collaboration_mode_kind: Default::default(),
        })),
        RolloutItem::EventMsg(EventMsg::UserMessage(UserMessageEvent {
            client_id: None,
            message: "hello".into(),
            images: None,
            text_elements: Vec::new(),
            local_images: Vec::new(),
            ..Default::default()
        })),
        RolloutItem::EventMsg(EventMsg::ThreadRolledBack(ThreadRolledBackEvent {
            num_turns: 1,
        })),
    ]);

    assert_eq!(
        changes,
        ThreadHistoryChangeSet {
            changed_items: Vec::new(),
            changed_turns: Vec::new(),
            removed_turn_ids: vec!["turn-a".into()],
        }
    );
}
