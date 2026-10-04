use super::*;
use pretty_assertions::assert_eq;

#[cfg(target_os = "macos")]
#[test]
fn macos_update_probe_uses_the_persisted_production_appcast_feed() {
    let home = tempfile::tempdir().expect("temporary home should be created");
    let application = InstalledApp {
        identity: "com.openai.codex",
        version: "26.623.10000".to_string(),
        bundle: PathBuf::new(),
        build: 6139,
    };
    assert_eq!(
        macos_desktop_update_url(home.path(), &application, "26.6.0"),
        DESKTOP_UPDATE_URL
    );

    let state_directory = home
        .path()
        .join("Library/Application Support/com.openai.codex");
    std::fs::create_dir_all(&state_directory)
        .expect("production appcast state directory should be created");
    std::fs::write(
        state_directory.join("production-appcast-bootstrap.json"),
        r#"{"backendAppcastEnabled":true,"installationId":"028e90f8-5f2a-47db-a05c-6a48f548d728"}"#,
    )
    .expect("production appcast state should be created");

    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        "arm64"
    };
    assert_eq!(
        macos_desktop_update_url(home.path(), &application, "26.6.0"),
        format!(
            "{BACKEND_DESKTOP_UPDATE_URL}?installation_id=028e90f8-5f2a-47db-a05c-6a48f548d728&arch={arch}&app_version=26.623.10000&beta=false&os-version=26.6.0&plan_type=unknown"
        )
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn macos_staged_updates_require_a_newer_matching_extracted_bundle() {
    let root = tempfile::tempdir().expect("temporary Sparkle cache should be created");
    for index in 0..320 {
        std::fs::create_dir(root.path().join(format!("unrelated-{index}")))
            .expect("unrelated Sparkle cache directory should be created");
    }
    for (name, identity, build) in [
        ("newest", "com.openai.codex", "6268"),
        ("newer", "com.openai.codex", "6168"),
        ("older", "com.openai.codex", "6138"),
        ("different", "com.example.other", "9999"),
        ("invalid", "com.openai.codex", "invalid"),
    ] {
        let bundle = root.path().join(name).join("extracted/ChatGPT.app");
        write_macos_bundle(&bundle, identity, build);
    }
    let outside = tempfile::tempdir().expect("external fixture should be created");
    let linked = outside.path().join("ChatGPT.app");
    write_macos_bundle(&linked, "com.openai.codex", "9999");
    std::os::unix::fs::symlink(&linked, root.path().join("ChatGPT.app"))
        .expect("symlinked staged app fixture should be created");

    assert_eq!(
        latest_macos_staged_build(root.path(), /*installed_build*/ 6139).await,
        Some(6268)
    );
    assert_eq!(
        latest_macos_staged_build(root.path(), /*installed_build*/ 6268).await,
        None
    );
}

#[test]
fn windows_store_updates_compare_all_four_production_build_components() {
    let mut manifest = serde_json::json!({
        "schemaVersion": 1,
        "buildVersion": "26.803.5235.1",
        "storeProductId": "9PLM9XGG6VKS",
        "packageIdentity": "OpenAI.Codex",
    });
    assert_eq!(
        windows_store_update(&serde_json::to_vec(&manifest).unwrap(), "26.803.5235.0"),
        Ok(Some("26.803.5235.1".to_string()))
    );
    assert_eq!(
        windows_store_update(&serde_json::to_vec(&manifest).unwrap(), "26.803.5235.1"),
        Ok(None)
    );
    manifest["storeProductId"] = "other".into();
    assert!(
        windows_store_update(&serde_json::to_vec(&manifest).unwrap(), "26.803.5235.0").is_err()
    );
}

#[cfg(target_os = "macos")]
fn write_macos_bundle(path: &Path, identity: &str, build: &str) {
    let contents = path.join("Contents");
    std::fs::create_dir_all(&contents).expect("staged app fixture should be created");
    std::fs::write(
        contents.join("Info.plist"),
        format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>{identity}</string>\
             <key>CFBundleVersion</key><string>{build}</string>\
             </dict></plist>"
        ),
    )
    .expect("staged app metadata should be created");
}
