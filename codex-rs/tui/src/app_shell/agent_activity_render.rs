use super::agent_activity::AgentActivity;
use super::agent_activity::AgentActivityState;
use super::agent_activity::AgentLifecycleStatus;
use super::design::palette;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::wrapping::RtOptions;
use crate::wrapping::word_wrap_lines;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

const MAX_VISIBLE_AGENTS: usize = 8;
const MAX_TREE_DEPTH: usize = 5;
const AGENT_ROWS: usize = 2;

pub(super) fn agent_activity_overview_lines(
    state: &AgentActivityState,
    width: usize,
) -> Vec<Line<'static>> {
    let counts = state.counts();
    let mut spans = vec![
        "Agents ".fg(palette::text()).bold(),
        counts.total.to_string().fg(palette::purple()).bold(),
    ];
    if counts.total == 0 {
        spans.extend(["  ".into(), "no agents yet".fg(palette::muted())]);
    } else {
        let categorized = counts
            .active
            .saturating_add(counts.completed)
            .saturating_add(counts.interrupted)
            .saturating_add(counts.failed);
        for (glyph, count, label, color) in [
            ("●", counts.active, "active", palette::cyan()),
            ("✓", counts.completed, "done", palette::success()),
            ("!", counts.interrupted, "interrupted", palette::warning()),
            ("×", counts.failed, "failed", palette::error()),
            (
                "?",
                counts.total.saturating_sub(categorized),
                "unknown",
                palette::muted(),
            ),
        ] {
            push_count(&mut spans, glyph, count, label, color);
        }
    }
    word_wrap_lines(
        vec![Line::from(spans)],
        RtOptions::new(width.max(1)).subsequent_indent(Line::from("  ")),
    )
}

pub(super) fn agent_activity_inspector_lines(
    state: &AgentActivityState,
    width: usize,
    line_budget: usize,
    models: &[codex_protocol::openai_models::ModelPreset],
) -> Vec<Line<'static>> {
    if line_budget == 0 {
        return Vec::new();
    }
    let agents = state.ordered_agents();
    if agents.is_empty() {
        return vec![truncate_line_with_ellipsis_if_overflow(
            Line::from("No agent activity yet".fg(palette::muted())),
            width,
        )];
    }

    let selected_id = state.selected_thread_id();
    let (start, end) = visible_agent_range(state, line_budget);
    let mut lines = Vec::new();
    for agent in &agents[start..end] {
        let selected = selected_id == Some(agent.thread_id.as_str());
        lines.push(agent_tree_line(
            agent,
            selected,
            state.color(&agent.thread_id),
            width,
        ));
        let model = agent.model.as_deref().map(|slug| model_name(slug, models));
        let effort = agent
            .reasoning_effort
            .as_ref()
            .map(|e| format!("{} effort", e.as_str()));
        let model = [model, effort]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        let indent = tree_indent(agent) + 2;
        let value = if model.is_empty() {
            "Model unknown"
        } else {
            &model
        };
        lines.push(truncate_line_with_ellipsis_if_overflow(
            Line::from(format!("{:indent$}{value}", "").fg(palette::muted())),
            width,
        ));
    }
    lines.truncate(line_budget);
    lines
}

pub(super) fn agent_activity_thread_at_line(
    state: &AgentActivityState,
    line: usize,
    line_budget: usize,
) -> Option<&str> {
    let agents = state.ordered_agents();
    let (start, end) = visible_agent_range(state, line_budget);
    if line >= line_budget || line / AGENT_ROWS >= end.saturating_sub(start) {
        return None;
    }
    agents
        .get(start + line / AGENT_ROWS)
        .map(|agent| agent.thread_id.as_str())
}

fn visible_agent_range(state: &AgentActivityState, line_budget: usize) -> (usize, usize) {
    let agents = state.ordered_agents();
    let selected_index = state
        .selected_thread_id()
        .and_then(|selected| agents.iter().position(|agent| agent.thread_id == selected))
        .unwrap_or_default();
    let tree_limit = agents
        .len()
        .min(MAX_VISIBLE_AGENTS)
        .min((line_budget / AGENT_ROWS).max(1));
    let start = selected_index
        .saturating_sub(tree_limit / 2)
        .min(agents.len().saturating_sub(tree_limit));
    (start, start.saturating_add(tree_limit))
}

fn tree_indent(agent: &AgentActivity) -> usize {
    agent
        .depth
        .unwrap_or(1)
        .saturating_sub(1)
        .min(MAX_TREE_DEPTH)
        * 2
}

fn agent_tree_line(
    agent: &AgentActivity,
    selected: bool,
    color: Color,
    width: usize,
) -> Line<'static> {
    let active = matches!(
        agent.status,
        AgentLifecycleStatus::PendingInit | AgentLifecycleStatus::Running
    );
    let status_color = if active {
        palette::text()
    } else {
        palette::muted()
    };
    let mut line = Line::from(vec![
        if selected {
            "› ".fg(palette::focus())
        } else {
            "  ".into()
        },
        " ".repeat(tree_indent(agent)).into(),
        agent.display_name().to_string().bold().fg(color),
        "  ".into(),
        agent.status.label().fg(status_color),
    ]);
    if selected {
        line = line.style(Style::new().bg(palette::elevated()));
    }
    truncate_line_with_ellipsis_if_overflow(line, width)
}

fn model_name(slug: &str, models: &[codex_protocol::openai_models::ModelPreset]) -> String {
    let preset = models
        .iter()
        .find(|m| m.model == slug || m.id == slug)
        .or_else(|| {
            // A short family name can refer to several generations in the catalog.
            let mut matches = models.iter().filter(|m| {
                m.model
                    .rsplit('-')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case(slug))
            });
            let only = matches.next()?;
            matches.next().is_none().then_some(only)
        });
    let name = preset
        .map(|m| m.display_name.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(slug);
    let Some(suffix) = name
        .strip_prefix("GPT-")
        .or_else(|| name.strip_prefix("gpt-"))
    else {
        return name.into();
    };
    let Some((version, family)) = suffix.split_once('-') else {
        return format!("GPT-{suffix}");
    };
    let mut chars = family.chars();
    if version.starts_with(|c: char| c.is_ascii_digit())
        && let Some(first) = chars.next()
    {
        format!(
            "GPT-{version} {}{}",
            first.to_ascii_uppercase(),
            chars.as_str()
        )
    } else {
        name.into()
    }
}

fn push_count(
    spans: &mut Vec<Span<'static>>,
    glyph: &'static str,
    count: usize,
    label: &'static str,
    color: Color,
) {
    if count == 0 {
        return;
    }
    spans.extend([
        "  ".into(),
        glyph.fg(color).bold(),
        format!(" {count} {label}").fg(color),
    ]);
}

pub(super) fn status_visual(status: AgentLifecycleStatus) -> (&'static str, Color) {
    match status {
        AgentLifecycleStatus::Unknown => ("?", palette::muted()),
        AgentLifecycleStatus::PendingInit => ("○", palette::warning()),
        AgentLifecycleStatus::Running => ("●", palette::cyan()),
        AgentLifecycleStatus::Interrupted => ("!", palette::warning()),
        AgentLifecycleStatus::Completed => ("✓", palette::success()),
        AgentLifecycleStatus::Errored => ("×", palette::error()),
        AgentLifecycleStatus::Shutdown => ("■", palette::muted()),
        AgentLifecycleStatus::NotFound => ("?", palette::error()),
    }
}

#[cfg(test)]
#[path = "agent_activity_render_tests.rs"]
mod tests;
