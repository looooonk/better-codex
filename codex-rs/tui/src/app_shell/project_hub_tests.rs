use super::super::project_hub::Chat;
use super::super::project_hub::Group;
use super::super::project_hub::HubView;
use super::super::sessions::SessionListHit;
use super::*;
use codex_app_server_protocol::Project;
use codex_app_server_protocol::ProjectRoot;
use pretty_assertions::assert_eq;

fn project_shell() -> (ShellState, ThreadId) {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    shell.composer.clear();
    let thread = test_thread_id("01900000-0000-7000-8000-000000000011");
    let root = test_absolute_path("research");
    let old = thread_fixture(thread, Some("Earlier experiment"), "Existing Desktop chat");
    shell.session_list.replace_threads(vec![
        old.clone(),
        thread_fixture(
            test_thread_id("01900000-0000-7000-8000-000000000012"),
            Some("Loose notes"),
            "CLI chat",
        ),
    ]);
    shell.projects.groups.push(Group {
        project: Project {
            id: "research".into(),
            name: "Research".into(),
            roots: vec![
                ProjectRoot { path: root },
                ProjectRoot {
                    path: test_absolute_path("datasets"),
                },
            ],
            metadata: [("appearance.color".into(), "purple".into())].into(),
            position: 0,
            created_at: 0,
            updated_at: 0,
            recency_at: None,
        },
        chats: vec![Chat::from_thread(old).unwrap()],
        next: Some("next".into()),
        pages: vec![None],
    });
    (shell, thread)
}

#[test]
fn active_chat_has_hover_background_in_both_lists_without_focus() {
    let (mut shell, id) = project_shell();
    shell.thread_id = id;
    shell.session_list.focused = false;
    shell.projects.expanded.insert("research".into());
    let area = Rect::new(0, 0, 116, 36);
    let buf = render_shell_buffer(&shell, area);
    let mut highlighted = Vec::new();
    for y in 0..area.height {
        if let Some(x) = row_needle_x(&buf, area, y, "Earlier experiment") {
            highlighted.push(buf[(x, y)].bg);
        }
    }
    assert_eq!(highlighted, vec![design::palette::border(); 2]);
    insta::assert_debug_snapshot!(highlighted);
}

#[test]
fn empty_active_chat_survives_catalog_refresh_until_its_first_message() {
    for project in [None, Some("research".to_owned())] {
        let (mut shell, _) = project_shell();
        shell.projects.expanded.insert("research".into());
        shell.thread_name = None;
        shell.show_new_session(project.clone());
        shell.session_list.replace_threads(Vec::new());
        let area = Rect::new(0, 0, 116, 36);
        let buf = render_shell_buffer(&shell, area);
        let rows: Vec<_> = (0..area.height)
            .filter_map(|y| row_needle_x(&buf, area, y, "Untitled chat").map(|x| buf[(x,y)].bg))
            .collect();
        assert_eq!(rows, vec![design::palette::border(); 1 + usize::from(project.is_some())]);
        shell.session_list.replace_threads(vec![thread_fixture(shell.thread_id, Some("First message"), "")]);
        assert_eq!(shell.session_list.row_count(), 1);
        assert_eq!(shell.session_list.selected_title(), Some("First message"));
        shell.clear_new_session();
    }
}

#[tokio::test]
async fn project_details_and_move_menu_share_pointer_and_keyboard_rows() {
    let (mut shell, thread) = project_shell();
    shell.projects.expanded.insert("research".into());
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 116, 36);
    insta::assert_snapshot!("sessions_projects", render_shell(&shell, area));
    let buf = render_shell_buffer(&shell, area);
    let y = row_containing(&buf, area, "⌄ Research").unwrap();
    let name = row_needle_x(&buf, area, y, "Research").unwrap();
    let purple = crate::terminal_palette::best_color((180, 142, 240));
    let expected = match purple {
        ratatui::style::Color::Reset => ratatui::style::Color::Magenta,
        color => color,
    };
    assert_eq!(buf[(name, y)].fg, expected);
    let x = row_needle_x(&buf, area, y, "…").unwrap();
    shell
        .handle_mouse_click(area, Position::new(x, y), &config, &mut backend)
        .await
        .unwrap();
    assert!(matches!(shell.projects.view,HubView::Details(ref id) if id=="research"));
    insta::assert_snapshot!("sessions_project_details", render_shell(&shell, area));
    shell
        .handle_session_list_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &config,
            &mut backend,
        )
        .await
        .unwrap();
    let buf = render_shell_buffer(&shell, area);
    let y = row_containing(&buf, area, "Earlier experiment").unwrap();
    let x = row_needle_x(&buf, area, y, "…").unwrap();
    shell
        .handle_mouse_click(area, Position::new(x, y), &config, &mut backend)
        .await
        .unwrap();
    assert!(matches!(shell.projects.view,HubView::Move(id) if id == thread));
    insta::assert_snapshot!("sessions_move_chat", render_shell(&shell, area));
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn selecting_new_project_chat_uses_both_roots_and_selecting_old_chat_resumes_its_id() {
    let (mut shell, thread) = project_shell();
    shell.projects.expanded.insert("research".into());
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 116, 36);
    let buf = render_shell_buffer(&shell, area);
    let y = row_containing(&buf, area, "+ New chat").unwrap();
    let x = row_needle_x(&buf, area, y, "+ New chat").unwrap();
    shell
        .handle_mouse_click(area, Position::new(x, y), &config, &mut backend)
        .await
        .unwrap();
    complete_backend_actions(&mut shell, &backend).await;
    {
        let configs = backend.start_configs.lock().unwrap();
        assert_eq!(configs[0].cwd, test_absolute_path("research"));
        assert_eq!(
            configs[0].workspace_roots,
            vec![
                test_absolute_path("research"),
                test_absolute_path("datasets")
            ]
        );
    }
    shell
        .activate_project(
            super::super::project_hub::Action::Open("research".into(), thread),
            &config,
            &mut backend,
        )
        .await;
    for _ in 0..100 {
        shell.poll_backend_actions(&backend).await;
        if shell.thread_id == thread {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(shell.thread_id, thread);
}

#[tokio::test]
async fn keyboard_navigation_keeps_recent_chats_visible_below_expanded_projects() {
    let (mut shell, _) = project_shell();
    shell.projects.expanded.insert("research".into());
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 116, 24);
    let template = shell.projects.groups[0].project.clone();
    for index in 1..12 {
        let mut project = template.clone();
        project.id = format!("project-{index}");
        project.name = format!("Project {index}");
        shell.projects.expanded.insert(project.id.clone());
        shell.projects.groups.push(Group {
            project,
            chats: Vec::new(),
            next: None,
            pages: vec![None],
        });
    }
    shell.project_menu(HubView::Browse);
    for code in [KeyCode::Char('p'), KeyCode::Down] {
        shell
            .handle_session_list_key(
                KeyEvent::new(code, KeyModifiers::NONE),
                &config,
                &mut backend,
            )
            .await
            .unwrap();
        let _ = render_shell_buffer(&shell, area);
    }
    assert_eq!(shell.session_list.selected_title(), Some("Loose notes"));
    let rendered = render_shell(&shell, area);
    assert!(rendered.contains("› Loose notes"));
    insta::assert_snapshot!("sessions_recents_below_projects", rendered);
}

#[test]
fn wheel_over_project_chats_scrolls_the_dashboard_without_requesting_a_page() {
    for width in [116, 78] {
        let (mut shell, _) = project_shell();
        shell.projects.expanded.insert("research".into());
        let group = &mut shell.projects.groups[0];
        for index in 2..=5 {
            group.chats.push(Chat {
                external_writer: false,
                id: ThreadId::default(),
                title: format!("Experiment {index}"),
                cwd: group.project.roots[0].path.to_path_buf(),
            });
        }
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 14);
        let buffer = render_shell_buffer(&shell, area);
        let y = row_containing(&buffer, area, "Earlier experiment").unwrap();
        let x = row_needle_x(&buffer, area, y, "Earlier experiment").unwrap();
        let pointer = Position::new(x, y);

        shell.handle_mouse_scroll(area, pointer, tui::MouseScrollDirection::Down);
        assert!(shell.dashboard_scroll.get() > 0);
        assert_eq!(shell.projects.requested_page, None);
        if width == 116 {
            insta::assert_snapshot!("wheel_over_project_chats", render_shell(&shell, area));
        }
        shell.handle_mouse_scroll(area, pointer, tui::MouseScrollDirection::Up);
        assert_eq!(shell.dashboard_scroll.get(), 0);
        assert_eq!(shell.projects.requested_page, None);
    }
}

#[tokio::test]
async fn project_and_chat_buttons_have_hover_targets_separate_from_the_row() {
    let config = test_config().await;
    for width in [116, 78] {
        let (mut shell, thread) = project_shell();
        let mut backend = RecordingBackend::default();
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 34);
        let initial = render_shell(&shell, area);
        assert!(initial.contains("› Research"));
        assert!(!initial.contains("+ New chat"));
        if width == 116 {
            insta::assert_snapshot!("sessions_collapsed_projects", initial);
        }
        // Check the project, a chat in Recents, and a chat inside an expanded project.
        for title in ["› Research", "Loose notes", "Earlier experiment"] {
            shell.clear_pointer_position();
            let buf = render_shell_buffer(&shell, area);
            let y = row_containing(&buf, area, title).unwrap();
            let name = row_needle_x(&buf, area, y, title).unwrap();
            let dots = row_needle_x(&buf, area, y, "…").unwrap();
            let original = (buf[(name, y)].bg, buf[(dots, y)].bg);
            shell.set_pointer_position(Position::new(name, y));
            let buf = render_shell_buffer(&shell, area);
            assert_eq!(
                (buf[(name, y)].bg, buf[(dots, y)].bg),
                (design::palette::border(), original.1)
            );
            shell.set_pointer_position(Position::new(dots, y));
            let buf = render_shell_buffer(&shell, area);
            assert_eq!(
                (
                    buf[(name, y)].bg,
                    buf[(dots - 2, y)].bg,
                    buf[(dots - 1, y)].bg,
                    buf[(dots + 1, y)].bg
                ),
                (
                    original.0,
                    original.0,
                    design::palette::border(),
                    design::palette::border()
                )
            );
            if title == "› Research" && width == 116 {
                let mut strip = Buffer::empty(Rect::new(name, y, dots + 2 - name, 1));
                for x in name..strip.area.right() {
                    strip[(x, y)] = buf[(x, y)].clone();
                }
                insta::assert_snapshot!(
                    "project_more_hover",
                    crate::test_support::buffer_style_grid(&strip)
                );
            }
            shell
                .handle_mouse_click(area, Position::new(dots, y), &config, &mut backend)
                .await
                .unwrap();
            if title == "› Research" {
                assert!(matches!(shell.projects.view, HubView::Details(_)));
                assert!(shell.projects.expanded.is_empty());
            } else {
                assert!(matches!(shell.projects.view, HubView::Move(_)));
            }
            shell
                .handle_session_list_key(
                    KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                    &config,
                    &mut backend,
                )
                .await
                .unwrap();
            if title == "› Research" {
                shell
                    .handle_mouse_click(area, Position::new(name, y), &config, &mut backend)
                    .await
                    .unwrap();
                assert!(render_shell(&shell, area).contains("⌄ Research"));
            }
        }
        assert_ne!(shell.thread_id, thread);
        assert!(backend.calls().is_empty());
    }
}

#[tokio::test]
async fn recents_fills_the_resized_dashboard_across_pages_and_keeps_selection() {
    let (mut shell, _) = project_shell();
    let threads = (1..=80)
        .map(|index| {
            thread_fixture(
                test_thread_id(&format!("01900000-0000-7000-8001-{index:012x}")),
                Some(&format!("Recent {index:02}")),
                "chat",
            )
        })
        .collect();
    let backend = RecordingBackend::with_threads(threads);
    refresh_session_list(&mut shell, &backend).await;
    let short = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 116, /*height*/ 18,
    );
    let tall = Rect {
        height: 54,
        ..short
    };
    let rendered = render_shell(&shell, short);
    let count = rendered.lines().filter(|l| l.contains("Recent ")).count();
    assert!(count > 5 && count < 20);
    for _ in 0..2 {
        render_shell(&shell, tall);
        shell.fill_session_viewport(&backend);
        finish_session_hydration(&mut shell, &backend).await;
    }
    assert_eq!(shell.session_list.selected_title(), Some("Recent 01"));
    let tall_count = render_shell(&shell, tall)
        .lines()
        .filter(|l| l.contains("Recent "))
        .count();
    assert_eq!(tall_count, 20);
    let buf = render_shell_buffer(&shell, short);
    let y = row_containing(&buf, short, "Recent 01").unwrap();
    let x = row_needle_x(&buf, short, y, "Recent 01").unwrap();
    shell.handle_mouse_scroll(short, Position::new(x, y), tui::MouseScrollDirection::Down);
    let scrolled = render_shell(&shell, short);
    assert!(scrolled.contains("› Research"));
    assert!(!scrolled.contains("Recent 01"));
    assert!(scrolled.contains(&format!("Recent {:02}", count + 1)));
    let hit = ShellView { shell: &shell }
        .dashboard_panel_position_at(short, Position::new(x, y), "Sessions")
        .unwrap();
    assert_eq!(
        shell.session_list.hit_at_line(hit.line),
        Some(SessionListHit::Thread(test_thread_id(
            "01900000-0000-7000-8001-000000000002"
        )))
    );
}

#[tokio::test]
async fn down_at_the_page_boundary_uses_an_in_flight_viewport_fill() {
    let threads = (1..=30)
        .map(|index| {
            thread_fixture(
                test_thread_id(&format!("01900000-0000-7000-8001-{index:012x}")),
                Some(&format!("Chat {index}")),
                "chat",
            )
        })
        .collect();
    let backend = RecordingBackend::with_threads(threads);
    let (mut shell, _) = project_shell();
    refresh_session_list(&mut shell, &backend).await;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 116, /*height*/ 36,
    );
    render_shell(&shell, area);
    shell.fill_session_viewport(&backend);
    while shell.session_list.move_selection_down() {}
    shell.start_session_list_next_page(&backend);
    finish_session_hydration(&mut shell, &backend).await;
    assert_eq!(shell.session_list.selected_title(), Some("Chat 21"));
    assert_eq!(backend.calls().len(), 2);
}

#[tokio::test]
async fn blocked_chat_is_yellow_in_both_lists_and_reports_only_in_the_dashboard() {
    let (mut shell, thread) = project_shell();
    let config = test_config().await;
    let backend = RecordingBackend::default();
    let mut record = thread_fixture(thread, Some("Earlier experiment"), "Desktop");
    record.extra = Some(codex_app_server_protocol::ThreadExtra {
        external_writer: true,
    });
    shell.projects.groups[0].chats = vec![Chat::from_thread(record.clone()).unwrap()];
    shell.session_list.replace_threads(vec![record]);
    shell.projects.expanded.insert("research".into());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 116, /*height*/ 30,
    );
    let buf = render_shell_buffer(&shell, area);
    let colors: Vec<_> = (0..area.height)
        .filter_map(|y| row_needle_x(&buf, area, y, "Earlier experiment").map(|x| buf[(x, y)].fg))
        .collect();
    assert_eq!(colors, vec![design::palette::warning(); 2]);
    let before = shell.transcript.clone();
    backend.fail_next_action("thread already has an active writer");
    shell.resume_session(&config, &backend, thread).await;
    let loading = render_shell(&shell, area);
    assert!(loading.contains("Loading session..."));
    insta::assert_snapshot!("loading_selected_session", loading);
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!(shell.transcript, before);
    assert!(
        shell
            .projects
            .error
            .as_deref()
            .unwrap()
            .contains("another Codex session")
    );
    shell.toggle_dashboard();
    assert_eq!(shell.projects.error, None);
    shell.toggle_dashboard();
    shell.report_resume_error(thread, &color_eyre::eyre::eyre!("active writer"));
    shell.set_dashboard_route(DashboardRoute::Agents);
    assert_eq!(shell.projects.error, None);
    shell.set_dashboard_route(DashboardRoute::Sessions);
    shell.report_resume_error(thread, &color_eyre::eyre::eyre!("active writer"));
    shell.resume_session(&config, &backend, thread).await;
    complete_backend_actions(&mut shell, &backend).await;
    assert_eq!((shell.thread_id, shell.projects.error), (thread, None));
}

#[tokio::test]
async fn short_backend_pages_are_combined_before_recents_is_published() {
    let (shell, _) = project_shell();
    let params = shell.session_list.list_params_with_cursor(None);
    let page = super::super::sessions::load_page(params, |params| async move {
        let first: usize = params.cursor.as_deref().unwrap_or("0").parse()?;
        let end = first + usize::min(params.limit.unwrap() as usize, 6);
        Ok(ThreadListResponse {
            data: (first..end)
                .map(|i| {
                    thread_fixture(
                        test_thread_id(&format!("01900000-0000-7000-8001-{i:012x}")),
                        Some(&format!("Chat {i}")),
                        "Desktop",
                    )
                })
                .collect(),
            next_cursor: Some(end.to_string()),
            read_states: None,
            backwards_cursor: None,
        })
    })
    .await
    .unwrap();
    assert_eq!(
        (page.data.len(), page.next_cursor.as_deref()),
        (20, Some("20"))
    );
}

#[test]
fn importing_desktop_reasoning_keeps_the_user_prompt_and_answer() {
    let (mut shell, _) = project_shell();
    shell.clear_visible_transcript();
    let mut turn = test_turn("desktop", TurnStatus::Completed);
    turn.items = vec![
        ThreadItem::untimed(ThreadItemPayload::UserMessage {
            id: "question".into(),
            client_id: None,
            content: vec![UserInput::Text {
                text: "What changed?\r\n".into(),
                text_elements: Vec::new(),
            }],
        }),
        ThreadItem::untimed(ThreadItemPayload::Reasoning {
            id: "reasoning".into(),
            summary: vec!["**Unifying selected-line rendering**".into()],
            content: vec![],
        }),
        ThreadItem::untimed(ThreadItemPayload::AgentMessage {
            id: "answer".into(),
            text: "The rows now align.".into(),
            phase: None,
            memory_citation: None,
            questions: None,
            delivery: None,
        }),
    ];
    shell.ingest_turn_history(vec![turn]);
    let kinds: Vec<_> = shell.transcript.iter().map(|l| l.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TranscriptKind::User,
            TranscriptKind::Assistant,
            TranscriptKind::Separator
        ]
    );
}
