use super::*;
use crate::legacy_core::config::ConfigBuilder;
use crate::pets::ImageProtocol;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;

async fn finish(shell: &mut ShellState) {
    tokio::time::timeout(std::time::Duration::from_secs(/*secs*/ 10), async {
        while shell.pets.has_work() {
            shell.poll_pets().await;
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn loaded_pet_reserves_space_without_overlapping_workspace_or_small_terminals() {
    let shell = ShellState::snapshot_fixture();
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 30,
    );
    assert_eq!(shell.pets.content_area(area), area);
    shell.pets.0.lock().unwrap().pet = Some(crate::pets::test_ambient_pet(
        FrameRequester::test_dummy(),
        /*animations_enabled*/ false,
    ));
    let content = shell.pets.content_area(area);
    let layout = super::super::shell_layout::calculate(&shell, area).unwrap();
    assert!(layout.input.right() <= content.right());
    assert!(layout.transcript.right() <= content.right());
    assert!(layout.dashboard.unwrap().area().right() <= content.right());
    let pet_area = Rect::new(
        content.right(),
        area.y,
        area.right() - content.right(),
        area.height,
    );
    let draw = shell
        .pets
        .0
        .lock()
        .unwrap()
        .pet
        .as_ref()
        .unwrap()
        .draw_request(pet_area, area.bottom())
        .unwrap();
    assert!(draw.x >= content.right());
    assert!(draw.x + draw.columns <= area.right());
    assert!(draw.y + draw.rows <= area.bottom());
    let narrow = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 12,
    );
    assert_eq!(shell.pets.content_area(narrow), narrow);
    let mut buf = Buffer::empty(area);
    super::super::render::ShellView { shell: &shell }.render(area, &mut buf);
    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered);
}

#[tokio::test]
async fn picker_uses_the_existing_fullscreen_selector() {
    let home = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.codex_home = home.path().to_path_buf();
    shell.status = "thinking".into();
    shell.run_pets_command("");
    finish(&mut shell).await;
    assert_eq!(shell.status, "thinking");
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 90, /*height*/ 26,
    );
    let mut buf = Buffer::empty(area);
    shell
        .selector
        .as_ref()
        .unwrap()
        .render(area, /*pointer*/ None, &mut buf);
    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered);
}

#[tokio::test]
async fn asset_load_precedes_persistence_and_disable_is_shared_across_sessions() {
    let home = tempfile::tempdir().unwrap();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .build()
        .await
        .unwrap();
    crate::pets::write_test_pack(home.path());
    let path = config.codex_home.join("selected-config.toml");
    let mut shell = ShellState::snapshot_fixture();
    shell.pets.configure(&config, FrameRequester::test_dummy());
    {
        let mut state = shell.pets.0.lock().unwrap();
        let settings = state.config.as_mut().unwrap();
        settings.path = path.clone();
        settings.support = PetImageSupport::Supported(ImageProtocol::Kitty);
    }
    shell.select_pet("missing-custom-pet".into());
    finish(&mut shell).await;
    assert!(!path.exists());
    assert!(shell.pets.0.lock().unwrap().selected.is_none());
    shell.select_pet("codex".into());
    finish(&mut shell).await;
    assert!(shell.pets.0.lock().unwrap().pet.is_some());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("pet = \"codex\"")
    );
    let mut child = ShellState::snapshot_fixture();
    child.pets = shell.pets.clone();
    child.select_pet(crate::pets::DISABLED_PET_ID.into());
    finish(&mut child).await;
    assert!(shell.pets.0.lock().unwrap().pet.is_none());
    assert_eq!(
        shell.pets.0.lock().unwrap().selected.as_deref(),
        Some(crate::pets::DISABLED_PET_ID)
    );
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("pet = \"disabled\"")
    );
}

#[tokio::test]
async fn teardown_cancels_pending_asset_work() {
    let state = PetState::default();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    state.0.lock().unwrap().pending = Some(tokio::spawn(async move {
        rx.await?;
        anyhow::bail!("unexpected completion")
    }));
    drop(state);
    tokio::task::yield_now().await;
    assert!(tx.is_closed());
}

#[tokio::test]
async fn pending_selection_is_not_aborted_by_a_second_choice() {
    let mut shell = ShellState::snapshot_fixture();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let thread_id = shell.thread_id;
    shell.pets.0.lock().unwrap().pending = Some(tokio::spawn(async move {
        rx.await?;
        Ok(PetUpdate::Catalog {
            thread_id,
            choices: Vec::new(),
        })
    }));
    shell.select_pet(crate::pets::DISABLED_PET_ID.into());
    tx.send(()).unwrap();
    finish(&mut shell).await;
    assert!(shell.selector.is_some());
}

#[tokio::test]
async fn a_late_catalog_does_not_replace_a_newer_selector() {
    let home = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.codex_home = home.path().to_path_buf();
    shell.run_pets_command("");
    shell.open_model_selector();
    let selector = shell.selector.clone();
    finish(&mut shell).await;
    assert_eq!(shell.selector, selector);
}

#[tokio::test]
async fn saved_preference_respects_higher_priority_pet_configuration() {
    let home = tempfile::tempdir().unwrap();
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .cli_overrides(vec![(
            "tui.pet".into(),
            toml::Value::String("disabled".into()),
        )])
        .build()
        .await
        .unwrap();
    crate::pets::write_test_pack(home.path());
    let mut shell = ShellState::snapshot_fixture();
    shell.pets.configure(&config, FrameRequester::test_dummy());
    shell
        .pets
        .0
        .lock()
        .unwrap()
        .config
        .as_mut()
        .unwrap()
        .support = PetImageSupport::Supported(ImageProtocol::Kitty);
    shell.select_pet("codex".into());
    finish(&mut shell).await;
    let state = shell.pets.0.lock().unwrap();
    assert_eq!(
        (state.selected.as_deref(), state.pet.is_none()),
        (Some("disabled"), true)
    );
    assert!(
        std::fs::read_to_string(home.path().join("config.toml"))
            .unwrap()
            .contains("pet = \"codex\"")
    );
}
