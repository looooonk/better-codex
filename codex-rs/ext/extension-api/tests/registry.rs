#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::Mutex;

use codex_extension_api::ApprovalReviewContributor;
use codex_extension_api::ApprovalReviewAction;
use codex_extension_api::ApprovalReviewBinding;
use codex_extension_api::ApprovalReviewCancellation;
use codex_extension_api::ApprovalReviewFailure;
use codex_extension_api::ApprovalReviewInput;
use codex_extension_api::ApprovalReviewOutcome;
use codex_extension_api::ApprovalReviewResult;
use codex_extension_api::ConfigContributor;
use codex_extension_api::ContextContributor;
use codex_extension_api::ContextualUserFragment;
use codex_extension_api::ExtensionData;
use codex_extension_api::ExtensionEventSink;
use codex_extension_api::ExtensionFuture;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_extension_api::PromptFragment;
use codex_extension_api::PromptSlot;
use codex_extension_api::SkillInvocationContributor;
use codex_extension_api::ThreadLifecycleContributor;
use codex_extension_api::TokenUsageContributor;
use codex_extension_api::ToolCall;
use codex_extension_api::ToolContributor;
use codex_extension_api::ToolExecutor;
use codex_extension_api::ToolLifecycleContributor;
use codex_extension_api::TurnContextContributionInput;
use codex_extension_api::TurnInputContext;
use codex_extension_api::TurnInputContributor;
use codex_extension_api::TurnItemContributor;
use codex_extension_api::TurnLifecycleContributor;
use codex_extension_api::empty_extension_registry;
use codex_protocol::items::HookPromptItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::GuardianRiskLevel;
use codex_protocol::protocol::GuardianUserAuthorization;
use codex_protocol::protocol::WarningEvent;
use pretty_assertions::assert_eq;

struct TestPromptFragment {
    role: &'static str,
    text: &'static str,
}

impl ContextualUserFragment for TestPromptFragment {
    fn role(&self) -> &'static str {
        self.role
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn body(&self) -> String {
        self.text.to_string()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }
}

fn developer_fragment(text: &'static str) -> TestPromptFragment {
    TestPromptFragment {
        role: "developer",
        text,
    }
}

fn user_fragment(text: &'static str) -> TestPromptFragment {
    TestPromptFragment { role: "user", text }
}

struct AllContributors;

impl ContextContributor for AllContributors {
    fn contribute_thread_context<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        _thread_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<PromptFragment>> {
        Box::pin(std::future::ready(Vec::new()))
    }
}

impl ThreadLifecycleContributor<()> for AllContributors {}

impl TurnLifecycleContributor for AllContributors {}

impl ConfigContributor<()> for AllContributors {}

impl TokenUsageContributor for AllContributors {}

impl SkillInvocationContributor for AllContributors {}

impl TurnInputContributor for AllContributors {
    fn contribute<'a>(
        &'a self,
        input: TurnInputContext,
        _session_store: &'a ExtensionData,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<Box<dyn ContextualUserFragment + Send>>> {
        Box::pin(async move {
            let _self = self;
            let _input = input;
            Vec::new()
        })
    }
}

impl ToolContributor for AllContributors {
    fn tools(
        &self,
        _session_store: &ExtensionData,
        _thread_store: &ExtensionData,
    ) -> Vec<Arc<dyn ToolExecutor<ToolCall>>> {
        Vec::new()
    }
}

impl ToolLifecycleContributor for AllContributors {}

impl TurnItemContributor for AllContributors {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
        _item: &'a mut TurnItem,
    ) -> ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let _self = self;
            Ok(())
        })
    }
}

impl ApprovalReviewContributor for AllContributors {
    fn review<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        _thread_store: &'a ExtensionData,
        _input: ApprovalReviewInput,
    ) -> ExtensionFuture<'a, ApprovalReviewResult> {
        Box::pin(async move {
            let _self = self;
            ApprovalReviewResult::Allow(review_outcome())
        })
    }
}

struct TestCancellation;

impl ApprovalReviewCancellation for TestCancellation {
    fn is_cancelled(&self) -> bool {
        false
    }

    fn cancelled(&self) -> ExtensionFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
}

fn review_outcome() -> ApprovalReviewOutcome {
    ApprovalReviewOutcome {
        risk_level: GuardianRiskLevel::Low,
        user_authorization: GuardianUserAuthorization::High,
        rationale: "allowed".to_string(),
    }
}

fn review_input() -> ApprovalReviewInput {
    let cwd = codex_utils_absolute_path::AbsolutePathBuf::from_absolute_path(
        std::env::current_dir().expect("current directory"),
    )
    .expect("current directory should be absolute");
    ApprovalReviewInput {
        binding: ApprovalReviewBinding {
            thread_id: "thread".to_string(),
            turn_id: "turn".to_string(),
            action_id: "action".to_string(),
            attempt_id: "attempt".to_string(),
            source: codex_extension_api::ToolCallSource::Direct,
            evidence_revision: 0,
        },
        action: ApprovalReviewAction::ApplyPatch {
            cwd,
            files: Vec::new(),
            patch: String::new(),
        },
        history: Vec::new(),
        evidence: Vec::new(),
        images: Vec::new(),
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(1),
        cancellation: Arc::new(TestCancellation),
    }
}

#[tokio::test]
async fn build_round_trips_every_contributor_category() {
    let contributor = Arc::new(AllContributors);
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    builder.thread_lifecycle_contributor(contributor.clone());
    builder.turn_lifecycle_contributor(contributor.clone());
    builder.config_contributor(contributor.clone());
    builder.token_usage_contributor(contributor.clone());
    builder.skill_invocation_contributor(contributor.clone());
    builder.prompt_contributor(contributor.clone());
    builder.turn_input_contributor(contributor.clone());
    builder.tool_contributor(contributor.clone());
    builder.tool_lifecycle_contributor(contributor.clone());
    builder.turn_item_contributor(contributor.clone());
    builder.approval_review_contributor(contributor);
    let registry = builder.build();

    assert_eq!(registry.thread_lifecycle_contributors().len(), 1);
    assert_eq!(registry.turn_lifecycle_contributors().len(), 1);
    assert_eq!(registry.config_contributors().len(), 1);
    assert_eq!(registry.token_usage_contributors().len(), 1);
    assert_eq!(registry.skill_invocation_contributors().len(), 1);
    assert_eq!(registry.context_contributors().len(), 1);
    assert_eq!(registry.turn_input_contributors().len(), 1);
    assert_eq!(registry.tool_contributors().len(), 1);
    assert_eq!(registry.tool_lifecycle_contributors().len(), 1);
    assert_eq!(registry.turn_item_contributors().len(), 1);
    assert_eq!(
        registry
            .approval_review(
                &ExtensionData::new("session"),
                &ExtensionData::new("thread"),
                review_input(),
            )
            .await,
        ApprovalReviewResult::Allow(review_outcome())
    );
}

struct NamedContextContributor(&'static str);

impl ContextContributor for NamedContextContributor {
    fn contribute_thread_context<'a>(
        &'a self,
        _session_store: &'a ExtensionData,
        _thread_store: &'a ExtensionData,
    ) -> ExtensionFuture<'a, Vec<PromptFragment>> {
        Box::pin(std::future::ready(vec![PromptFragment::developer_policy(
            developer_fragment(self.0),
        )]))
    }
}

struct NamedTurnContextContributor(&'static str);

impl ContextContributor for NamedTurnContextContributor {
    fn contribute_turn_context<'a>(
        &'a self,
        _input: TurnContextContributionInput<'a>,
    ) -> ExtensionFuture<'a, Vec<PromptFragment>> {
        Box::pin(std::future::ready(vec![PromptFragment::new(
            PromptSlot::ContextualUser,
            user_fragment(self.0),
        )]))
    }
}

struct RecordingTurnItemContributor {
    name: &'static str,
    calls: Arc<Mutex<Vec<&'static str>>>,
}

impl TurnItemContributor for RecordingTurnItemContributor {
    fn contribute<'a>(
        &'a self,
        _thread_store: &'a ExtensionData,
        _turn_store: &'a ExtensionData,
        _item: &'a mut TurnItem,
    ) -> ExtensionFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("turn item calls lock should not be poisoned")
                .push(self.name);
            Ok(())
        })
    }
}

#[tokio::test]
async fn contributors_preserve_registration_order() {
    let turn_item_calls = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    builder.prompt_contributor(Arc::new(NamedContextContributor("first")));
    builder.prompt_contributor(Arc::new(NamedContextContributor("second")));
    builder.prompt_contributor(Arc::new(NamedTurnContextContributor("turn-first")));
    builder.prompt_contributor(Arc::new(NamedTurnContextContributor("turn-second")));
    for name in ["first", "second"] {
        builder.turn_item_contributor(Arc::new(RecordingTurnItemContributor {
            name,
            calls: Arc::clone(&turn_item_calls),
        }));
    }
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    let turn_store = ExtensionData::new("turn");

    let mut fragments = Vec::new();
    for contributor in registry.context_contributors() {
        fragments.extend(
            contributor
                .contribute_thread_context(&session_store, &thread_store)
                .await,
        );
    }
    for contributor in registry.context_contributors() {
        fragments.extend(
            contributor
                .contribute_turn_context(TurnContextContributionInput {
                    thread_id: codex_protocol::ThreadId::default(),
                    turn_id: turn_store.level_id(),
                    session_store: &session_store,
                    thread_store: &thread_store,
                    turn_store: &turn_store,
                    model_context_window: Some(123),
                })
                .await,
        );
    }
    let mut item = TurnItem::HookPrompt(HookPromptItem {
        id: "item".to_string(),
        fragments: Vec::new(),
    });
    for contributor in registry.turn_item_contributors() {
        contributor
            .contribute(&thread_store, &turn_store, &mut item)
            .await
            .expect("turn item contribution should succeed");
    }

    assert_eq!(
        fragments
            .iter()
            .map(|fragment| (fragment.slot(), fragment.render()))
            .collect::<Vec<_>>(),
        vec![
            (PromptSlot::DeveloperPolicy, "first".to_string()),
            (PromptSlot::DeveloperPolicy, "second".to_string()),
            (PromptSlot::ContextualUser, "turn-first".to_string()),
            (PromptSlot::ContextualUser, "turn-second".to_string()),
        ]
    );
    assert_eq!(
        turn_item_calls
            .lock()
            .expect("turn item calls lock")
            .as_slice(),
        ["first", "second"]
    );
}

#[derive(Debug, PartialEq, Eq)]
struct ApprovalCall {
    contributor: &'static str,
    session_id: String,
    thread_id: String,
    action_id: String,
}

struct RecordingApprovalContributor {
    name: &'static str,
    decision: ApprovalReviewResult,
    calls: Arc<Mutex<Vec<ApprovalCall>>>,
}

impl ApprovalReviewContributor for RecordingApprovalContributor {
    fn review<'a>(
        &'a self,
        session_store: &'a ExtensionData,
        thread_store: &'a ExtensionData,
        input: ApprovalReviewInput,
    ) -> ExtensionFuture<'a, ApprovalReviewResult> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("approval calls lock should not be poisoned")
                .push(ApprovalCall {
                    contributor: self.name,
                    session_id: session_store.level_id().to_string(),
                    thread_id: thread_store.level_id().to_string(),
                    action_id: input.binding.action_id,
                });
            self.decision.clone()
        })
    }
}

#[tokio::test]
async fn approval_review_requires_exactly_one_authority() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut builder = ExtensionRegistryBuilder::<()>::new();
    for name in ["first", "second"] {
        builder.approval_review_contributor(Arc::new(RecordingApprovalContributor {
            name,
            decision: ApprovalReviewResult::Allow(review_outcome()),
            calls: Arc::clone(&calls),
        }));
    }
    let registry = builder.build();

    let decision = registry
        .approval_review(
            &ExtensionData::new("session-1"),
            &ExtensionData::new("thread-1"),
            review_input(),
        )
        .await;

    assert_eq!(
        decision,
        ApprovalReviewResult::ManualReview(ApprovalReviewFailure::MultipleAuthorities)
    );
    assert!(calls.lock().expect("approval calls lock").is_empty());
}

#[derive(Default)]
struct RecordingEventSink {
    events: Mutex<Vec<(String, String)>>,
}

impl ExtensionEventSink for RecordingEventSink {
    fn emit(&self, event: Event) {
        let EventMsg::Warning(warning) = event.msg else {
            panic!("test sink only accepts warning events");
        };
        self.events
            .lock()
            .expect("recording event sink lock should not be poisoned")
            .push((event.id, warning.message));
    }
}

#[test]
fn custom_event_sink_survives_registry_build() {
    let sink = Arc::new(RecordingEventSink::default());
    let builder = ExtensionRegistryBuilder::<()>::with_event_sink(sink.clone());

    builder
        .event_sink()
        .emit(warning_event("builder", "before"));
    let registry = builder.build();
    registry
        .event_sink()
        .emit(warning_event("registry", "after"));

    assert_eq!(
        sink.events
            .lock()
            .expect("recording event sink lock")
            .as_slice(),
        [
            ("builder".to_string(), "before".to_string()),
            ("registry".to_string(), "after".to_string()),
        ]
    );
}

#[tokio::test]
async fn empty_registry_requires_manual_approval_review() {
    let registry = empty_extension_registry::<()>();

    assert_eq!(
        registry
            .approval_review(
                &ExtensionData::new("session"),
                &ExtensionData::new("thread"),
                review_input(),
            )
            .await,
        ApprovalReviewResult::ManualReview(ApprovalReviewFailure::NotInstalled)
    );
}

fn warning_event(id: &str, message: &str) -> Event {
    Event {
        id: id.to_string(),
        msg: EventMsg::Warning(WarningEvent {
            message: message.to_string(),
        }),
    }
}
