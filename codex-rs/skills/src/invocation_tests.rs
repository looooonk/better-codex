use std::collections::HashMap;

use codex_protocol::protocol::SkillScope;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use super::ImplicitSkillLookup;
use super::canonicalize_if_exists;
use super::detect_skill_doc_read;
use super::detect_skill_script_run;
use super::script_run_token;
use crate::SkillMetadata;

#[derive(Default)]
struct TestLookup {
    scripts: HashMap<AbsolutePathBuf, SkillMetadata>,
    docs: HashMap<AbsolutePathBuf, SkillMetadata>,
}

impl ImplicitSkillLookup for TestLookup {
    fn implicit_skill_for_scripts_dir(&self, path: &AbsolutePathBuf) -> Option<&SkillMetadata> {
        self.scripts.get(path)
    }

    fn implicit_skill_for_doc_path(&self, path: &AbsolutePathBuf) -> Option<&SkillMetadata> {
        self.docs.get(path)
    }
}

fn skill(path: AbsolutePathBuf) -> SkillMetadata {
    SkillMetadata {
        name: "test-skill".to_string(),
        description: "test".to_string(),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: path,
        scope: SkillScope::User,
        plugin_id: None,
    }
}

#[test]
fn script_run_detection_requires_runner_and_script_extension() {
    assert_eq!(
        [
            script_run_token(&["python3".into(), "scripts/fetch.py".into()]),
            script_run_token(&["python3".into(), "-c".into(), "print(1)".into()]),
        ],
        [Some("scripts/fetch.py"), None]
    );
}

#[test]
fn skill_doc_read_uses_the_shared_command_parser() {
    let skill_path = test_path_buf("/tmp/skill-test/SKILL.md").abs();
    let normalized = canonicalize_if_exists(&skill_path);
    let lookup = TestLookup {
        docs: HashMap::from([(normalized, skill(skill_path))]),
        ..Default::default()
    };
    let tokens = vec![
        "nl".to_string(),
        "-ba".to_string(),
        test_path_buf("/tmp/skill-test/SKILL.md")
            .display()
            .to_string(),
    ];

    let found = detect_skill_doc_read(&lookup, &tokens, &test_path_buf("/tmp").abs());

    assert_eq!(found.map(|skill| skill.name), Some("test-skill".to_string()));
}

#[test]
fn skill_script_run_resolves_relative_paths_from_workdir() {
    let skill_path = test_path_buf("/tmp/skill-test/SKILL.md").abs();
    let scripts = canonicalize_if_exists(&test_path_buf("/tmp/skill-test/scripts").abs());
    let lookup = TestLookup {
        scripts: HashMap::from([(scripts, skill(skill_path))]),
        ..Default::default()
    };
    let tokens = vec![
        "python3".to_string(),
        "scripts/fetch_comments.py".to_string(),
    ];

    let found =
        detect_skill_script_run(&lookup, &tokens, &test_path_buf("/tmp/skill-test").abs());

    assert_eq!(found.map(|skill| skill.name), Some("test-skill".to_string()));
}
