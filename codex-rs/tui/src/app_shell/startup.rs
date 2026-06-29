use super::backend::AppShellBackend;
use super::design::MOCHA_BASE;
use super::design::MOCHA_MANTLE;
use super::design::MOCHA_SURFACE0;
use super::design::fill_rect;
use super::design::pane_content_rect;
use super::design::pane_style;
use crate::app_server_session::AppServerSession;
use crate::config_update::build_project_trust_level_edit;
use crate::legacy_core::config::Config;
use crate::tui;
use crate::tui::TuiEvent;
use codex_exec_server::LOCAL_FS;
use codex_git_utils::resolve_root_git_project_for_trust;
use codex_protocol::config_types::TrustLevel;
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use std::path::PathBuf;
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartupOnboardingOutcome {
    Continue { trust_persisted: bool },
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupSelection {
    Trust,
    ContinueUntrusted,
    Exit,
}

impl StartupSelection {
    const ALL: [Self; 3] = [Self::Trust, Self::ContinueUntrusted, Self::Exit];

    fn label(self) -> &'static str {
        match self {
            Self::Trust => "Trust workspace",
            Self::ContinueUntrusted => "Continue untrusted",
            Self::Exit => "Exit",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Trust => "Load project config, hooks, MCP, and workspace policies for this repo.",
            Self::ContinueUntrusted => {
                "Use safe defaults and keep project-local configuration disabled."
            }
            Self::Exit => "Return to the terminal without starting a thread.",
        }
    }

    fn trust_level(self) -> Option<TrustLevel> {
        match self {
            Self::Trust => Some(TrustLevel::Trusted),
            Self::ContinueUntrusted => Some(TrustLevel::Untrusted),
            Self::Exit => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupOnboardingState {
    cwd: PathBuf,
    trust_target: PathBuf,
    selected: usize,
    error: Option<String>,
}

impl StartupOnboardingState {
    fn new(cwd: PathBuf, trust_target: PathBuf) -> Self {
        Self {
            cwd,
            trust_target,
            selected: 0,
            error: None,
        }
    }

    fn selected(&self) -> StartupSelection {
        StartupSelection::ALL[self.selected.min(StartupSelection::ALL.len() - 1)]
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn move_down(&mut self) {
        self.selected = self
            .selected
            .saturating_add(1)
            .min(StartupSelection::ALL.len().saturating_sub(1));
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

pub(crate) async fn run_startup_onboarding(
    tui: &mut tui::Tui,
    app_server: &mut AppServerSession,
    config: &Config,
) -> Result<StartupOnboardingOutcome> {
    if config.active_project.trust_level.is_some() {
        return Ok(StartupOnboardingOutcome::Continue {
            trust_persisted: false,
        });
    }

    tui.enter_alt_screen()
        .wrap_err("failed to enter startup setup screen")?;
    tui.frame_requester().schedule_frame();

    let cwd = config.cwd.to_path_buf();
    let trust_target = resolve_root_git_project_for_trust(LOCAL_FS.as_ref(), &config.cwd)
        .await
        .map(Into::into)
        .unwrap_or_else(|| cwd.clone());
    let mut state = StartupOnboardingState::new(cwd, trust_target);
    let mut tui_events = tui.event_stream();

    loop {
        let Some(event) = tui_events.next().await else {
            return Ok(StartupOnboardingOutcome::Exit);
        };
        match event {
            TuiEvent::Key(key) => match handle_startup_key(key, &mut state) {
                StartupKeyAction::Continue => {
                    if let Some(trust_level) = state.selected().trust_level() {
                        match app_server
                            .write_config(vec![build_project_trust_level_edit(
                                &state.trust_target,
                                trust_level,
                            )])
                            .await
                        {
                            Ok(_) => {
                                return Ok(StartupOnboardingOutcome::Continue {
                                    trust_persisted: true,
                                });
                            }
                            Err(err) => {
                                state.error = Some(crate::config_update::format_config_error(&err));
                            }
                        }
                    } else {
                        return Ok(StartupOnboardingOutcome::Exit);
                    }
                }
                StartupKeyAction::Exit => return Ok(StartupOnboardingOutcome::Exit),
                StartupKeyAction::Redraw => {
                    tui.frame_requester().schedule_frame();
                }
                StartupKeyAction::Ignored => {}
            },
            TuiEvent::Paste(_) => {}
            TuiEvent::Resize | TuiEvent::Draw => {
                draw_startup_onboarding(tui, &state)?;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupKeyAction {
    Continue,
    Exit,
    Redraw,
    Ignored,
}

fn handle_startup_key(key: KeyEvent, state: &mut StartupOnboardingState) -> StartupKeyAction {
    if key.kind != KeyEventKind::Press {
        return StartupKeyAction::Ignored;
    }
    match key.code {
        KeyCode::Up => {
            state.move_up();
            StartupKeyAction::Redraw
        }
        KeyCode::Down => {
            state.move_down();
            StartupKeyAction::Redraw
        }
        KeyCode::Char(number @ ('1' | '2' | '3')) => {
            state.select_number(number);
            StartupKeyAction::Redraw
        }
        KeyCode::Enter => StartupKeyAction::Continue,
        KeyCode::Esc => StartupKeyAction::Exit,
        KeyCode::Char(_)
        | KeyCode::Backspace
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => StartupKeyAction::Ignored,
    }
}

fn draw_startup_onboarding(
    tui: &mut tui::Tui,
    state: &StartupOnboardingState,
) -> std::io::Result<()> {
    let height = tui.terminal.size()?.height;
    tui.draw(height, |frame| {
        StartupOnboardingView { state }.render(frame.area(), frame.buffer);
    })
}

struct StartupOnboardingView<'a> {
    state: &'a StartupOnboardingState,
}

impl StartupOnboardingView<'_> {
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
        let mut lines = vec![
            Line::from("First run setup".bold()),
            Line::from(""),
            Line::from("Choose how Better Codex should treat this workspace.".dim()),
            Line::from(""),
        ];
        lines.extend(wrapped_lines(
            "Trusting a workspace enables project-local config, hooks, MCP servers, and execution policies before the first thread starts.",
            usize::from(content.width),
        ));
        if self.state.cwd != self.state.trust_target {
            lines.push(Line::from(""));
            lines.extend(wrapped_lines(
                &format!(
                    "This directory is inside a Git repository; the decision applies to {}.",
                    self.state.trust_target.display()
                ),
                usize::from(content.width),
            ));
        }
        lines.push(Line::from(""));
        for (index, selection) in StartupSelection::ALL.into_iter().enumerate() {
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
        if let Some(error) = &self.state.error {
            lines.push(Line::from(""));
            lines.extend(
                wrapped_lines(error, usize::from(content.width))
                    .into_iter()
                    .map(ratatui::prelude::Stylize::red),
            );
        }
        Paragraph::new(lines)
            .style(pane_style(MOCHA_BASE))
            .render(content, buf);

        fill_rect(buf, vertical[2], MOCHA_SURFACE0);
        Paragraph::new(vec![
            Line::from("Enter continue  Up/Down choose  1/2/3 jump  Esc exit".dim()),
            Line::from("You can change this later from config.toml.".dim()),
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
            &self.state.cwd.display().to_string(),
            usize::from(content.width),
        ));
        lines.push(Line::from(""));
        lines.push(Line::from("trust target".dim()));
        lines.extend(wrapped_lines(
            &self.state.trust_target.display().to_string(),
            usize::from(content.width),
        ));
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

fn selection_line(index: usize, selection: StartupSelection, selected: bool) -> Line<'static> {
    let marker = if selected {
        ">".cyan().bold()
    } else {
        " ".dim()
    };
    let label = format!("{}. {}", index + 1, selection.label());
    Line::from(vec![marker, " ".dim(), label.into()])
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
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn startup_selection_keys_move_between_choices() {
        let mut state = StartupOnboardingState::new(
            PathBuf::from("/workspace/project"),
            PathBuf::from("/workspace/project"),
        );

        assert_eq!(
            handle_startup_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE), &mut state),
            StartupKeyAction::Redraw
        );
        assert_eq!(state.selected(), StartupSelection::ContinueUntrusted);

        assert_eq!(
            handle_startup_key(
                KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
                &mut state
            ),
            StartupKeyAction::Redraw
        );
        assert_eq!(state.selected(), StartupSelection::Exit);
    }

    #[test]
    fn startup_onboarding_view_renders_native_trust_choices() {
        let state = StartupOnboardingState::new(
            PathBuf::from("/workspace/project/crate"),
            PathBuf::from("/workspace/project"),
        );
        let backend = TestBackend::new(/*width*/ 100, /*height*/ 28);
        let mut terminal = Terminal::new(backend).expect("create terminal");

        terminal
            .draw(|frame| {
                StartupOnboardingView { state: &state }.render(frame.area(), frame.buffer_mut());
            })
            .expect("draw startup onboarding");
        insta::assert_snapshot!(terminal.backend().to_string());
    }
}
