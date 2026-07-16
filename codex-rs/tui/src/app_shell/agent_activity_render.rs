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
const MAX_FIELD_LINES: usize = 2;
const MAX_TIMELINE_LINES: usize = 4;

pub(super) fn agent_activity_overview_lines(
    state: &AgentActivityState,
    width: usize,
) -> Vec<Line<'static>> {
    let counts = state.counts();
    let mut spans = vec![
        "Agents ".fg(palette::TEXT).bold(),
        counts.total.to_string().fg(palette::PURPLE).bold(),
    ];
    if counts.total == 0 {
        spans.extend(["  ".into(), "no agents yet".fg(palette::MUTED)]);
    } else {
        let categorized = counts
            .active
            .saturating_add(counts.completed)
            .saturating_add(counts.interrupted)
            .saturating_add(counts.failed);
        for (glyph, count, label, color) in [
            ("●", counts.active, "active", palette::CYAN),
            ("✓", counts.completed, "done", palette::SUCCESS),
            ("!", counts.interrupted, "interrupted", palette::WARNING),
            ("×", counts.failed, "failed", palette::ERROR),
            (
                "?",
                counts.total.saturating_sub(categorized),
                "unknown",
                palette::MUTED,
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
) -> Vec<Line<'static>> {
    if line_budget == 0 {
        return Vec::new();
    }
    let agents = state.ordered_agents();
    if agents.is_empty() {
        return vec![truncate_line_with_ellipsis_if_overflow(
            Line::from("No agent activity yet".fg(palette::MUTED)),
            width,
        )];
    }

    let selected_id = state.selected_thread_id();
    let (start, end) = visible_agent_range(state, line_budget);
    let mut lines = agents[start..end]
        .iter()
        .map(|agent| agent_tree_line(agent, selected_id == Some(agent.thread_id.as_str()), width))
        .collect::<Vec<_>>();

    let Some(selected) = state.selected().or_else(|| agents.first().copied()) else {
        return lines;
    };
    if lines.len() < line_budget {
        lines.push(inspector_header(selected, width));
    }
    append_field(
        &mut lines,
        "Path",
        selected
            .path
            .as_ref()
            .map_or(selected.thread_id.as_str(), |path| path.as_str()),
        width,
        /*max_lines*/ 1,
        palette::MUTED,
        line_budget,
    );
    append_field(
        &mut lines,
        "Task",
        selected
            .task_summary
            .as_deref()
            .unwrap_or("No task summary"),
        width,
        MAX_FIELD_LINES,
        selected
            .task_summary
            .as_ref()
            .map_or(palette::MUTED, |_| palette::TEXT),
        line_budget,
    );
    let runtime = match (&selected.model, &selected.reasoning_effort) {
        (Some(model), Some(effort)) => format!("{model} · {effort} reasoning"),
        (Some(model), None) => model.clone(),
        (None, Some(effort)) => format!("{effort} reasoning"),
        (None, None) => "Default model and effort".to_string(),
    };
    append_field(
        &mut lines,
        "Runtime",
        &runtime,
        width,
        MAX_FIELD_LINES,
        palette::PURPLE,
        line_budget,
    );
    append_field(
        &mut lines,
        "Latest",
        selected
            .latest_message
            .as_deref()
            .unwrap_or("No message yet"),
        width,
        MAX_FIELD_LINES,
        selected
            .latest_message
            .as_ref()
            .map_or(palette::MUTED, |_| palette::TEXT),
        line_budget,
    );

    if !selected.timeline.is_empty() && lines.len() < line_budget {
        lines.push(Line::from("Recent".fg(palette::CYAN).bold()));
        for entry in selected.timeline.iter().rev().take(MAX_TIMELINE_LINES) {
            if lines.len() >= line_budget {
                break;
            }
            lines.push(truncate_line_with_ellipsis_if_overflow(
                Line::from(vec![
                    "  • ".fg(palette::BORDER),
                    entry.label().to_string().fg(palette::MUTED),
                ]),
                width,
            ));
        }
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
    if line >= end.saturating_sub(start) {
        return None;
    }
    agents
        .get(start.saturating_add(line))
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
        .min(line_budget.saturating_sub(/*rhs*/ 6).max(/*other*/ 1));
    let start = selected_index
        .saturating_sub(tree_limit / 2)
        .min(agents.len().saturating_sub(tree_limit));
    (start, start.saturating_add(tree_limit))
}

fn agent_tree_line(agent: &AgentActivity, selected: bool, width: usize) -> Line<'static> {
    let depth = agent
        .depth
        .unwrap_or(/*default*/ 1)
        .saturating_sub(/*rhs*/ 1)
        .min(MAX_TREE_DEPTH);
    let (glyph, color) = status_visual(agent.status);
    let mut line = Line::from(vec![
        if selected {
            "› ".fg(palette::FOCUS).bold()
        } else {
            "  ".into()
        },
        "  ".repeat(depth).into(),
        if depth > 0 {
            "└ ".fg(palette::BORDER)
        } else {
            "".into()
        },
        glyph.fg(color).bold(),
        " ".into(),
        agent.status.label().to_string().fg(color),
        "  ".into(),
        agent.display_name().to_string().fg(if selected {
            palette::TEXT
        } else {
            palette::MUTED
        }),
    ]);
    if selected {
        line = line.style(Style::new().bg(palette::ELEVATED));
    }
    truncate_line_with_ellipsis_if_overflow(line, width)
}

fn inspector_header(agent: &AgentActivity, width: usize) -> Line<'static> {
    let (glyph, color) = status_visual(agent.status);
    let mut spans = vec![
        "Inspector  ".fg(palette::CYAN).bold(),
        agent.display_name().to_string().fg(palette::TEXT).bold(),
        "  ".into(),
        glyph.fg(color).bold(),
        " ".into(),
        agent.status.label().to_string().fg(color),
    ];
    if width >= 60
        && let Some(latest_message) = &agent.latest_message
    {
        spans.extend([
            "  · ".fg(palette::BORDER),
            latest_message.clone().fg(palette::TEXT),
        ]);
    }
    truncate_line_with_ellipsis_if_overflow(Line::from(spans), width)
}

fn append_field(
    lines: &mut Vec<Line<'static>>,
    label: &'static str,
    value: &str,
    width: usize,
    max_lines: usize,
    color: Color,
    line_budget: usize,
) {
    if lines.len() >= line_budget || max_lines == 0 {
        return;
    }
    let prefix = format!("{label}  ");
    let body_width = width
        .saturating_sub(prefix.chars().count())
        .max(/*other*/ 1);
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let wrapped = textwrap::wrap(&normalized, textwrap::Options::new(body_width));
    for (index, part) in wrapped.into_iter().take(max_lines).enumerate() {
        if lines.len() >= line_budget {
            break;
        }
        lines.push(truncate_line_with_ellipsis_if_overflow(
            Line::from(vec![
                if index == 0 {
                    prefix.clone().fg(palette::CYAN).bold()
                } else {
                    " ".repeat(prefix.chars().count()).into()
                },
                part.into_owned().fg(color),
            ]),
            width,
        ));
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
        AgentLifecycleStatus::Unknown => ("?", palette::MUTED),
        AgentLifecycleStatus::PendingInit => ("○", palette::WARNING),
        AgentLifecycleStatus::Running => ("●", palette::CYAN),
        AgentLifecycleStatus::Interrupted => ("!", palette::WARNING),
        AgentLifecycleStatus::Completed => ("✓", palette::SUCCESS),
        AgentLifecycleStatus::Errored => ("×", palette::ERROR),
        AgentLifecycleStatus::Shutdown => ("■", palette::MUTED),
        AgentLifecycleStatus::NotFound => ("?", palette::ERROR),
    }
}

#[cfg(test)]
#[path = "agent_activity_render_tests.rs"]
mod tests;
