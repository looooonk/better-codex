use std::cell::Cell;
use std::collections::HashSet;

use codex_app_server_protocol::Project;
use codex_app_server_protocol::Thread;
use codex_protocol::ThreadId;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Styled;
use ratatui::style::Stylize;
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use super::dashboard::dashboard_value;
use super::design::palette;

pub(super) const PAGE_SIZE: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Previous,
    Next,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Action {
    Toggle(String),
    Details(String),
    New(String),
    Open(String, ThreadId),
    Move(ThreadId),
    Assign(ThreadId, Option<String>),
    Page(String, Direction),
    Back,
}

#[derive(Default)]
pub(super) enum HubView {
    #[default]
    Browse,
    Details(String),
    Move(ThreadId),
}

pub(super) struct Group {
    pub project: Project,
    pub chats: Vec<Chat>,
    pub next: Option<String>,
    pub pages: Vec<Option<String>>,
}

pub(super) struct Chat {
    pub id: ThreadId,
    pub title: String,
    pub cwd: std::path::PathBuf,
}

impl Chat {
    pub(super) fn from_thread(thread: Thread) -> Option<Self> {
        Some(Self {
            id: ThreadId::from_string(&thread.id).ok()?,
            title: thread
                .name
                .filter(|s| !s.trim().is_empty())
                .or_else(|| thread.preview.lines().next().map(str::to_owned))
                .unwrap_or_else(|| "Untitled chat".into()),
            cwd: thread.cwd.to_path_buf(),
        })
    }
}

#[derive(Default)]
pub(super) struct ProjectHub {
    pub groups: Vec<Group>,
    pub collapsed: HashSet<String>,
    pub view: HubView,
    pub selected: Option<usize>,
    pub reveal: Cell<bool>,
    pub error: Option<String>,
    pub width: Cell<usize>,
    pub wheel_page: bool,
    pub requested_page: Option<(String, Vec<Option<String>>)>,
    pub load_error: Option<String>,
    pub task: Option<tokio::task::JoinHandle<color_eyre::Result<Vec<Group>>>>,
}

pub(super) struct HubRow {
    pub line: Line<'static>,
    pub action: Option<Action>,
    pub more: Option<Action>,
}

fn color(project: &Project) -> Color {
    match project
        .metadata
        .get("appearance.color")
        .or_else(|| project.metadata.get("color"))
        .map(String::as_str)
    {
        Some("red") => Color::LightRed,
        Some("orange") => Color::Rgb(245, 153, 78),
        Some("yellow") => Color::Yellow,
        Some("green") => Color::Green,
        Some("teal" | "cyan") => Color::Cyan,
        Some("blue") => Color::LightBlue,
        Some("purple" | "violet") => Color::Rgb(180, 142, 240),
        Some("pink") => Color::Rgb(234, 144, 192),
        Some("gray" | "grey") => Color::Gray,
        Some(value) => value.parse().unwrap_or_else(|_| palette::text()),
        None => palette::text(),
    }
}

impl ProjectHub {
    pub fn group(&self, id: &str) -> Option<&Group> {
        self.groups.iter().find(|group| group.project.id == id)
    }

    pub fn page(&mut self, id: &str, direction: Direction) -> bool {
        let Some(group) = self.group(id) else {
            return false;
        };
        let mut pages = group.pages.clone();
        match direction {
            Direction::Next => {
                let Some(next) = &group.next else {
                    return false;
                };
                pages.push(Some(next.clone()));
            }
            Direction::Previous if pages.len() > 1 => {
                pages.pop();
            }
            Direction::Previous => return false,
        }
        self.requested_page = Some((id.to_owned(), pages));
        true
    }

    pub fn rows(&self, width: usize) -> Vec<HubRow> {
        let mut rows = Vec::new();
        let mut add =
            |title: String, style: Style, action: Option<Action>, more: Option<Action>| {
                let right = if more.is_some() { "  … " } else { "" };
                let title = dashboard_value(&title, width.saturating_sub(right.width()), 0);
                let space = if more.is_some() {
                    width.saturating_sub(title.width() + right.width())
                } else {
                    0
                };
                let mut line = Line::from(vec![
                    title.set_style(style),
                    " ".repeat(space).into(),
                    right.fg(palette::muted()),
                ]);
                if self.selected == Some(rows.len()) && action.is_some() {
                    line = line.bg(palette::surface()).bold();
                }
                rows.push(HubRow { line, action, more });
            };
        let muted = Style::new().fg(palette::muted());
        let normal = Style::new().fg(palette::text());
        if let Some(error) = self.error.as_ref().or(self.load_error.as_ref()) {
            for line in textwrap::wrap(error, width.max(1)) {
                add(
                    line.into_owned(),
                    Style::new().fg(palette::error()),
                    None,
                    None,
                );
            }
        }
        match &self.view {
            HubView::Browse => {
                add("Projects".into(), muted.bold(), None, None);
                for group in &self.groups {
                    let p = &group.project;
                    let collapsed = self.collapsed.contains(&p.id);
                    let arrow = if collapsed { "›" } else { "⌄" };
                    add(
                        format!("{arrow} {}", p.name),
                        Style::new().fg(color(p)).bold(),
                        Some(Action::Toggle(p.id.clone())),
                        Some(Action::Details(p.id.clone())),
                    );
                    if collapsed {
                        continue;
                    }
                    for chat in &group.chats {
                        add(
                            format!("    {}", chat.title),
                            normal,
                            Some(Action::Open(p.id.clone(), chat.id)),
                            Some(Action::Move(chat.id)),
                        );
                    }
                    if group.pages.len() > 1 {
                        add(
                            "    ↑ Newer chats".into(),
                            muted,
                            Some(Action::Page(p.id.clone(), Direction::Previous)),
                            None,
                        );
                    }
                    if group.next.is_some() {
                        add(
                            "    ↓ Older chats".into(),
                            muted,
                            Some(Action::Page(p.id.clone(), Direction::Next)),
                            None,
                        );
                    }
                    add(
                        "    + New chat".into(),
                        muted,
                        Some(Action::New(p.id.clone())),
                        None,
                    );
                }
                if self.groups.is_empty() && self.load_error.is_none() {
                    add(
                        if self.task.is_some() {
                            "Loading projects…"
                        } else {
                            "No projects"
                        }
                        .into(),
                        muted,
                        None,
                        None,
                    );
                }
            }
            HubView::Details(id) => {
                add("‹  Projects".into(), muted, Some(Action::Back), None);
                if let Some(group) = self.group(id) {
                    let project = &group.project;
                    add(
                        project.name.clone(),
                        Style::new().fg(color(project)).bold(),
                        None,
                        None,
                    );
                    for (index, root) in project.roots.iter().enumerate() {
                        if index == 0 {
                            add("Working directory".into(), muted, None, None);
                        }
                        if index == 1 {
                            add("Additional directories".into(), muted, None, None);
                        }
                        for line in textwrap::wrap(&root.path.display().to_string(), width.max(1)) {
                            add(line.into_owned(), normal, None, None);
                        }
                    }
                    if project.roots.is_empty() {
                        add(
                            "Uses the chat’s working directory".into(),
                            muted,
                            None,
                            None,
                        );
                    }
                    add(
                        "+ New chat".into(),
                        normal,
                        Some(Action::New(id.clone())),
                        None,
                    );
                }
            }
            HubView::Move(thread) => {
                add("‹  Cancel".into(), muted, Some(Action::Back), None);
                add("Move chat to".into(), muted.bold(), None, None);
                add(
                    "No project".into(),
                    normal,
                    Some(Action::Assign(*thread, None)),
                    None,
                );
                for group in &self.groups {
                    add(
                        group.project.name.clone(),
                        Style::new().fg(color(&group.project)),
                        Some(Action::Assign(*thread, Some(group.project.id.clone()))),
                        None,
                    );
                }
            }
        }
        rows
    }

    pub fn step(&mut self, direction: Direction) -> bool {
        let rows = self.rows(self.width.get().max(1));
        let selected = (0..rows.len()).filter(|i| rows[*i].action.is_some());
        self.selected = if direction == Direction::Next {
            selected
                .into_iter()
                .find(|i| self.selected.is_none_or(|s| *i > s))
        } else {
            selected
                .into_iter()
                .rev()
                .find(|i| self.selected.is_none_or(|s| *i < s))
        };
        self.reveal.set(true);
        self.selected.is_some()
    }
}
