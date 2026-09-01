use std::hash::Hash;
use std::hash::Hasher;
use std::sync::Arc;
use std::sync::Weak;

use codex_config::SkillConfigRules;
use codex_exec_server::ExecutorFileSystem;
use codex_protocol::protocol::SkillScope;
use codex_skills::SkillRootSnapshots;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_plugins::PluginSkillRoot;
use codex_utils_plugins::SkillDiscoveryMode;

use crate::loader::HostSkillRoot;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ConfigSkillsCacheKey {
    roots: Vec<ConfigSkillRootCacheKey>,
    skill_config_rules: SkillConfigRules,
    plugin_skill_snapshots: Option<SkillRootSnapshots<PluginSkillRoot>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ConfigSkillRootCacheKey {
    path: AbsolutePathBuf,
    scope_rank: u8,
    plugin_id: Option<String>,
    plugin_namespace: Option<String>,
    plugin_root: Option<AbsolutePathBuf>,
    discovery_mode: SkillDiscoveryMode,
    file_system: FileSystemIdentity,
}

#[derive(Debug, Clone)]
struct FileSystemIdentity(Weak<dyn ExecutorFileSystem>);

impl PartialEq for FileSystemIdentity {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FileSystemIdentity {}

impl Hash for FileSystemIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as *const ()).hash(state);
    }
}

pub(crate) fn config_skills_cache_key(
    roots: &[HostSkillRoot],
    skill_config_rules: &SkillConfigRules,
    plugin_skill_snapshots: Option<&SkillRootSnapshots<PluginSkillRoot>>,
) -> ConfigSkillsCacheKey {
    ConfigSkillsCacheKey {
        roots: roots.iter().map(config_skill_root_cache_key).collect(),
        skill_config_rules: skill_config_rules.clone(),
        plugin_skill_snapshots: plugin_skill_snapshots
            .filter(|_| roots.iter().any(|root| root.plugin_id().is_some()))
            .cloned(),
    }
}

pub(crate) fn config_skill_root_cache_key(root: &HostSkillRoot) -> ConfigSkillRootCacheKey {
    let scope_rank = match root.scope {
        SkillScope::Repo => 0,
        SkillScope::User => 1,
        SkillScope::System => 2,
        SkillScope::Admin => 3,
    };
    ConfigSkillRootCacheKey {
        path: root.path.clone(),
        scope_rank,
        plugin_id: root.plugin_id().map(ToString::to_string),
        plugin_namespace: root.plugin_namespace().map(ToString::to_string),
        plugin_root: root.plugin_root().cloned(),
        discovery_mode: root.discovery_mode(),
        file_system: FileSystemIdentity(Arc::downgrade(&root.file_system)),
    }
}
