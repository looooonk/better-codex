use strum_macros::Display;
use strum_macros::EnumIter;
use strum_macros::EnumString;

#[derive(EnumIter, EnumString, Display, Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
#[strum(serialize_all = "kebab_case")]
pub(crate) enum StatusLineItem {
    /// The current model name.
    #[strum(to_string = "model", serialize = "model-name")]
    ModelName,

    /// Model name with reasoning level suffix.
    ModelWithReasoning,

    /// Current reasoning level.
    Reasoning,

    /// Current working directory path.
    CurrentDir,

    /// Project root directory (if detected).
    #[strum(
        to_string = "project-name",
        serialize = "project",
        serialize = "project-root"
    )]
    ProjectRoot,

    /// Hostname of the machine running Codex.
    Hostname,

    /// Current git branch name (if in a repository).
    GitBranch,

    /// Open pull request number for the current branch.
    PullRequestNumber,

    /// Committed branch diff stats relative to the default branch.
    BranchChanges,

    /// Compact runtime run-state text.
    #[strum(to_string = "run-state", serialize = "status")]
    Status,

    /// Active permission profile or sandbox summary.
    Permissions,

    /// Active command approval mode.
    #[strum(to_string = "approval-mode", serialize = "approval")]
    ApprovalMode,

    /// Percentage of context window remaining.
    ContextRemaining,

    /// Percentage of context window used.
    ///
    /// Also accepts the legacy `context-usage` config value.
    #[strum(to_string = "context-used", serialize = "context-usage")]
    ContextUsed,

    /// Remaining usage on the primary rate limit.
    FiveHourLimit,

    /// Remaining usage on the secondary rate limit.
    WeeklyLimit,

    /// Codex application version.
    CodexVersion,

    /// Total context window size in tokens.
    ContextWindowSize,

    /// Total tokens used in the current session.
    UsedTokens,

    /// Total input tokens consumed.
    TotalInputTokens,

    /// Total output tokens generated.
    TotalOutputTokens,

    /// Estimated credits attributed directly to the current enterprise thread.
    ThreadCredits,

    /// Estimated dollar cost attributed directly to the current enterprise thread.
    EstimatedThreadCost,

    /// Full thread UUID.
    #[strum(to_string = "thread-id", serialize = "session-id")]
    SessionId,

    /// Whether Fast mode is currently active.
    FastMode,

    /// Whether Daybreak is enabled for this thread.
    Daybreak,

    /// Whether raw scrollback mode is currently active.
    RawOutput,

    /// Current thread name, omitted when unnamed.
    ThreadName,

    /// Current thread title, falling back to its identifier when unnamed.
    ThreadTitle,

    /// Current workspace notification headline.
    WorkspaceHeadline,

    /// Latest checklist task progress from `update_plan` (if available).
    TaskProgress,
}

impl StatusLineItem {
    /// User-visible description shown in the popup.
    pub(crate) fn description(self) -> &'static str {
        match self {
            StatusLineItem::ModelName => "Current model name",
            StatusLineItem::ModelWithReasoning => "Current model name with reasoning level",
            StatusLineItem::Reasoning => "Current reasoning level",
            StatusLineItem::CurrentDir => "Current working directory",
            StatusLineItem::ProjectRoot => "Project name (omitted when unavailable)",
            StatusLineItem::Hostname => "Current machine hostname (omitted when unavailable)",
            StatusLineItem::GitBranch => "Current Git branch (omitted when unavailable)",
            StatusLineItem::PullRequestNumber => {
                "Open pull request number for the current branch (omitted when unavailable)"
            }
            StatusLineItem::BranchChanges => {
                "Committed branch changes against the default branch (omitted when unavailable)"
            }
            StatusLineItem::Status => "Compact session run-state text (Ready, Working, Thinking)",
            StatusLineItem::Permissions => "Active permission profile or sandbox mode",
            StatusLineItem::ApprovalMode => "Active command approval mode",
            StatusLineItem::ContextRemaining => {
                "Percentage of context window remaining (omitted when unknown)"
            }
            StatusLineItem::ContextUsed => {
                "Percentage of context window used (omitted when unknown)"
            }
            StatusLineItem::FiveHourLimit => {
                "Remaining usage on the primary usage limit (omitted when unavailable)"
            }
            StatusLineItem::WeeklyLimit => {
                "Remaining usage on the secondary usage limit (omitted when unavailable)"
            }
            StatusLineItem::CodexVersion => "Codex application version",
            StatusLineItem::ContextWindowSize => {
                "Total context window size in tokens (omitted when unknown)"
            }
            StatusLineItem::UsedTokens => "Total tokens used in session (omitted when zero)",
            StatusLineItem::TotalInputTokens => "Total input tokens used in session",
            StatusLineItem::TotalOutputTokens => "Total output tokens used in session",
            StatusLineItem::ThreadCredits => {
                "Estimated current-thread credits (Enterprise workspaces only; omitted when unavailable)"
            }
            StatusLineItem::EstimatedThreadCost => {
                "Estimated current-thread cost in USD (Enterprise workspaces only; omitted when unavailable)"
            }
            StatusLineItem::SessionId => "Current thread identifier (omitted until thread starts)",
            StatusLineItem::FastMode => "Whether Fast mode is currently active",
            StatusLineItem::Daybreak => "Whether Daybreak is enabled for this thread",
            StatusLineItem::RawOutput => "Whether raw scrollback mode is active",
            StatusLineItem::ThreadName => "Current thread name (omitted when unnamed)",
            StatusLineItem::ThreadTitle => {
                "Current thread title, or thread identifier when unnamed"
            }
            StatusLineItem::WorkspaceHeadline => {
                "Workspace notification headline (Enterprise workspaces only; omitted when unavailable)"
            }
            StatusLineItem::TaskProgress => {
                "Latest task progress from update_plan (omitted until available)"
            }
        }
    }
}

#[derive(EnumIter, EnumString, Display, Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum TerminalTitleItem {
    /// Codex app name.
    AppName,
    /// Project root name, or a compact cwd fallback.
    #[strum(to_string = "project-name", serialize = "project")]
    Project,
    /// Current working directory path.
    CurrentDir,
    /// Terminal-title activity indicator while active.
    #[strum(to_string = "activity", serialize = "spinner")]
    Spinner,
    /// Compact runtime run-state text.
    #[strum(to_string = "run-state", serialize = "status")]
    Status,
    /// Current thread name, omitted when unnamed.
    ThreadName,
    /// Current thread title (if available).
    #[strum(to_string = "thread-title", serialize = "thread")]
    Thread,
    /// Current git branch (if available).
    GitBranch,
    /// Percentage of context window remaining.
    ContextRemaining,
    /// Percentage of context window used.
    #[strum(to_string = "context-used", serialize = "context-usage")]
    ContextUsed,
    /// Remaining usage on the primary rate limit.
    FiveHourLimit,
    /// Remaining usage on the secondary rate limit.
    WeeklyLimit,
    /// Codex application version.
    CodexVersion,
    /// Total tokens used in the current session.
    UsedTokens,
    /// Total input tokens consumed.
    TotalInputTokens,
    /// Total output tokens generated.
    TotalOutputTokens,
    /// Estimated credits attributed directly to the current enterprise thread.
    ThreadCredits,
    /// Estimated dollar cost attributed directly to the current enterprise thread.
    EstimatedThreadCost,
    /// Full thread UUID.
    #[strum(to_string = "thread-id", serialize = "session-id")]
    SessionId,
    /// Whether Fast mode is currently active.
    FastMode,
    /// Whether Daybreak is enabled for this thread.
    Daybreak,
    /// Current model name.
    #[strum(to_string = "model", serialize = "model-name")]
    Model,
    /// Current model name with reasoning level.
    ModelWithReasoning,
    /// Current reasoning level.
    Reasoning,
    /// Latest checklist task progress from `update_plan` (if available).
    TaskProgress,
}

impl TerminalTitleItem {
    pub(crate) fn description(self) -> &'static str {
        match self {
            TerminalTitleItem::AppName => "Codex app name",
            TerminalTitleItem::Project => "Project name (falls back to current directory name)",
            TerminalTitleItem::CurrentDir => "Current working directory",
            TerminalTitleItem::Spinner => {
                "Spinner while working, action-required message while blocked"
            }
            TerminalTitleItem::Status => {
                "Compact session run-state text (Ready, Working, Thinking)"
            }
            TerminalTitleItem::ThreadName => "Current thread name (omitted when unnamed)",
            TerminalTitleItem::Thread => "Current thread title, or thread identifier when unnamed",
            TerminalTitleItem::GitBranch => "Current Git branch (omitted when unavailable)",
            TerminalTitleItem::ContextRemaining => {
                "Percentage of context window remaining (omitted when unknown)"
            }
            TerminalTitleItem::ContextUsed => {
                "Percentage of context window used (omitted when unknown)"
            }
            TerminalTitleItem::FiveHourLimit => {
                "Remaining usage on the primary usage limit (omitted when unavailable)"
            }
            TerminalTitleItem::WeeklyLimit => {
                "Remaining usage on the secondary usage limit (omitted when unavailable)"
            }
            TerminalTitleItem::CodexVersion => "Codex application version",
            TerminalTitleItem::UsedTokens => "Total tokens used in session (omitted when zero)",
            TerminalTitleItem::TotalInputTokens => "Total input tokens used in session",
            TerminalTitleItem::TotalOutputTokens => "Total output tokens used in session",
            TerminalTitleItem::ThreadCredits => {
                "Estimated current-thread credits (Enterprise workspaces only; omitted when unavailable)"
            }
            TerminalTitleItem::EstimatedThreadCost => {
                "Estimated current-thread cost (Enterprise workspaces only; omitted when unavailable)"
            }
            TerminalTitleItem::SessionId => {
                "Current thread identifier (omitted until thread starts)"
            }
            TerminalTitleItem::FastMode => "Whether Fast mode is currently active",
            TerminalTitleItem::Daybreak => "Whether Daybreak is enabled for this thread",
            TerminalTitleItem::Model => "Current model name",
            TerminalTitleItem::ModelWithReasoning => "Current model name with reasoning level",
            TerminalTitleItem::Reasoning => "Current reasoning level",
            TerminalTitleItem::TaskProgress => {
                "Latest task progress from update_plan (omitted until available)"
            }
        }
    }
}
