use super::*;
use pretty_assertions::assert_eq;

fn row(id: &str, title: &str, status: &'static str) -> AgentRow {
    AgentRow {
        target: SessionTarget {
            thread_id: ThreadId::from_string(id).unwrap(),
            path: None,
        },
        title: title.to_string(),
        cwd: "/workspace/better-codex".to_string(),
        model: None,
        reasoning_effort: None,
        preview: "Review the current workspace".into(),
        status,
        updated_at: 0,
        can_accept_input: true,
    }
}

#[test]
fn refresh_preserves_selection_when_agents_reorder_or_disappear() {
    let first = "01987b77-33b8-76e3-9a7f-a367513be004";
    let second = "01987b77-33b8-76e3-9a7f-a367513be005";
    let mut state = Overview::default();
    state.replace(vec![
        row(first, "Review", "working"),
        row(second, "Tests", "idle"),
    ]);
    state.selected = 1;
    state.replace(vec![
        row(second, "Tests", "working"),
        row(first, "Review", "idle"),
    ]);
    assert_eq!(state.selected, 0);
    state.replace(vec![row(first, "Review", "idle")]);
    assert_eq!(state.selected, 0);
    state.replace(vec![]);
    assert_eq!(state.selected, 0);
}

#[test]
fn agents_overview_snapshot() {
    let state = Overview {
        rows: vec![
            row(
                "01987b77-33b8-76e3-9a7f-a367513be004",
                "Review upstream compatibility",
                "working",
            ),
            row(
                "01987b77-33b8-76e3-9a7f-a367513be005",
                "Run interface tests",
                "needs input",
            ),
        ],
        selected: 1,
        notice: None,
    };
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 92, /*height*/ 18,
    );
    let mut buffer = Buffer::empty(area);
    state.render(area, &mut buffer);
    let lines = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    insta::assert_snapshot!(lines.join("\n"));
}

#[derive(Clone, Default)]
struct Reader {
    pages: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<ThreadLoadedListResponse>>>,
    reads: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl AgentsReader for Reader {
    async fn loaded(
        &self,
        _: ThreadLoadedListParams,
    ) -> Result<ThreadLoadedListResponse, TypedRequestError> {
        Ok(self
            .pages
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected loaded-thread request"))
    }

    async fn thread(&self, thread_id: String) -> Result<ThreadReadResponse, TypedRequestError> {
        self.reads.lock().unwrap().push(thread_id.clone());
        Ok(serde_json::from_value(serde_json::json!({"thread": {
            "id": thread_id, "sessionId": "session", "preview": "Review", "ephemeral": false,
            "modelProvider": "openai", "createdAt": 0, "updatedAt": 0, "status": {"type":"idle"},
            "model": "gpt-6.1-sol", "reasoningEffort": "high",
            "cwd": "/workspace", "cliVersion": "0.160.0", "source": "cli", "turns": [], "projectId": null
        }})).unwrap())
    }
}

#[tokio::test]
async fn loaded_agent_pages_deduplicate_before_metadata_requests() {
    let first = "01987b77-33b8-76e3-9a7f-a367513be004";
    let second = "01987b77-33b8-76e3-9a7f-a367513be005";
    let reader = Reader::default();
    reader.pages.lock().unwrap().extend([
        ThreadLoadedListResponse {
            data: vec![first.to_string()],
            next_cursor: Some("next".to_string()),
        },
        ThreadLoadedListResponse {
            data: vec![first.to_string(), second.to_string()],
            next_cursor: None,
        },
    ]);
    let (rows, notice) = load_agents(reader.clone()).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| (row.model.as_deref(), row.reasoning_effort.as_ref()))
            .collect::<Vec<_>>(),
        vec![(Some("gpt-6.1-sol"), Some(&ReasoningEffort::High)); 2]
    );
    assert_eq!(notice, None);
    let mut reads = reader.reads.lock().unwrap().clone();
    reads.sort();
    assert_eq!(reads, [first, second]);
}

#[tokio::test]
async fn invalid_loaded_pages_fail_before_starting_metadata_requests() {
    for pages in [
        vec![ThreadLoadedListResponse {
            data: vec!["too many".to_string(); 101],
            next_cursor: None,
        }],
        vec![
            ThreadLoadedListResponse {
                data: vec![],
                next_cursor: Some("again".to_string()),
            },
            ThreadLoadedListResponse {
                data: vec![],
                next_cursor: Some("again".to_string()),
            },
        ],
    ] {
        let reader = Reader::default();
        reader.pages.lock().unwrap().extend(pages);
        assert!(load_agents(reader.clone()).await.is_err());
        assert!(reader.reads.lock().unwrap().is_empty());
    }
}

#[test]
fn long_model_and_effort_keep_project_and_prompt_visible() {
    let mut selected = row(
        "01987b77-33b8-76e3-9a7f-a367513be004",
        "Review long provider settings",
        "working",
    );
    selected.model = Some("provider/".repeat(12));
    selected.reasoning_effort = Some(ReasoningEffort::Custom("deliberate".repeat(12)));
    selected.cwd = "/workspace/a-long-project-path/".repeat(4);
    let state = Overview {
        rows: vec![selected],
        selected: 0,
        notice: None,
    };
    for width in [92, 48] {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 18);
        let mut buffer = Buffer::empty(area);
        state.render(area, &mut buffer);
        let rendered = buffer
            .content
            .chunks(usize::from(width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let fields = ["Model:", "Reasoning:", "Project:", "Prompt:"]
            .map(|label| rendered.find(label).expect("task detail is visible"));
        assert!(fields.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!rendered.contains(&"provider/".repeat(12)));
        assert!(!rendered.contains(&"deliberate".repeat(12)));
        insta::assert_snapshot!(format!("task_details_long_values_{width}"), rendered);
    }
}
