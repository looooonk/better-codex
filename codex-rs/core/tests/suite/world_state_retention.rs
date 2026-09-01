use anyhow::Result;
use codex_config::ConfigLayerStack;
use codex_extension_api::ExtensionRegistryBuilder;
use codex_history::CompactedItem;
use codex_history::RolloutItem;
use codex_history::RolloutLine;
use codex_login::CodexAuth;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::SKILLS_INSTRUCTIONS_OPEN_TAG;
use codex_skills_extension::SkillsExtensionConfig;
use core_test_support::PathBufExt;
use core_test_support::responses::ResponsesRequest;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;
use toml::toml;
use wiremock::MockServer;

fn write_skill(workspace: &Path, description: &str) -> std::io::Result<()> {
    let skill_dir = workspace.join(".agents/skills/demo");
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: demo\ndescription: {description}\n---\n\n# body\n"),
    )
}

fn host_catalog(request: &ResponsesRequest, description: &str) -> String {
    request
        .message_input_texts("developer")
        .into_iter()
        .find(|text| {
            text.starts_with(SKILLS_INSTRUCTIONS_OPEN_TAG)
                && text.contains("(file:")
                && text.contains(description)
        })
        .unwrap_or_else(|| panic!("missing {description} host skills catalog"))
}

fn append_compacted_catalog_history(
    rollout_path: &Path,
    catalog_a: &str,
    catalog_b: &str,
) -> Result<()> {
    let rollout_text = std::fs::read_to_string(rollout_path)?;
    let rollout_lines = rollout_text
        .lines()
        .map(serde_json::from_str::<RolloutLine>)
        .collect::<Result<Vec<_>, _>>()?;
    let world_state_start = rollout_lines
        .iter()
        .rposition(
            |line| matches!(&line.item, RolloutItem::WorldState(world_state) if world_state.full),
        )
        .expect("full world-state snapshot");
    let world_state_items = rollout_lines[world_state_start..]
        .iter()
        .filter_map(|line| {
            let RolloutItem::WorldState(world_state) = &line.item else {
                return None;
            };
            Some(RolloutItem::WorldState(world_state.clone()))
        })
        .collect::<Vec<_>>();
    let turn_context = rollout_lines
        .iter()
        .rev()
        .find_map(|line| {
            let RolloutItem::TurnContext(turn_context) = &line.item else {
                return None;
            };
            Some(turn_context.clone())
        })
        .expect("latest turn context");
    let replacement_history = vec![
        ResponseItem::Message {
            id: None,
            role: "developer".to_string(),
            content: vec![
                ContentItem::InputText {
                    text: catalog_a.to_string(),
                },
                ContentItem::InputText {
                    text: catalog_b.to_string(),
                },
            ],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }
        .into(),
    ];
    let next_ordinal = rollout_lines
        .iter()
        .filter_map(|line| line.ordinal)
        .max()
        .unwrap_or(rollout_lines.len() as u64)
        + 1;
    let mut appended_items = vec![RolloutItem::Compacted(CompactedItem {
        message: "compacted skills history".to_string(),
        replacement_history: Some(replacement_history),
        window_number: Some(1),
        first_window_id: None,
        previous_window_id: None,
        window_id: None,
    })];
    appended_items.extend(world_state_items);
    appended_items.push(RolloutItem::TurnContext(turn_context));
    let mut rollout_file = OpenOptions::new().append(true).open(rollout_path)?;
    for (offset, item) in appended_items.into_iter().enumerate() {
        let line = RolloutLine {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            ordinal: Some(next_ordinal + offset as u64),
            item,
        };
        writeln!(rollout_file, "{}", serde_json::to_string(&line)?)?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compacted_resume_reasserts_latest_host_skills_catalog() -> Result<()> {
    skip_if_no_network!(Ok(()));
    let server = MockServer::start().await;
    let req1 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp1"), ev_completed("resp1")]),
    )
    .await;
    let req2 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp2"), ev_completed("resp2")]),
    )
    .await;
    let req3 = mount_sse_once(
        &server,
        sse(vec![ev_response_created("resp3"), ev_completed("resp3")]),
    )
    .await;

    let codex_home_a = Arc::new(TempDir::new()?);
    let codex_home_b = Arc::new(TempDir::new()?);
    let mut extension_builder = ExtensionRegistryBuilder::new();
    codex_skills_extension::install(
        &mut extension_builder,
        |_config: &codex_core::config::Config| SkillsExtensionConfig {
            include_instructions: true,
            bundled_skills_enabled: false,
            orchestrator_skills_enabled: false,
        },
    );
    let extensions = Arc::new(extension_builder.build());
    let workspace_a = codex_home_a.path().join("workspace-a");
    let workspace_b = codex_home_b.path().join("workspace-b");
    write_skill(&workspace_a, "catalog a")?;
    write_skill(&workspace_b, "catalog b")?;
    let workspace_a = workspace_a.abs();
    let workspace_b = workspace_b.abs();

    let initial_cwd = workspace_a.clone();
    let user_config_path_a = codex_home_a.path().join("config.toml").abs();
    let mut initial_builder = test_codex()
        .with_home(Arc::clone(&codex_home_a))
        .with_auth(CodexAuth::from_api_key("Test API Key"))
        .with_extensions(Arc::clone(&extensions))
        .with_config(move |config| {
            config.cwd = initial_cwd.clone();
            config.config_layer_stack = ConfigLayerStack::default().with_user_config(
                &user_config_path_a,
                toml! { skills = { bundled = { enabled = false } } }.into(),
            );
        });
    let initial = initial_builder.build(&server).await?;
    let rollout_path = initial
        .session_configured
        .rollout_path
        .clone()
        .expect("rollout path");

    initial.submit_turn("catalog a").await?;
    initial.codex.shutdown_and_wait().await?;

    let catalog_a = host_catalog(&req1.single_request(), "catalog a");
    let user_config_path_b = codex_home_b.path().join("config.toml").abs();
    let mut builder_b = test_codex()
        .with_home(Arc::clone(&codex_home_b))
        .with_auth(CodexAuth::from_api_key("Test API Key"))
        .with_extensions(Arc::clone(&extensions))
        .with_config(move |config| {
            config.cwd = workspace_b.clone();
            config.config_layer_stack = ConfigLayerStack::default().with_user_config(
                &user_config_path_b,
                toml! { skills = { bundled = { enabled = false } } }.into(),
            );
        });
    let catalog_b_session = builder_b.build(&server).await?;
    catalog_b_session.submit_turn("catalog b").await?;
    catalog_b_session.codex.shutdown_and_wait().await?;
    let catalog_b = host_catalog(&req2.single_request(), "catalog b");
    append_compacted_catalog_history(&rollout_path, &catalog_a, &catalog_b)?;

    let resume_cwd = workspace_a;
    let resume_user_config_path = codex_home_a.path().join("config.toml").abs();
    let mut resume_builder = test_codex()
        .with_home(Arc::clone(&codex_home_a))
        .with_auth(CodexAuth::from_api_key("Test API Key"))
        .with_extensions(extensions)
        .with_config(move |config| {
            config.cwd = resume_cwd;
            config.config_layer_stack = ConfigLayerStack::default().with_user_config(
                &resume_user_config_path,
                toml! { skills = { bundled = { enabled = false } } }.into(),
            );
        });
    let resumed = resume_builder
        .resume(&server, Arc::clone(&codex_home_a), rollout_path)
        .await?;
    resumed.submit_turn("after resume").await?;

    let resumed_catalogs = req3
        .single_request()
        .message_input_texts("developer")
        .into_iter()
        .filter(|text| text.starts_with(SKILLS_INSTRUCTIONS_OPEN_TAG) && text.contains("(file:"))
        .collect::<Vec<_>>();
    assert_eq!(
        resumed_catalogs,
        vec![catalog_a.clone(), catalog_b, catalog_a]
    );
    Ok(())
}
