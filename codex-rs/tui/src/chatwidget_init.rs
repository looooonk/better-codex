//! Shared initialization parameters for legacy chat-widget construction.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::app_event_sender::AppEventSender;
use crate::legacy_core::config::Config;
use crate::model_catalog::ModelCatalog;
use crate::status::StatusAccountDisplay;
use crate::tui::FrameRequester;
use crate::user_message::UserMessage;
use crate::workspace_command::WorkspaceCommandRunner;
use codex_otel::SessionTelemetry;
use codex_protocol::account::PlanType;

/// Common initialization parameters shared by all `ChatWidget` constructors.
pub(crate) struct ChatWidgetInit {
    pub(crate) config: Config,
    pub(crate) frame_requester: FrameRequester,
    pub(crate) app_event_tx: AppEventSender,
    /// App-server-backed runner used by status surfaces for workspace metadata probes.
    ///
    /// Tests that do not exercise git status-line refreshes may leave this unset. Production TUI
    /// construction provides a runner for the active app-server session.
    pub(crate) workspace_command_runner: Option<WorkspaceCommandRunner>,
    pub(crate) initial_user_message: Option<UserMessage>,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) has_chatgpt_account: bool,
    pub(crate) has_codex_backend_auth: bool,
    pub(crate) model_catalog: Arc<ModelCatalog>,
    pub(crate) feedback: codex_feedback::CodexFeedback,
    pub(crate) is_first_run: bool,
    pub(crate) status_account_display: Option<StatusAccountDisplay>,
    pub(crate) runtime_model_provider_base_url: Option<String>,
    pub(crate) initial_plan_type: Option<PlanType>,
    pub(crate) model: Option<String>,
    pub(crate) startup_tooltip_override: Option<String>,
    // Shared latch so we only warn once about invalid status-line item IDs.
    pub(crate) status_line_invalid_items_warned: Arc<AtomicBool>,
    // Shared latch so we only warn once about invalid terminal-title item IDs.
    pub(crate) terminal_title_invalid_items_warned: Arc<AtomicBool>,
    pub(crate) session_telemetry: SessionTelemetry,
}
