use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

const PLUGIN_ROOT_VARIABLE: &str = "PLUGIN_ROOT";
const PLUGIN_DATA_VARIABLE: &str = "PLUGIN_DATA";

pub(super) fn normalize_agent_plugin_stdio_server(
    mut command: String,
    mut args: Vec<String>,
    mut env: BTreeMap<String, String>,
    cwd: Option<String>,
    plugin_root: &Path,
    plugin_data_root: &Path,
) -> Result<JsonMap<String, JsonValue>, String> {
    #[cfg(windows)]
    let has_windows_path_prefix = matches!(
        Path::new(&command).components().next(),
        Some(std::path::Component::Prefix(_))
    );
    #[cfg(not(windows))]
    let has_windows_path_prefix = false;
    let is_bare_command = !command.is_empty()
        && !command.contains('/')
        && !command.contains('\\')
        && !has_windows_path_prefix;
    let is_plugin_relative_command =
        command.starts_with("./") && is_portable_relative_path(&command);
    if !is_bare_command && !is_plugin_relative_command {
        return Err(
            "Agent Plugins stdio command must be a bare executable name or a contained `./` path"
                .to_string(),
        );
    }
    for reserved in [PLUGIN_ROOT_VARIABLE, PLUGIN_DATA_VARIABLE] {
        if env
            .keys()
            .any(|name| environment_variable_names_match(name, reserved))
        {
            return Err(format!(
                "Agent Plugins stdio `env` cannot override reserved variable `{reserved}`"
            ));
        }
    }
    #[cfg(windows)]
    {
        let mut normalized_env = BTreeMap::new();
        for (name, value) in env {
            let normalized_name = name.to_ascii_uppercase();
            if normalized_env.insert(normalized_name, value).is_some() {
                return Err(format!(
                    "duplicate case-insensitive Agent Plugins environment variable `{name}`"
                ));
            }
        }
        env = normalized_env;
    }

    let root_path = absolute_plugin_path(plugin_root)?;
    let data_root_path = absolute_plugin_path(plugin_data_root)?;
    let root = host_path_string(&root_path);
    let data_root = host_path_string(&data_root_path);
    if command.starts_with("./") {
        command = host_path_string(&resolve_contained_host_path(
            &command, &root_path, &root_path,
        )?);
    }
    for arg in &mut args {
        *arg = expand_agent_plugin_placeholders(arg, &root, &data_root);
    }
    for value in env.values_mut() {
        *value = expand_agent_plugin_placeholders(value, &root, &data_root);
    }

    let cwd = cwd.as_deref().unwrap_or("${PLUGIN_ROOT}");
    let Some(cwd_root) = parse_agent_plugin_cwd(cwd) else {
        return Err(
            "Agent Plugins stdio `cwd` must be a contained `./`, `${PLUGIN_ROOT}`, or `${PLUGIN_DATA}` path"
                .to_string(),
        );
    };
    let cwd = expand_agent_plugin_placeholders(cwd, &root, &data_root);
    let cwd_root = match cwd_root {
        AgentPluginCwdRoot::Package => &root_path,
        AgentPluginCwdRoot::Data => &data_root_path,
    };
    env.insert(PLUGIN_ROOT_VARIABLE.to_string(), root);
    env.insert(PLUGIN_DATA_VARIABLE.to_string(), data_root);

    Ok(JsonMap::from_iter([
        ("command".to_string(), JsonValue::String(command)),
        (
            "args".to_string(),
            JsonValue::Array(args.into_iter().map(JsonValue::String).collect()),
        ),
        ("env".to_string(), string_map_value(env)),
        (
            "cwd".to_string(),
            JsonValue::String(host_path_string(&resolve_contained_host_path(
                &cwd, cwd_root, cwd_root,
            )?)),
        ),
    ]))
}

fn environment_variable_names_match(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn string_map_value(values: BTreeMap<String, String>) -> JsonValue {
    JsonValue::Object(
        values
            .into_iter()
            .map(|(name, value)| (name, JsonValue::String(value)))
            .collect(),
    )
}

#[derive(Clone, Copy)]
enum AgentPluginCwdRoot {
    Package,
    Data,
}

fn parse_agent_plugin_cwd(value: &str) -> Option<AgentPluginCwdRoot> {
    if value == "./" {
        return Some(AgentPluginCwdRoot::Package);
    }
    if let Some(relative) = value.strip_prefix("./")
        && is_portable_path_suffix(relative)
    {
        return Some(AgentPluginCwdRoot::Package);
    }
    for (placeholder, root) in [
        ("${PLUGIN_ROOT}", AgentPluginCwdRoot::Package),
        ("${PLUGIN_DATA}", AgentPluginCwdRoot::Data),
    ] {
        if value == placeholder {
            return Some(root);
        }
        if let Some(relative) = value.strip_prefix(&format!("{placeholder}/"))
            && (relative.is_empty() || is_portable_path_suffix(relative))
        {
            return Some(root);
        }
    }
    None
}

fn expand_agent_plugin_placeholders(value: &str, plugin_root: &str, plugin_data: &str) -> String {
    const ROOT: &str = "${PLUGIN_ROOT}";
    const DATA: &str = "${PLUGIN_DATA}";
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    loop {
        let next = match (remaining.find(ROOT), remaining.find(DATA)) {
            (Some(root), Some(data)) if root <= data => Some((root, ROOT, plugin_root)),
            (Some(_), Some(data)) => Some((data, DATA, plugin_data)),
            (Some(root), None) => Some((root, ROOT, plugin_root)),
            (None, Some(data)) => Some((data, DATA, plugin_data)),
            (None, None) => None,
        };
        let Some((index, placeholder, replacement)) = next else {
            output.push_str(remaining);
            break;
        };
        output.push_str(&remaining[..index]);
        output.push_str(replacement);
        remaining = &remaining[index + placeholder.len()..];
    }
    output
}

fn absolute_plugin_path(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| format!("failed to resolve plugin path: {err}"))
    }?;
    resolve_existing_path_prefix(&absolute)
}

fn resolve_contained_host_path(
    value: &str,
    root: &Path,
    allowed_root: &Path,
) -> Result<PathBuf, String> {
    let value = Path::new(value);
    let path = if value.is_absolute() {
        value.to_path_buf()
    } else {
        root.join(value)
    };
    let path = resolve_existing_path_prefix(&path)?;
    if !path.starts_with(allowed_root) {
        return Err(format!(
            "expanded path `{}` must remain within `{}`",
            value.display(),
            allowed_root.display()
        ));
    }
    Ok(path)
}

fn resolve_existing_path_prefix(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path.to_path_buf();
    let mut missing_components = Vec::<OsString>::new();
    loop {
        match std::fs::canonicalize(&existing) {
            Ok(mut resolved) => {
                for component in missing_components.iter().rev() {
                    resolved.push(component);
                }
                return Ok(lexical_normalize(&resolved));
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::symlink_metadata(&existing)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(format!("failed to resolve symlinked path `{}`", path.display()));
                }
                let Some(component) = existing.components().next_back() else {
                    return Err(format!("failed to resolve path `{}`: {err}", path.display()));
                };
                if matches!(
                    component,
                    std::path::Component::Prefix(_) | std::path::Component::RootDir
                ) {
                    return Err(format!("failed to resolve path `{}`: {err}", path.display()));
                }
                missing_components.push(component.as_os_str().to_os_string());
                if !existing.pop() {
                    return Err(format!("failed to resolve path `{}`: {err}", path.display()));
                }
            }
            Err(err) => return Err(format!("failed to resolve path `{}`: {err}", path.display())),
        }
    }
}

fn host_path_string(path: &Path) -> String {
    let rendered = path.to_string_lossy();
    #[cfg(windows)]
    if let Some(path) = rendered.strip_prefix(r"\\?\") {
        return path
            .strip_prefix(r"UNC\")
            .map(|path| format!(r"\\{path}"))
            .unwrap_or_else(|| path.to_string());
    }
    rendered.into_owned()
}

fn is_portable_relative_path(value: &str) -> bool {
    value
        .strip_prefix("./")
        .is_some_and(is_portable_path_suffix)
}

fn is_portable_path_suffix(value: &str) -> bool {
    !value.is_empty() && !value.contains('\\')
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}
