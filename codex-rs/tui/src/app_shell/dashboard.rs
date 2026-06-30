use super::ShellState;
use super::ToolActivity;
use super::dashboard_rate_limits::rate_limit_lines;
use super::dashboard_workspace::workspace_lines;
use super::design::MOCHA_MANTLE;
use super::design::MOCHA_SURFACE0;
use super::design::Tone;
use super::design::badge_span;
use super::design::key_hint_line;
use super::design::tab_span;
use super::navigation::DashboardRoute;
use crate::goal_display::format_goal_elapsed_seconds;
use crate::goal_display::goal_status_label;
use crate::text_formatting::truncate_text;
use codex_app_server_protocol::TurnPlanStepStatus;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use std::collections::VecDeque;

pub(super) struct DashboardPanel {
    pub(super) title: String,
    pub(super) lines: Vec<Line<'static>>,
}

impl DashboardPanel {
    fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self {
            title: title.into(),
            lines,
        }
    }

    pub(super) fn height(&self) -> u16 {
        u16::try_from(self.lines.len().saturating_add(1)).unwrap_or(u16::MAX)
    }

    pub(super) fn background(&self, index: usize) -> Color {
        if index.is_multiple_of(2) {
            MOCHA_SURFACE0
        } else {
            MOCHA_MANTLE
        }
    }
}

pub(super) fn dashboard_panels(shell: &ShellState, width: usize) -> Vec<DashboardPanel> {
    let mut panels = vec![dashboard_navigation_panel(shell.dashboard_route)];
    panels.push(DashboardPanel::new(
        "Sessions",
        shell.session_list.lines(width),
    ));
    panels.push(DashboardPanel::new(
        "Settings",
        shell.settings.lines(&shell.settings_view(), width),
    ));
    panels.push(DashboardPanel::new(
        "Integrations",
        integration_lines(shell, width),
    ));
    let mut status_lines = vec![status_line(&shell.status)];
    if let Some(active_turn_id) = &shell.active_turn_id {
        status_lines.push(Line::from(vec![
            "turn ".dim(),
            short_id(active_turn_id).cyan(),
        ]));
    }
    panels.push(DashboardPanel::new("Status", status_lines));
    let thread_label = shell.thread_name.as_deref().unwrap_or("untitled thread");
    panels.push(DashboardPanel::new(
        "Thread",
        vec![
            Line::from(dashboard_value(
                thread_label,
                width,
                /*prefix_width*/ 0,
            )),
            Line::from(vec![
                "id ".dim(),
                dashboard_value(&shell.thread_id.to_string(), width, /*prefix_width*/ 3).cyan(),
            ]),
            Line::from("resume, fork, archive, delete in session list".dim()),
        ],
    ));
    let mut model_lines = vec![Line::from(dashboard_value(
        &shell.model,
        width,
        /*prefix_width*/ 0,
    ))];
    if let Some(reasoning_effort) = &shell.reasoning_effort {
        model_lines.push(Line::from(format!("reasoning {reasoning_effort}").dim()));
    }
    if let Some(service_tier) = shell
        .service_tier
        .as_deref()
        .filter(|service_tier| !service_tier.trim().is_empty())
    {
        model_lines.push(Line::from(vec![
            "tier ".dim(),
            dashboard_value(service_tier, width, /*prefix_width*/ 5).into(),
        ]));
    }
    panels.push(DashboardPanel::new("Model", model_lines));
    let token_lines = vec![
        Line::from(format!(
            "total {}",
            format_i64(shell.token_usage.total_tokens)
        )),
        Line::from(format!(
            "input {}",
            format_i64(shell.token_usage.input_tokens)
        )),
        Line::from(format!(
            "output {}",
            format_i64(shell.token_usage.output_tokens)
        )),
        match context_remaining_percent(&shell.token_usage, shell.model_context_window) {
            Some(percent) => Line::from(format!("Context {percent}% left")),
            None => Line::from("Context unknown".dim()),
        },
    ];
    panels.push(DashboardPanel::new("Tokens", token_lines));

    if shell.pending_approval.is_some()
        || shell.pending_elicitation.is_some()
        || shell.pending_user_input.is_some()
    {
        panels.push(DashboardPanel::new(
            "Approvals",
            approval_activity_lines(shell, width),
        ));
    }
    if let Some(background_lines) = background_activity_lines(shell) {
        panels.push(DashboardPanel::new("Background", background_lines));
    }

    if !shell.rate_limits.is_empty() || shell.rate_limit_reset_credits.is_some() {
        let mut limit_lines = Vec::new();
        for limit in shell.rate_limits.iter().take(2) {
            limit_lines.extend(rate_limit_lines(limit, width));
        }
        if shell.rate_limits.len() > 2 {
            limit_lines.push(Line::from(
                format!("+{} more", format_usize(shell.rate_limits.len() - 2)).dim(),
            ));
        }
        if let Some(credits) = shell.rate_limit_reset_credits {
            limit_lines.push(Line::from(
                format!("reset credits {}", format_i64(credits)).dim(),
            ));
        }
        panels.push(DashboardPanel::new("Rate Limits", limit_lines));
    }

    let diff_lines = if let Some(diff) = &shell.latest_diff {
        vec![Line::from(format!(
            "{} files +{} -{}",
            format_usize(diff.files),
            format_usize(diff.additions),
            format_usize(diff.removals)
        ))]
    } else {
        vec![Line::from("no changes".dim())]
    };
    panels.push(DashboardPanel::new("Diff", diff_lines));

    let mut plan_lines = Vec::new();
    if let Some(goal) = &shell.active_goal {
        plan_lines.push(Line::from(vec![
            "goal ".dim(),
            goal_status_span(goal.status),
        ]));
        plan_lines.push(Line::from(format!(
            "  {}",
            dashboard_value(&goal.objective, width, /*prefix_width*/ 2)
        )));
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
            plan_lines.push(Line::from(format!("  {}", usage.join(" | ")).dim()));
        }
    }
    if let Some(explanation) = &shell.plan_explanation {
        plan_lines.push(Line::from(explanation.clone().dim()));
    }
    if shell.plan_steps.is_empty() && shell.active_goal.is_none() {
        plan_lines.push(Line::from("no active plan".dim()));
    } else {
        for step in shell.plan_steps.iter().take(5) {
            plan_lines.push(plan_step_line(step.status, &step.step));
        }
    }
    panels.push(DashboardPanel::new("Plan", plan_lines));

    panels.push(DashboardPanel::new(
        "Tools",
        activity_lines(&shell.tool_activity, width, "idle"),
    ));
    if !shell.subagent_activity.is_empty() {
        panels.push(DashboardPanel::new(
            "Subagents",
            activity_lines(&shell.subagent_activity, width, "idle"),
        ));
    }

    panels.push(DashboardPanel::new(
        "Workspace",
        workspace_lines(shell, width),
    ));

    let key_lines = if shell.transcript_selection.is_some() {
        vec![
            key_hint_line("Up/Down select"),
            key_hint_line("Enter copy"),
            key_hint_line("Esc composer"),
        ]
    } else if shell.active_turn_id.is_some() {
        vec![
            key_hint_line("Enter steer"),
            key_hint_line("Ctrl+C interrupt, Esc exit"),
            key_hint_line("Alt+Up select, Ctrl+O copy"),
        ]
    } else {
        vec![
            key_hint_line("Enter send"),
            key_hint_line("Ctrl+C/Esc exit"),
            key_hint_line("Alt+Up select, Ctrl+O copy"),
        ]
    };
    panels.push(DashboardPanel::new("Keys", key_lines));

    route_dashboard_panels(shell.dashboard_route, panels)
}

pub(super) fn dashboard_value(text: &str, line_width: usize, prefix_width: usize) -> String {
    let max_chars = line_width.saturating_sub(prefix_width).max(1);
    truncate_text(text, max_chars)
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

fn route_dashboard_panels(
    route: DashboardRoute,
    panels: Vec<DashboardPanel>,
) -> Vec<DashboardPanel> {
    let titles: &[&str] = match route {
        DashboardRoute::Sessions => &[
            "Navigation",
            "Sessions",
            "Approvals",
            "Background",
            "Tools",
            "Subagents",
            "Thread",
            "Status",
            "Plan",
            "Keys",
        ],
        DashboardRoute::Workspace => &["Navigation", "Workspace", "Diff", "Tools", "Keys"],
        DashboardRoute::Settings => &[
            "Navigation",
            "Settings",
            "Model",
            "Tokens",
            "Integrations",
            "Rate Limits",
            "Workspace",
            "Keys",
        ],
        DashboardRoute::Help => &[
            "Navigation",
            "Keys",
            "Status",
            "Approvals",
            "Background",
            "Tools",
            "Subagents",
        ],
    };

    let mut panels = panels;
    titles
        .iter()
        .filter_map(|title| {
            let index = panels
                .iter()
                .position(|panel| panel.title.as_str() == *title)?;
            Some(panels.remove(index))
        })
        .collect()
}

fn dashboard_navigation_panel(active_route: DashboardRoute) -> DashboardPanel {
    let mut spans = Vec::new();
    for (index, route) in DashboardRoute::ALL.into_iter().enumerate() {
        if index > 0 {
            spans.push("  ".dim());
        }
        let label = format!("{}{}", index + 1, route.short_label());
        spans.push(tab_span(label, route == active_route));
    }
    spans.push("  ".dim());
    spans.push("Alt+Left/Right".dim());

    DashboardPanel::new("Navigation", vec![Line::from(spans)])
}

fn status_line(status: &str) -> Line<'static> {
    Line::from(status_span(status))
}

fn status_span(status: &str) -> Span<'static> {
    match status {
        "ready" => badge_span(status, Tone::Success),
        "failed" | "error" | "disconnected" => badge_span(status, Tone::Danger),
        "thinking" | "reasoning" | "retrying" => badge_span(status, Tone::Focus),
        "interrupted" => badge_span(status, Tone::Codex),
        _ => status.to_string().into(),
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
        TurnPlanStepStatus::InProgress => ">".cyan().bold(),
        TurnPlanStepStatus::Completed => "x".green(),
    };
    Line::from(vec![marker, " ".dim(), step.to_string().into()])
}

fn tool_activity_line(activity: &ToolActivity, width: usize) -> Line<'static> {
    let status = match activity.status.as_str() {
        "completed" => activity.status.clone().green(),
        "failed" | "declined" => activity.status.clone().red(),
        "in progress" | "inprogress" => activity.status.clone().cyan(),
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
        label.cyan(),
        " ".dim(),
        dashboard_value(title, width, prefix_width).into(),
    ])
}

fn integration_lines(shell: &ShellState, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(vec![
        "mcp ".dim(),
        dashboard_value(&shell.mcp_inventory.label(), width, /*prefix_width*/ 4).into(),
    ])];
    lines.push(Line::from(vec![
        "plugins ".dim(),
        dashboard_value(
            &shell.plugin_inventory.label(),
            width,
            /*prefix_width*/ 8,
        )
        .into(),
    ]));
    if shell.mcp_inventory.has_details() {
        lines.extend(shell.mcp_inventory.lines(width).into_iter().take(2));
    }
    if shell.plugin_inventory.has_details() {
        lines.extend(shell.plugin_inventory.lines(width).into_iter().take(2));
    }
    lines
}

fn short_id(id: &str) -> String {
    id.get(..8)
        .map(|prefix| format!("{prefix}..."))
        .unwrap_or_else(|| id.to_string())
}

pub(super) fn format_i64(value: i64) -> String {
    if value < 0 {
        format!("-{}", format_u64(value.unsigned_abs()))
    } else {
        format_u64(value as u64)
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
