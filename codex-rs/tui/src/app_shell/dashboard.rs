use super::ShellState;
use super::ToolActivity;
use super::agent_activity_render::agent_activity_inspector_lines;
use super::agent_activity_render::agent_activity_overview_lines;
use super::dashboard_rate_limits::credits_and_resets_line;
use super::dashboard_rate_limits::rate_limit_lines;
use super::dashboard_workspace::workspace_lines;
use super::design::Tone;
use super::design::badge_span;
use super::design::palette;
use super::diff_style::diff_stat_spans;
use super::navigation::DashboardRoute;
use super::navigation::DashboardTabs;
use super::thread_usage::thread_usage_line;
use crate::goal_display::format_goal_elapsed_seconds;
use crate::goal_display::goal_status_label;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use codex_app_server_protocol::TurnPlanStepStatus;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::VecDeque;

pub(super) struct DashboardPanel {
    pub(super) title: String,
    pub(super) lines: Vec<Line<'static>>,
    title_hint: Option<String>,
    pub(super) show_title: bool,
}

#[derive(Clone, Copy)]
enum DashboardPanelKind {
    Navigation,
    Sessions,
    Settings,
    Thread,
    Tokens,
    Approvals,
    Background,
    RateLimits,
    Edits,
    Goal,
    Plan,
    Tools,
    Agents,
    Workspace,
    Keys,
}

impl DashboardPanel {
    fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
            title_hint: None,
            show_title: true,
        }
    }

    pub(super) fn title_line(&self) -> Line<'static> {
        let mut spans = vec![
            "◆ ".set_style(ratatui::style::Style::new().fg(palette::focus())),
            self.title
                .to_uppercase()
                .set_style(ratatui::style::Style::new().fg(palette::muted()).bold()),
        ];
        if let Some(hint) = &self.title_hint {
            spans.extend(["  ".into(), hint.clone().dim()]);
        }
        Line::from(spans)
    }

    pub(super) fn height(&self) -> u16 {
        u16::try_from(
            self.lines
                .len()
                .saturating_add(usize::from(self.show_title)),
        )
        .unwrap_or(u16::MAX)
    }

    pub(super) fn render_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.show_title
            .then(|| self.title_line())
            .into_iter()
            .chain(self.lines.iter().cloned())
            .map(|line| truncate_line_with_ellipsis_if_overflow(line, width))
            .collect()
    }
}

pub(super) fn dashboard_panels(
    shell: &ShellState,
    width: usize,
    height: usize,
) -> Vec<DashboardPanel> {
    dashboard_panel_kinds(shell.dashboard_route)
        .iter()
        .filter_map(|kind| dashboard_panel(shell, width, height, *kind))
        .collect()
}

pub(super) fn dashboard_value(text: &str, line_width: usize, prefix_width: usize) -> String {
    let max_width = line_width.saturating_sub(prefix_width).max(1);
    truncate_line_with_ellipsis_if_overflow(Line::from(text.to_string()), max_width).to_string()
}

pub(super) fn format_usize(value: usize) -> String {
    format_u64(value as u64)
}

pub(super) fn context_used_percent(
    usage: &crate::token_usage::TokenUsage,
    model_context_window: Option<i64>,
) -> Option<i64> {
    Some(100 - context_remaining_percent(usage, model_context_window)?)
}

fn context_remaining_percent(
    usage: &crate::token_usage::TokenUsage,
    model_context_window: Option<i64>,
) -> Option<i64> {
    let context_window = model_context_window.filter(|window| *window > 0)?;
    Some(usage.percent_of_context_window_remaining(context_window))
}

fn dashboard_panel_kinds(route: DashboardRoute) -> &'static [DashboardPanelKind] {
    match route {
        DashboardRoute::Sessions => &[
            DashboardPanelKind::Navigation,
            DashboardPanelKind::Sessions,
            DashboardPanelKind::Thread,
        ],
        DashboardRoute::Agents => &[
            DashboardPanelKind::Navigation,
            DashboardPanelKind::Agents,
            DashboardPanelKind::Approvals,
        ],
        DashboardRoute::Status => &[
            DashboardPanelKind::Navigation,
            DashboardPanelKind::Settings,
            DashboardPanelKind::Goal,
            DashboardPanelKind::Plan,
            DashboardPanelKind::Tools,
            DashboardPanelKind::Edits,
            DashboardPanelKind::Workspace,
            DashboardPanelKind::Tokens,
            DashboardPanelKind::RateLimits,
        ],
        DashboardRoute::Help => &[
            DashboardPanelKind::Navigation,
            DashboardPanelKind::Keys,
            DashboardPanelKind::Approvals,
            DashboardPanelKind::Background,
        ],
    }
}

fn dashboard_panel(
    shell: &ShellState,
    width: usize,
    height: usize,
    kind: DashboardPanelKind,
) -> Option<DashboardPanel> {
    let content_width = width.saturating_sub(1);
    match kind {
        DashboardPanelKind::Navigation => {
            let mut panel = dashboard_navigation_panel(shell.dashboard_route, width);
            if shell.dashboard_route == DashboardRoute::Help
                && super::dashboard_help::uses_dense_layout(width)
            {
                panel.lines.truncate(1);
            }
            Some(panel)
        }
        DashboardPanelKind::Sessions => Some(DashboardPanel::new(
            "Sessions",
            shell.session_list.lines(content_width),
        )),
        DashboardPanelKind::Settings => Some(DashboardPanel::new(
            "Settings",
            shell.settings.lines(&shell.settings_view(), content_width),
        )),
        DashboardPanelKind::Thread => {
            let thread_label = shell.thread_name.as_deref().unwrap_or("untitled thread");
            Some(DashboardPanel::new(
                "Thread",
                vec![
                    Line::from(dashboard_value(
                        thread_label,
                        content_width,
                        /*prefix_width*/ 0,
                    )),
                    Line::from(vec![
                        "id ".dim(),
                        dashboard_value(
                            &shell.thread_id.to_string(),
                            content_width,
                            /*prefix_width*/ 3,
                        )
                        .fg(palette::cyan()),
                    ]),
                    Line::from(
                        dashboard_value(
                            "resume, fork, archive, delete in session list",
                            content_width,
                            /*prefix_width*/ 0,
                        )
                        .dim(),
                    ),
                ],
            ))
        }
        DashboardPanelKind::Tokens => {
            let mut lines = vec![
                Line::from(format!(
                    "input {} | output {}",
                    format_token_count(shell.token_usage.input_tokens),
                    format_token_count(shell.token_usage.output_tokens)
                )),
                Line::from(format!(
                    "Context {}% left",
                    context_remaining_percent(
                        &shell.context_token_usage,
                        shell.model_context_window,
                    )
                    .unwrap_or(100)
                )),
            ];
            if let Some(line) = shell.thread_usage.as_ref().and_then(thread_usage_line) {
                lines.push(line);
            }
            Some(DashboardPanel::new("Tokens", lines))
        }
        DashboardPanelKind::Approvals => (shell.pending_approval.is_some()
            || shell.pending_elicitation.is_some()
            || shell.pending_user_input.is_some())
        .then(|| DashboardPanel::new("Approvals", approval_activity_lines(shell, content_width))),
        DashboardPanelKind::Background => {
            background_activity_lines(shell).map(|lines| DashboardPanel::new("Background", lines))
        }
        DashboardPanelKind::RateLimits => (!shell.rate_limits.is_empty()
            || shell.rate_limit_reset_credits.is_some())
        .then(|| {
            let current_time_at = chrono::Utc::now().timestamp();
            let mut lines = rate_limit_lines(&shell.rate_limits, content_width, current_time_at);
            lines.push(credits_and_resets_line(
                &shell.rate_limits,
                shell.rate_limit_reset_credits,
            ));
            DashboardPanel::new("Rate Limits", lines)
        }),
        DashboardPanelKind::Edits => {
            let session = shell.diff_store.session_stats();
            let has_recorded_history = shell.diff_store.has_recorded_history();
            let truncated = shell.diff_store.session_is_truncated();
            let summary = if has_recorded_history || truncated {
                (session.files > 0).then_some((session.files, session.additions, session.removals))
            } else {
                // Snapshot and benchmark fixtures can provide a summary without retained rows.
                // Real edit notifications always create DiffStore history first.
                shell
                    .latest_diff
                    .as_ref()
                    .map(|diff| (diff.files, diff.additions, diff.removals))
            };
            let mut lines = summary.map_or_else(
                || {
                    vec![Line::from(
                        if truncated {
                            "retained edit history is incomplete"
                        } else {
                            "no changes"
                        }
                        .dim(),
                    )]
                },
                |(files, additions, removals)| {
                    vec![Line::from(diff_stat_spans(format!(
                        "{} files +{} -{}",
                        format_usize(files),
                        format_usize(additions),
                        format_usize(removals)
                    )))]
                },
            );
            if truncated && summary.is_some() {
                lines.push(Line::from(
                    "retained subset; totals may be incomplete".dim(),
                ));
            }
            Some(DashboardPanel::new("Edits", lines))
        }
        DashboardPanelKind::Goal => shell.active_goal.as_ref().map(|goal| {
            let mut lines = vec![
                Line::from(vec!["status ".dim(), goal_status_span(goal.status)]),
                Line::from(dashboard_value(
                    &goal.objective,
                    content_width,
                    /*prefix_width*/ 0,
                )),
            ];
            let mut usage = Vec::new();
            if goal.time_used_seconds > 0 {
                usage.push(format_goal_elapsed_seconds(goal.time_used_seconds));
            }
            if let Some(token_budget) = goal.token_budget {
                usage.push(format!(
                    "{}/{} tokens",
                    format_i64(goal.tokens_used),
                    format_i64(token_budget)
                ));
            } else if goal.tokens_used > 0 {
                usage.push(format!("{} tokens", format_i64(goal.tokens_used)));
            }
            if !usage.is_empty() {
                lines.push(Line::from(usage.join(" | ").dim()));
            }
            DashboardPanel::new("Goal", lines)
        }),
        DashboardPanelKind::Plan => {
            let mut lines = Vec::new();
            if let Some(explanation) = &shell.plan_explanation {
                lines.push(Line::from(explanation.clone().dim()));
            }
            if shell.plan_steps.is_empty() {
                lines.push(Line::from("no active plan".dim()));
            } else {
                for step in shell.plan_steps.iter().take(5) {
                    lines.push(plan_step_line(step.status, &step.step));
                }
            }
            Some(DashboardPanel::new("Plan", lines))
        }
        DashboardPanelKind::Tools => Some(DashboardPanel::new(
            "Tools",
            activity_lines(&shell.tool_activity, content_width, "idle"),
        )),
        DashboardPanelKind::Agents => {
            let mut lines = agent_activity_overview_lines(&shell.agent_activity, content_width);
            lines.extend(agent_activity_inspector_lines(
                &shell.agent_activity,
                content_width,
                /*line_budget*/ 23,
            ));
            lines.push(Line::from(
                if shell.agents_focused {
                    "Enter full log · j/k select · Esc release"
                } else {
                    "Enter focus · then Enter opens full log"
                }
                .fg(if shell.agents_focused {
                    palette::cyan()
                } else {
                    palette::muted()
                }),
            ));
            Some(DashboardPanel::new("Agents", lines))
        }
        DashboardPanelKind::Workspace => Some(DashboardPanel::new(
            "Workspace",
            workspace_lines(shell, content_width),
        )),
        DashboardPanelKind::Keys => {
            let dense = super::dashboard_help::uses_dense_layout(width);
            let mut panel = DashboardPanel::new(
                "Keys",
                super::dashboard_help::key_hint_lines(shell, width, height),
            );
            if dense {
                panel.show_title = false;
            }
            Some(panel)
        }
    }
}

fn dashboard_navigation_panel(active_route: DashboardRoute, width: usize) -> DashboardPanel {
    let width = u16::try_from(width).unwrap_or(u16::MAX);
    DashboardPanel {
        title: "Navigation".to_string(),
        lines: DashboardTabs::new(width).lines(active_route).into(),
        title_hint: None,
        show_title: false,
    }
}

fn goal_status_span(status: codex_app_server_protocol::ThreadGoalStatus) -> Span<'static> {
    let label = goal_status_label(status);
    match status {
        codex_app_server_protocol::ThreadGoalStatus::Active => badge_span(label, Tone::Focus),
        codex_app_server_protocol::ThreadGoalStatus::Complete => badge_span(label, Tone::Success),
        codex_app_server_protocol::ThreadGoalStatus::Blocked
        | codex_app_server_protocol::ThreadGoalStatus::UsageLimited
        | codex_app_server_protocol::ThreadGoalStatus::BudgetLimited => {
            badge_span(label, Tone::Danger)
        }
        codex_app_server_protocol::ThreadGoalStatus::Paused => badge_span(label, Tone::Codex),
    }
}

fn plan_step_line(status: TurnPlanStepStatus, step: &str) -> Line<'static> {
    let marker = match status {
        TurnPlanStepStatus::Pending => "-".dim(),
        TurnPlanStepStatus::InProgress => ">".fg(palette::cyan()).bold(),
        TurnPlanStepStatus::Completed => "x".fg(palette::success()),
    };
    Line::from(vec![marker, " ".dim(), step.to_string().into()])
}

fn tool_activity_line(activity: &ToolActivity, width: usize) -> Line<'static> {
    let status = match activity.status.as_str() {
        "completed" => activity.status.clone().fg(palette::success()),
        "failed" | "declined" => activity.status.clone().fg(palette::error()),
        "in progress" | "inprogress" => activity.status.clone().fg(palette::cyan()),
        _ => activity.status.clone().dim(),
    };
    let prefix_width = activity.status.chars().count() + 1;
    Line::from(vec![
        status,
        " ".dim(),
        dashboard_value(&activity.title, width, prefix_width).into(),
    ])
}

fn activity_lines(
    activities: &VecDeque<ToolActivity>,
    width: usize,
    empty_label: &'static str,
) -> Vec<Line<'static>> {
    if activities.is_empty() {
        return vec![Line::from(empty_label.dim())];
    }

    activities
        .iter()
        .rev()
        .take(4)
        .rev()
        .map(|activity| tool_activity_line(activity, width))
        .collect()
}

fn approval_activity_lines(shell: &ShellState, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(pending) = &shell.pending_approval {
        lines.push(activity_status_line("approval", pending.title(), width));
    }
    if let Some(pending) = &shell.pending_elicitation {
        lines.push(activity_status_line("mcp", pending.title(), width));
    }
    if let Some(pending) = &shell.pending_user_input {
        lines.push(activity_status_line("input", pending.title(), width));
    }
    if lines.is_empty() {
        lines.push(Line::from("none pending".dim()));
    }
    lines
}

fn background_activity_lines(shell: &ShellState) -> Option<Vec<Line<'static>>> {
    let mut lines = Vec::new();
    if shell.workspace_status_refresh_due {
        lines.push(Line::from("workspace refresh queued".dim()));
    }
    (!lines.is_empty()).then_some(lines)
}

fn activity_status_line(label: &'static str, title: &str, width: usize) -> Line<'static> {
    let prefix_width = label.chars().count() + 1;
    Line::from(vec![
        label.fg(palette::cyan()),
        " ".dim(),
        dashboard_value(title, width, prefix_width).into(),
    ])
}

pub(super) fn format_i64(value: i64) -> String {
    if value < 0 {
        format!("-{}", format_u64(value.unsigned_abs()))
    } else {
        format_u64(value as u64)
    }
}

fn format_token_count(value: i64) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let value = value.unsigned_abs();
    let unit = if value >= 1_000_000_000_000 {
        Some((1_000_000_000_000, "t"))
    } else if value >= 1_000_000_000 {
        Some((1_000_000_000, "b"))
    } else if value >= 1_000_000 {
        Some((1_000_000, "m"))
    } else {
        None
    };
    if let Some((unit, suffix)) = unit {
        let tenth = unit / 10;
        let tenths = (value + tenth / 2) / tenth;
        let whole = tenths / 10;
        let decimal = tenths % 10;
        if decimal == 0 {
            format!("{sign}{whole}{suffix}")
        } else {
            format!("{sign}{whole}.{decimal}{suffix}")
        }
    } else if value >= 1_000 {
        format!("{sign}{}k", (value + 500) / 1_000)
    } else {
        format!("{sign}{value}")
    }
}

fn format_u64(value: u64) -> String {
    let text = value.to_string();
    let mut grouped = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
}

#[cfg(test)]
#[path = "dashboard_tests.rs"]
mod tests;
