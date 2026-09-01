use super::*;
use codex_file_system::CopyOptions;
use codex_file_system::CreateDirectoryOptions;
use codex_file_system::ExecutorFileSystemFuture;
use codex_file_system::FileMetadata;
use codex_file_system::FileSystemReadStream;
use codex_file_system::FileSystemSandboxContext;
use codex_file_system::ReadDirectoryEntry;
use codex_file_system::RemoveOptions;
use codex_protocol::config_types::ForcedLoginMethod;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

struct TestFileSystem;

impl ExecutorFileSystem for TestFileSystem {
    fn canonicalize<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, PathUri> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            let canonicalized = path.canonicalize()?;
            Ok(PathUri::from_abs_path(&canonicalized))
        })
    }

    fn read_file<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            tokio::fs::read(path.as_path()).await
        })
    }

    fn read_file_stream<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileSystemReadStream> {
        Box::pin(async {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "test filesystem does not support streaming reads",
            ))
        })
    }

    fn write_file<'a>(
        &'a self,
        _path: &'a PathUri,
        _contents: Vec<u8>,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn create_directory<'a>(
        &'a self,
        _path: &'a PathUri,
        _create_directory_options: CreateDirectoryOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn get_metadata<'a>(
        &'a self,
        path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, FileMetadata> {
        Box::pin(async move {
            let path = path.to_abs_path()?;
            let metadata = tokio::fs::symlink_metadata(path.as_path()).await?;
            Ok(FileMetadata {
                is_directory: metadata.is_dir(),
                is_file: metadata.is_file(),
                is_symlink: metadata.file_type().is_symlink(),
                size: metadata.len(),
                created_at_ms: 0,
                modified_at_ms: 0,
            })
        })
    }

    fn read_directory<'a>(
        &'a self,
        _path: &'a PathUri,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, Vec<ReadDirectoryEntry>> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn remove<'a>(
        &'a self,
        _path: &'a PathUri,
        _remove_options: RemoveOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }

    fn copy<'a>(
        &'a self,
        _source_path: &'a PathUri,
        _destination_path: &'a PathUri,
        _copy_options: CopyOptions,
        _sandbox: Option<&'a FileSystemSandboxContext>,
    ) -> ExecutorFileSystemFuture<'a, ()> {
        Box::pin(async move { unimplemented!("test filesystem only supports reads") })
    }
}

#[tokio::test]
async fn packaged_defaults_have_lower_precedence_than_existing_layers() {
    let tmp = tempdir().expect("tempdir");
    let packaged_defaults_path =
        AbsolutePathBuf::resolve_path_against_base("packaged-defaults.toml", tmp.path());
    let system_config_path = tmp.path().join("system.toml");
    let user_config_path = tmp.path().join(CONFIG_TOML_FILE);
    std::fs::write(
        packaged_defaults_path.as_path(),
        "model = \"packaged-model\"\nmodel_provider = \"packaged-provider\"\nmodel_context_window = 120000\n",
    )
    .expect("write packaged defaults");
    std::fs::write(
        &system_config_path,
        "model = \"system-model\"\nmodel_provider = \"system-provider\"\n",
    )
    .expect("write system config");
    std::fs::write(&user_config_path, "model = \"user-model\"\n").expect("write user config");
    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.packaged_defaults_path = Some(packaged_defaults_path.clone());
    overrides.system_config_path = Some(system_config_path.clone());

    let stack = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[(
            "model".to_string(),
            TomlValue::String("session-model".to_string()),
        )],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("load config layers");

    assert_eq!(
        stack
            .get_layers(
                crate::ConfigLayerStackOrdering::LowestPrecedenceFirst,
                /*include_disabled*/ false,
            )
            .into_iter()
            .map(|layer| layer.name.clone())
            .collect::<Vec<_>>(),
        vec![
            ConfigLayerSource::PackagedDefaults {
                file: packaged_defaults_path,
            },
            ConfigLayerSource::System {
                file: AbsolutePathBuf::from_absolute_path(system_config_path)
                    .expect("absolute system config path"),
            },
            ConfigLayerSource::User {
                file: AbsolutePathBuf::from_absolute_path(user_config_path)
                    .expect("absolute user config path"),
                profile: None,
            },
            ConfigLayerSource::SessionFlags,
        ]
    );
    assert_eq!(
        stack.effective_config(),
        toml::toml! {
            model = "session-model"
            model_provider = "system-provider"
            model_context_window = 120000
        }
        .into()
    );
}

#[tokio::test]
async fn missing_packaged_defaults_file_returns_an_error() {
    let tmp = tempdir().expect("tempdir");
    let packaged_defaults_path =
        AbsolutePathBuf::resolve_path_against_base("packaged-defaults.toml", tmp.path());
    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.packaged_defaults_path = Some(packaged_defaults_path.clone());

    let err = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect_err("an explicitly configured packaged defaults file must exist");

    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert_eq!(
        err.to_string(),
        format!(
            "packaged defaults config file {} not found",
            packaged_defaults_path.display()
        )
    );
}

#[tokio::test]
async fn ignore_login_requirements_only_strips_managed_auth_policy() {
    let tmp = tempdir().expect("tempdir");
    std::fs::write(
        tmp.path().join("requirements.toml"),
        concat!(
            "allowed_login_methods = [\"api\"]\n",
            "allowed_chatgpt_workspaces = [\"workspace-a\"]\n",
            "allowed_approval_policies = [\"never\"]\n",
        ),
    )
    .expect("write requirements");
    let loader_overrides =
        LoaderOverrides::with_managed_config_path_for_tests(tmp.path().join("managed_config.toml"));

    let local_layers = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        loader_overrides.clone(),
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("load local requirements");
    assert_eq!(
        local_layers
            .requirements()
            .managed_auth_policy()
            .allowed_login_methods(),
        vec![ForcedLoginMethod::Api]
    );
    assert_eq!(
        local_layers
            .requirements()
            .managed_auth_policy()
            .allowed_chatgpt_workspaces(),
        Some(["workspace-a".to_string()].as_slice())
    );

    let remote_layers = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        LoaderOverrides {
            ignore_login_requirements: true,
            ..loader_overrides
        },
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("load requirements for remote workspace");
    assert_eq!(
        remote_layers
            .requirements()
            .managed_auth_policy()
            .allowed_login_methods(),
        vec![ForcedLoginMethod::Api, ForcedLoginMethod::Chatgpt]
    );
    assert_eq!(
        remote_layers
            .requirements()
            .managed_auth_policy()
            .allowed_chatgpt_workspaces(),
        None
    );
    assert!(
        remote_layers
            .requirements()
            .approval_policy
            .source
            .is_some()
    );
}

#[tokio::test]
async fn profile_v2_rejects_matching_legacy_profile_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
model = "gpt-main"

[profiles.work]
model = "gpt-work"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    let err = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect_err("profile-v2 should reject a matching legacy profile in base user config");

    assert_eq!(
        err.kind(),
        io::ErrorKind::InvalidData,
        "a matching legacy profile should be a hard config error"
    );
    let message = err.to_string();
    assert!(
        message.contains("--profile `work` cannot be used"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("config.toml"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("[profiles.work]"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("https://developers.openai.com/codex/config-advanced#profiles"),
        "unexpected error message: {message}"
    );
}

#[tokio::test]
async fn profile_v2_rejects_matching_legacy_profile_selector_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
profile = "work"
model = "gpt-main"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    let err = load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect_err("profile-v2 should reject a matching legacy profile selector");

    assert_eq!(
        err.kind(),
        io::ErrorKind::InvalidData,
        "a matching legacy profile selector should be a hard config error"
    );
    let message = err.to_string();
    assert!(
        message.contains("--profile `work` cannot be used"),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("profile = \"work\""),
        "unexpected error message: {message}"
    );
    assert!(
        message.contains("work.config.toml"),
        "unexpected error message: {message}"
    );
}

#[tokio::test]
async fn profile_v2_allows_unrelated_legacy_profiles_in_base_user_config() {
    let tmp = tempdir().expect("tempdir");
    let selected_config = tmp.path().join("work.config.toml");

    std::fs::write(
        tmp.path().join(CONFIG_TOML_FILE),
        r#"
model = "gpt-main"

[profiles.dev]
model = "gpt-dev"
"#,
    )
    .expect("write default user config");
    std::fs::write(&selected_config, r#"model = "gpt-work-v2""#)
        .expect("write selected user config");

    let mut overrides = LoaderOverrides::without_managed_config_for_tests();
    overrides.user_config_path = Some(AbsolutePathBuf::resolve_path_against_base(
        "work.config.toml",
        tmp.path(),
    ));
    overrides.user_config_profile = Some("work".parse().expect("profile-v2 name"));

    load_config_layers_state(
        &TestFileSystem,
        tmp.path(),
        /*cwd*/ None,
        &[],
        overrides,
        &crate::NoopThreadConfigLoader,
    )
    .await
    .expect("profile-v2 should allow unrelated legacy profiles in base user config");
}

#[test]
fn local_layer_projection_preserves_blockers_and_cloud_position() {
    let tmp = tempdir().expect("tempdir");
    let base_dir = AbsolutePathBuf::from_absolute_path(tmp.path()).expect("absolute base");
    let layer = |source, contents| LocalTomlLayer {
        source,
        base_dir: base_dir.clone(),
        toml: toml::from_str(contents).expect("valid TOML"),
    };
    let requirements = LocalTomlLayerStack {
        layers: Vec::<LocalTomlLayer<RequirementSource>>::new(),
        cloud_insertion_index: 0,
    };
    let layers = LocalConfigLayers {
        config: LocalTomlLayerStack {
            layers: vec![
                layer(
                    ConfigLayerSource::System {
                        file: base_dir.join("system.toml"),
                    },
                    "ignored=true\n\"literal.key\"=\"literal\"\narray=[1,2]\n[a]\nb=1\nc=2",
                ),
                layer(ConfigLayerSource::SessionFlags, "a=2\nonly_user=true"),
                layer(
                    ConfigLayerSource::LegacyManagedConfigTomlFromMdm,
                    "[a]\nunrequested=true",
                ),
            ],
            cloud_insertion_index: 1,
        },
        requirements,
    };

    let only_user = layers
        .clone()
        .project(&[vec!["only_user".into()]], &[]);
    assert_eq!(
        only_user.config,
        LocalTomlLayerStack {
            layers: vec![layer(ConfigLayerSource::SessionFlags, "only_user=true")],
            cloud_insertion_index: 0,
        }
    );

    let projected = layers.project(
        &[
            vec!["a".into(), "b".into()],
            vec!["array".into(), "unused".into()],
            vec!["literal.key".into()],
        ],
        &[],
    );
    assert_eq!(
        projected.config,
        LocalTomlLayerStack {
            layers: vec![
                layer(
                    ConfigLayerSource::System {
                        file: base_dir.join("system.toml"),
                    },
                    "\"literal.key\"=\"literal\"\narray=[1,2]\n[a]\nb=1",
                ),
                layer(ConfigLayerSource::SessionFlags, "a=2"),
                layer(ConfigLayerSource::LegacyManagedConfigTomlFromMdm, "[a]"),
            ],
            cloud_insertion_index: 1,
        }
    );

    let mut merged = TomlValue::Table(toml::map::Map::new());
    for layer in projected.config.layers {
        merge_toml_values(&mut merged, &layer.toml);
    }
    assert_eq!(
        merged.get("a"),
        Some(&TomlValue::Table(toml::map::Map::new()))
    );
}

#[tokio::test]
async fn local_layers_preserve_raw_paths_trust_and_managed_auth() {
    let tmp = tempdir().expect("tempdir");
    let codex_home = tmp.path().join("codex-home");
    let project = tmp.path().join("project");
    let dot_codex = project.join(".codex");
    let system_dir = tmp.path().join("system");
    let managed_dir = tmp.path().join("managed");
    for dir in [&codex_home, &dot_codex, &system_dir, &managed_dir] {
        std::fs::create_dir_all(dir).expect("create fixture directory");
    }
    std::fs::write(project.join(".project-root"), "").expect("write project marker");

    let project_key = TomlValue::String(project_trust_key(&project)).to_string();
    let user_config = |trust_level| {
        format!(
            "project_root_markers=[\".project-root\"]\nmodel_instructions_file=\"./user.md\"\n[projects.{project_key}]\ntrust_level=\"{trust_level}\""
        )
    };
    let user_file = codex_home.join(CONFIG_TOML_FILE);
    std::fs::write(&user_file, user_config("trusted")).expect("write user config");
    let system_file = system_dir.join(CONFIG_TOML_FILE);
    std::fs::write(&system_file, "model_instructions_file=\"./system.md\"")
        .expect("write system config");
    std::fs::write(
        dot_codex.join(CONFIG_TOML_FILE),
        concat!(
            "model_instructions_file=\"./project.md\"\n",
            "openai_base_url=\"https://ignored\"\n",
            "[tui]\napp_theme=\"dark\"",
        ),
    )
    .expect("write project config");
    let managed_file = managed_dir.join("managed_config.toml");
    std::fs::write(
        &managed_file,
        concat!(
            "approval_policy=\"never\"\n",
            "sandbox_mode=\"workspace-write\"\n",
            "model_instructions_file=\"./managed.md\"",
        ),
    )
    .expect("write managed config");
    let requirements_file = managed_dir.join("requirements.toml");
    std::fs::write(
        &requirements_file,
        concat!(
            "allowed_login_methods=[\"api\"]\n",
            "allowed_chatgpt_workspaces=[\"workspace-a\"]\n",
            "allowed_sandbox_modes=[\"read-only\"]\n",
            "log_dir=\"./logs\"",
        ),
    )
    .expect("write system requirements");

    let mut overrides = LoaderOverrides::with_managed_config_path_for_tests(managed_file);
    overrides.system_config_path = Some(system_file);
    overrides.system_requirements_path = Some(requirements_file);
    let cwd = AbsolutePathBuf::from_absolute_path(&project).expect("absolute cwd");
    let layers = local::load_local_config_layers_with_overrides(
        &TestFileSystem,
        &codex_home,
        &cwd,
        &overrides,
    )
    .await
    .expect("load local layers");

    assert_eq!(
        (
            layers
                .config
                .layers
                .iter()
                .map(|layer| layer.base_dir.to_path_buf())
                .collect::<Vec<_>>(),
            layers.config.cloud_insertion_index,
            layers.requirements.cloud_insertion_index,
        ),
        (
            vec![
                system_dir,
                codex_home.clone(),
                dot_codex.clone(),
                managed_dir,
            ],
            1,
            1,
        )
    );
    let project_toml = &layers.config.layers[2].toml;
    assert_eq!(
        (
            project_toml.get("model_instructions_file"),
            project_toml.get("openai_base_url"),
            project_toml
                .get("tui")
                .and_then(TomlValue::as_table)
                .and_then(|tui| tui.get("app_theme")),
            layers.config.layers[3]
                .toml
                .get("model_instructions_file"),
            layers.requirements.layers[0].toml.clone(),
            layers.requirements.layers[1].toml.clone(),
        ),
        (
            Some(&TomlValue::String("./project.md".into())),
            None,
            None,
            Some(&TomlValue::String("./managed.md".into())),
            toml::from_str(concat!(
                "allowed_login_methods=[\"api\"]\n",
                "allowed_chatgpt_workspaces=[\"workspace-a\"]\n",
                "allowed_sandbox_modes=[\"read-only\"]\n",
                "log_dir=\"./logs\"",
            ))
            .expect("system requirements TOML"),
            toml::from_str(
                "allowed_approval_policies=[\"never\"]\nallowed_sandbox_modes=[\"read-only\",\"workspace-write\"]"
            )
            .expect("legacy requirements TOML"),
        )
    );

    std::fs::write(&user_file, user_config("untrusted")).expect("write user config");
    let layers = local::load_local_config_layers_with_overrides(
        &TestFileSystem,
        &codex_home,
        &cwd,
        &overrides,
    )
    .await
    .expect("load untrusted local layers");
    assert_eq!(
        layers
            .config
            .layers
            .iter()
            .filter(|layer| matches!(layer.source, ConfigLayerSource::Project { .. }))
            .count(),
        0
    );
}
