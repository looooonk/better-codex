use super::super::project_hub::Chat;
use super::super::project_hub::Group;
use super::super::project_hub::HubView;
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

#[tokio::test]
async fn project_details_and_move_menu_share_pointer_and_keyboard_rows() {
    let (mut shell, thread) = project_shell();
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
    let config = test_config().await;
    let mut backend = RecordingBackend::default();
    let area = Rect::new(0, 0, 116, 24);
    let template = shell.projects.groups[0].project.clone();
    for index in 1..12 {
        let mut project = template.clone();
        project.id = format!("project-{index}");
        project.name = format!("Project {index}");
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
        let group = &mut shell.projects.groups[0];
        for index in 2..=5 {
            group.chats.push(Chat {
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
