use super::PluginManifest;
use super::PluginManifestMcpServers;
use super::load_plugin_manifest;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_plugins::AGENT_PLUGIN_SCHEMA_URI;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write_agent_plugin_manifest(plugin_root: &Path, extra_fields: &str) {
    fs::create_dir_all(plugin_root).expect("create plugin root");
    fs::write(
        plugin_root.join("plugin.json"),
        format!(
            r#"{{
  "$schema": "{AGENT_PLUGIN_SCHEMA_URI}",
  "name": "demo-plugin"{extra_fields}
}}"#
        ),
    )
    .expect("write Agent Plugins manifest");
}

fn load_manifest(plugin_root: &Path) -> PluginManifest {
    load_plugin_manifest(plugin_root).expect("load plugin manifest")
}

#[test]
fn maps_portable_metadata_and_fixed_components() {
    let tmp = tempdir().expect("tempdir");
    let plugin_root = tmp.path().join("demo-plugin");
    write_agent_plugin_manifest(
        &plugin_root,
        r#",
  "version": "release-2026-07",
  "description": "Portable demo",
  "author": {"name": "Portable Author"},
  "homepage": "https://example.com/plugin",
  "keywords": ["portable"]"#,
    );

    let manifest = load_manifest(&plugin_root);
    assert_eq!(manifest.name, "demo-plugin");
    assert_eq!(manifest.version.as_deref(), Some("release-2026-07"));
    assert_eq!(manifest.description.as_deref(), Some("Portable demo"));
    assert_eq!(manifest.keywords, vec!["portable"]);
    assert_eq!(
        manifest.paths.skills,
        vec![
            AbsolutePathBuf::from_absolute_path_checked(plugin_root.join("skills"))
                .expect("skills path")
        ]
    );
    assert_eq!(
        manifest.paths.mcp_servers,
        Some(PluginManifestMcpServers::Path(
            AbsolutePathBuf::from_absolute_path_checked(plugin_root.join("mcp.json"))
                .expect("MCP path")
        ))
    );
    let interface = manifest.interface.expect("portable interface");
    assert_eq!(interface.display_name.as_deref(), Some("demo-plugin"));
    assert_eq!(interface.short_description.as_deref(), Some("Portable demo"));
    assert_eq!(interface.long_description.as_deref(), Some("Portable demo"));
    assert_eq!(
        interface.developer_name.as_deref(),
        Some("Portable Author")
    );
    assert_eq!(
        interface.website_url.as_deref(),
        Some("https://example.com/plugin")
    );
}

#[test]
fn rejects_invalid_names_and_unsupported_schemas() {
    let tmp = tempdir().expect("tempdir");
    let plugin_root = tmp.path().join("demo-plugin");
    write_agent_plugin_manifest(&plugin_root, "");

    fs::write(
        plugin_root.join("plugin.json"),
        format!(r#"{{"$schema":"{AGENT_PLUGIN_SCHEMA_URI}","name":"Demo_Plugin"}}"#),
    )
    .expect("write invalid name");
    assert_eq!(load_plugin_manifest(&plugin_root), None);

    fs::write(
        plugin_root.join("plugin.json"),
        r#"{"$schema":"https://agent-plugins.org/schemas/2.0.0/plugin.schema.json","name":"demo-plugin"}"#,
    )
    .expect("write unsupported schema");
    assert_eq!(load_plugin_manifest(&plugin_root), None);
}

#[test]
fn applies_codex_overlay_without_replacing_portable_components() {
    let tmp = tempdir().expect("tempdir");
    let plugin_root = tmp.path().join("demo-plugin");
    write_agent_plugin_manifest(
        &plugin_root,
        r#",
  "version": "portable-version",
  "description": "Portable description""#,
    );
    fs::create_dir_all(plugin_root.join(".codex-plugin")).expect("create overlay dir");
    fs::write(
        plugin_root.join(".codex-plugin/plugin.json"),
        r#"{
  "name": "different-name",
  "version": "9.9.9",
  "skills": [],
  "mcpServers": null,
  "interface": {"displayName": "Codex Demo"}
}"#,
    )
    .expect("write overlay");

    let manifest = load_manifest(&plugin_root);
    assert_eq!(manifest.name, "demo-plugin");
    assert_eq!(manifest.version.as_deref(), Some("portable-version"));
    assert_eq!(
        manifest.paths.skills,
        vec![
            AbsolutePathBuf::from_absolute_path_checked(plugin_root.join("skills"))
                .expect("skills path")
        ]
    );
    assert_eq!(
        manifest
            .interface
            .and_then(|interface| interface.display_name),
        Some("Codex Demo".to_string())
    );
}
