use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use tokio_stream::StreamExt;

use crate::app_server_session::AppServerSession;
use crate::config_update::format_config_error;
use crate::hooks_rpc::HookTrustUpdate;
use crate::hooks_rpc::fetch_hooks_list;
use crate::hooks_rpc::hook_needs_review;
use crate::hooks_rpc::hooks_list_entry_for_cwd;
use crate::hooks_rpc::write_hook_trusts;
use crate::legacy_core::config::Config;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use codex_app_server_client::AppServerRequestHandle;
use codex_app_server_protocol::HooksListEntry;
use std::path::PathBuf;

const MOCHA_BASE: Color = Color::Rgb(30, 30, 46);
const MOCHA_MANTLE: Color = Color::Rgb(24, 24, 37);
const MOCHA_SURFACE0: Color = Color::Rgb(49, 50, 68);

pub(crate) enum StartupHooksReviewOutcome {
    Continue,
    OpenHooksBrowser(HooksListEntry),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StartupHooksReviewSelection {
    ReviewHooks,
    TrustAllAndContinue,
    ContinueWithoutTrusting,
}

impl StartupHooksReviewSelection {
    const ALL: [Self; 3] = [
        Self::ReviewHooks,
        Self::TrustAllAndContinue,
        Self::ContinueWithoutTrusting,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::ReviewHooks => "Review hooks",
            Self::TrustAllAndContinue => "Trust all and continue",
            Self::ContinueWithoutTrusting => "Continue without trusting",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::ReviewHooks => "Open the hooks browser before starting the app shell.",
            Self::TrustAllAndContinue => "Persist trust for every new or changed hook.",
            Self::ContinueWithoutTrusting => "Start the app shell with untrusted hooks disabled.",
        }
    }
}

#[derive(Clone, Debug)]
struct StartupHooksReviewState {
    entry: HooksListEntry,
    selected: usize,
    trust_all_error: Option<String>,
    trusting_all: bool,
}

impl StartupHooksReviewState {
    fn new(entry: HooksListEntry) -> Self {
        Self {
            entry,
            selected: 0,
            trust_all_error: None,
            trusting_all: false,
        }
    }

    fn selected(&self) -> StartupHooksReviewSelection {
        StartupHooksReviewSelection::ALL[self
            .selected
            .min(StartupHooksReviewSelection::ALL.len() - 1)]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(StartupHooksReviewSelection::ALL.len().saturating_sub(1));
    }

    fn select_number(&mut self, number: char) {
        self.selected = match number {
            '1' => 0,
            '2' => 1,
            '3' => 2,
            _ => self.selected,
        };
    }
}

pub(crate) async fn load_startup_hooks_review_entry(
    request_handle: AppServerRequestHandle,
    cwd: PathBuf,
) -> HooksListEntry {
    let response = match fetch_hooks_list(request_handle, cwd.clone()).await {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!("failed to load startup hook review state: {err:#}");
            return HooksListEntry {
                cwd,
                hooks: Vec::new(),
                warnings: Vec::new(),
                errors: Vec::new(),
            };
        }
    };
    hooks_list_entry_for_cwd(response, &cwd)
}

pub(crate) async fn maybe_run_startup_hooks_review(
    app_server: &mut AppServerSession,
    tui: &mut Tui,
    _config: &Config,
    bypass_hook_trust: bool,
    entry: HooksListEntry,
) -> Result<StartupHooksReviewOutcome> {
    if !review_is_needed(bypass_hook_trust, &entry) {
        return Ok(StartupHooksReviewOutcome::Continue);
    }

    run_startup_hooks_review_app(app_server, tui, entry).await
}

async fn run_startup_hooks_review_app(
    app_server: &mut AppServerSession,
    tui: &mut Tui,
    entry: HooksListEntry,
) -> Result<StartupHooksReviewOutcome> {
    tui.enter_alt_screen()
        .wrap_err("failed to enter startup hooks review screen")?;
    tui.frame_requester().schedule_frame();

    let mut state = StartupHooksReviewState::new(entry);
    let mut tui_events = tui.event_stream();

    loop {
        let Some(event) = tui_events.next().await else {
            return Ok(StartupHooksReviewOutcome::Continue);
        };
        match event {
            TuiEvent::Key(key) => match handle_startup_hooks_key(key, &mut state) {
                StartupHooksReviewKeyAction::Continue => match state.selected() {
                    StartupHooksReviewSelection::ReviewHooks => {
                        return Ok(StartupHooksReviewOutcome::OpenHooksBrowser(state.entry));
                    }
                    StartupHooksReviewSelection::ContinueWithoutTrusting => {
                        return Ok(StartupHooksReviewOutcome::Continue);
                    }
                    StartupHooksReviewSelection::TrustAllAndContinue => {
                        state.trusting_all = true;
                        tui.frame_requester().schedule_frame();
                        match persist_hook_trusts(app_server, &state.entry).await {
                            Ok(()) => return Ok(StartupHooksReviewOutcome::Continue),
                            Err(err) => {
                                state.trusting_all = false;
                                state.trust_all_error = Some(err);
                                tui.frame_requester().schedule_frame();
                            }
                        }
                    }
                },
                StartupHooksReviewKeyAction::Exit => {
                    return Ok(StartupHooksReviewOutcome::Continue);
                }
                StartupHooksReviewKeyAction::Redraw => {
                    tui.frame_requester().schedule_frame();
                }
                StartupHooksReviewKeyAction::Ignored => {}
            },
            TuiEvent::Paste(_) => {}
            TuiEvent::Resize | TuiEvent::Draw => draw_startup_hooks_review(tui, &state)?,
        }
    }
}

async fn persist_hook_trusts(
    app_server: &mut AppServerSession,
    entry: &HooksListEntry,
) -> std::result::Result<(), String> {
    write_hook_trusts(
        app_server.request_handle(),
        entry
            .hooks
            .iter()
            .filter(|hook| hook_needs_review(hook))
            .map(|hook| HookTrustUpdate {
                key: hook.key.clone(),
                current_hash: hook.current_hash.clone(),
            })
            .collect(),
    )
    .await
    .map(|_| ())
    .map_err(|err| format!("Failed to trust hooks: {}", format_config_error(&err)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupHooksReviewKeyAction {
    Continue,
    Exit,
    Redraw,
    Ignored,
}

fn handle_startup_hooks_key(
    key: KeyEvent,
    state: &mut StartupHooksReviewState,
) -> StartupHooksReviewKeyAction {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return StartupHooksReviewKeyAction::Ignored;
    }
    match key.code {
        KeyCode::Up => {
            state.move_up();
            StartupHooksReviewKeyAction::Redraw
        }
        KeyCode::Down => {
            state.move_down();
            StartupHooksReviewKeyAction::Redraw
        }
        KeyCode::Char(number @ ('1' | '2' | '3')) => {
            state.select_number(number);
            StartupHooksReviewKeyAction::Redraw
        }
        KeyCode::Enter => StartupHooksReviewKeyAction::Continue,
        KeyCode::Esc => StartupHooksReviewKeyAction::Exit,
        _ => StartupHooksReviewKeyAction::Ignored,
    }
}

fn review_needed_count(entry: &HooksListEntry) -> usize {
    entry
        .hooks
        .iter()
        .filter(|hook| hook_needs_review(hook))
        .count()
}

fn review_is_needed(bypass_hook_trust: bool, entry: &HooksListEntry) -> bool {
    !bypass_hook_trust && review_needed_count(entry) > 0
}

fn draw_startup_hooks_review(
    tui: &mut Tui,
    state: &StartupHooksReviewState,
) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        StartupHooksReviewView { state }.render(frame.area(), frame.buffer);
    })
}

struct StartupHooksReviewView<'a> {
    state: &'a StartupHooksReviewState,
}

impl StartupHooksReviewView<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        self.render_main(horizontal[0], buf);
        self.render_dashboard(horizontal[1], buf);
    }

    fn render_main(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_BASE);
        let vertical = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(5),
            ])
            .split(area);
        fill_rect(buf, vertical[0], MOCHA_MANTLE);
        Paragraph::new(Line::from("Better Codex".magenta().bold()))
            .style(pane_style(MOCHA_MANTLE))
            .render(pane_content_rect(vertical[0]), buf);

        let content = pane_content_rect(vertical[1]);
        let count = review_needed_count(&self.state.entry);
        let mut lines = vec![
            Line::from("Hooks need review".bold()),
            Line::from(""),
            hook_count_line(count),
            Line::from("Hooks can run outside the sandbox after you trust them.".dim()),
            Line::from(""),
        ];
        for (index, selection) in StartupHooksReviewSelection::ALL.into_iter().enumerate() {
            lines.push(selection_line(
                index,
                selection,
                index == self.state.selected,
            ));
            lines.extend(
                wrapped_lines_with_indent(
                    selection.description(),
                    usize::from(content.width),
                    "  ",
                )
                .into_iter()
                .map(ratatui::prelude::Stylize::dim),
            );
        }
        if let Some(error) = &self.state.trust_all_error {
            lines.push(Line::from(""));
            lines.extend(
                wrapped_lines(error, usize::from(content.width))
                    .into_iter()
                    .map(ratatui::prelude::Stylize::red),
            );
        } else if self.state.trusting_all {
            lines.push(Line::from(""));
            lines.push("Trusting hooks...".dim().into());
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_BASE))
            .render(content, buf);

        fill_rect(buf, vertical[2], MOCHA_SURFACE0);
        Paragraph::new(vec![
            Line::from("Enter continue  Up/Down choose  1/2/3 jump  Esc continue".dim()),
            Line::from("Hook trust decisions are written through app-server config.".dim()),
        ])
        .style(pane_style(MOCHA_SURFACE0))
        .render(pane_content_rect(vertical[2]), buf);
    }

    fn render_dashboard(&self, area: Rect, buf: &mut Buffer) {
        fill_rect(buf, area, MOCHA_SURFACE0);
        let content = pane_content_rect(area);
        let mut lines = vec![
            Line::from("Startup".bold()),
            Line::from(""),
            Line::from("workspace".dim()),
        ];
        lines.extend(wrapped_lines(
            &self.state.entry.cwd.display().to_string(),
            usize::from(content.width),
        ));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            "needs review ".dim(),
            review_needed_count(&self.state.entry)
                .to_string()
                .cyan()
                .bold(),
        ]));
        lines.push(Line::from(vec![
            "warnings ".dim(),
            self.state.entry.warnings.len().to_string().into(),
        ]));
        lines.push(Line::from(vec![
            "errors ".dim(),
            self.state.entry.errors.len().to_string().into(),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            "selected ".dim(),
            self.state.selected().label().cyan().bold(),
        ]));
        Paragraph::new(lines)
            .style(pane_style(MOCHA_SURFACE0))
            .render(content, buf);
    }
}

fn hook_count_line(count: usize) -> Line<'static> {
    match count {
        1 => "1 hook is new or changed.".magenta().into(),
        count => format!("{count} hooks are new or changed.")
            .magenta()
            .into(),
    }
}

fn selection_line(
    index: usize,
    selection: StartupHooksReviewSelection,
    selected: bool,
) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
    };
    let label = format!("{}. {}", index + 1, selection.label());
    Line::from(vec![marker, " ".dim(), label.into()])
}

fn fill_rect(buf: &mut Buffer, area: Rect, color: Color) {
    let style = pane_style(color);
    for y in area.y..area.y.saturating_add(area.height) {
        for x in area.x..area.x.saturating_add(area.width) {
            buf[(x, y)].set_symbol(" ").set_style(style);
        }
    }
}

fn pane_style(color: Color) -> Style {
    Style::new().bg(color)
}

fn pane_content_rect(area: Rect) -> Rect {
    let horizontal_padding = 1.min(area.width.saturating_sub(1) / 2);
    let vertical_padding = 1.min(area.height.saturating_sub(1) / 2);
    Rect::new(
        area.x.saturating_add(horizontal_padding),
        area.y.saturating_add(vertical_padding),
        area.width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        area.height
            .saturating_sub(vertical_padding.saturating_mul(2)),
    )
}

fn wrapped_lines(text: &str, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    textwrap::wrap(text, width)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

fn wrapped_lines_with_indent(text: &str, width: usize, indent: &'static str) -> Vec<Line<'static>> {
    let options = textwrap::Options::new(width.max(indent.len() + 1))
        .initial_indent(indent)
        .subsequent_indent(indent);
    textwrap::wrap(text, options)
        .into_iter()
        .map(|line| Line::from(line.into_owned()))
        .collect()
}

#[cfg(test)]
#[path = "startup_hooks_review_tests.rs"]
mod tests;
