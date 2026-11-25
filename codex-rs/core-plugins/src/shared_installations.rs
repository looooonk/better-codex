//! Keep shared downloads available to the Codex App and CLI after local edits.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

pub(super) struct CodexRegistrations(Option<HashSet<String>>);

impl CodexRegistrations {
    pub(super) fn read(home: &Path, section: &str) -> Self {
        Self(Self::read_names(home, section).ok())
    }

    fn read_names(home: &Path, section: &str) -> io::Result<HashSet<String>> {
        let mut names = HashSet::new();
        for entry in fs::read_dir(home)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name != codex_config::CONFIG_TOML_FILE
                && !name
                    .strip_suffix(".config.toml")
                    .is_some_and(|profile| profile.parse::<codex_config::ProfileV2Name>().is_ok())
            {
                continue;
            }
            let contents = fs::read_to_string(entry.path())?;
            let config: toml::Value = toml::from_str(&contents).map_err(io::Error::other)?;
            let mut collect = |layer: &toml::Value| {
                if let Some(entries) = layer.get(section).and_then(toml::Value::as_table) {
                    names.extend(entries.keys().map(|name| name.to_ascii_lowercase()));
                }
            };
            collect(&config);
            if let Some(profiles) = config.get("profiles").and_then(toml::Value::as_table) {
                profiles.values().for_each(collect);
            }
        }
        Ok(names)
    }

    pub(super) fn contains(&self, key: &str) -> bool {
        // When a settings file cannot be inspected, retain the cached installation.
        self.0
            .as_ref()
            .is_none_or(|names| names.contains(&key.to_ascii_lowercase()))
    }
}

pub(super) fn referenced_by_codex(home: &Path, section: &str, key: &str) -> bool {
    CodexRegistrations::read(home, section).contains(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_shared_references_in_named_profiles_and_ignores_private_registrations()
    -> io::Result<()> {
        let home = tempfile::tempdir()?;
        fs::write(
            home.path().join("better-codex.toml"),
            "[plugins.'only@fork']\nenabled = true\n",
        )?;
        fs::write(
            home.path().join("work.config.toml"),
            "[plugins.'shared@team']\nenabled = true\n",
        )?;
        assert!(referenced_by_codex(home.path(), "plugins", "shared@team"));
        assert!(!referenced_by_codex(home.path(), "plugins", "only@fork"));
        Ok(())
    }
}
