use std::collections::HashMap;
use std::collections::HashSet;

use codex_protocol::protocol::SkillScope;
use codex_protocol::user_input::UserInput;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

use super::*;

#[derive(Default)]
struct TestLookup {
    skills: Vec<SkillMetadata>,
    disabled_paths: HashSet<AbsolutePathBuf>,
    discovery_paths: HashMap<AbsolutePathBuf, AbsolutePathBuf>,
}

impl ExplicitSkillLookup for TestLookup {
    fn skills(&self) -> &[SkillMetadata] {
        &self.skills
    }

    fn disabled_paths(&self) -> &HashSet<AbsolutePathBuf> {
        &self.disabled_paths
    }

    fn skill_discovery_path_for_path(&self, path: &AbsolutePathBuf) -> Option<&AbsolutePathBuf> {
        self.discovery_paths.get(path)
    }
}

fn skill(name: &str, path: &str) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: format!("{name} skill"),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: test_path_buf(path).abs(),
        scope: SkillScope::User,
        plugin_id: None,
    }
}

#[test]
fn structured_mentions_take_priority_and_block_invalid_plain_fallbacks() {
    let alpha = skill("alpha-skill", "/tmp/alpha");
    let beta = skill("beta-skill", "/tmp/beta");
    let lookup = TestLookup {
        skills: vec![alpha.clone(), beta.clone()],
        ..Default::default()
    };
    let selected = collect_explicit_skill_mentions(
        &[
            UserInput::Text {
                text: "$alpha-skill $beta-skill".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "beta-skill".to_string(),
                path: test_path_buf("/tmp/beta"),
            },
        ],
        &lookup,
        &HashMap::new(),
    );
    assert_eq!(selected, vec![beta, alpha]);

    let blocked = collect_explicit_skill_mentions(
        &[
            UserInput::Text {
                text: "$alpha-skill".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "alpha-skill".to_string(),
                path: test_path_buf("/tmp/missing"),
            },
        ],
        &lookup,
        &HashMap::new(),
    );
    assert_eq!(blocked, Vec::new());
}

#[test]
fn discovery_paths_select_canonical_skills_and_honor_disabled_state() {
    let linked = skill("linked", "/tmp/shared/linked/SKILL.md");
    let discovery_path = test_path_buf("/tmp/repo/.agents/skills/linked/SKILL.md").abs();
    let mut lookup = TestLookup {
        skills: vec![linked.clone()],
        discovery_paths: HashMap::from([(
            linked.path_to_skills_md.clone(),
            discovery_path.clone(),
        )]),
        ..Default::default()
    };
    let input = [UserInput::Skill {
        name: "linked".to_string(),
        path: discovery_path.as_path().to_path_buf(),
    }];

    assert_eq!(
        collect_explicit_skill_mentions(&input, &lookup, &HashMap::new()),
        vec![linked.clone()]
    );
    assert_eq!(
        collect_explicit_skill_mentions(
            &[UserInput::Text {
                text: format!("use [$linked]({})", discovery_path.display()),
                text_elements: Vec::new(),
            }],
            &lookup,
            &HashMap::new(),
        ),
        vec![linked.clone()]
    );
    lookup.disabled_paths.insert(linked.path_to_skills_md);
    assert_eq!(
        collect_explicit_skill_mentions(&input, &lookup, &HashMap::new()),
        Vec::new()
    );
}

#[test]
fn plain_names_require_unambiguous_skill_and_connector_identity() {
    let alpha = skill("demo", "/tmp/alpha");
    let beta = skill("demo", "/tmp/beta");
    let input = [UserInput::Text {
        text: "$demo".to_string(),
        text_elements: Vec::new(),
    }];
    let ambiguous = TestLookup {
        skills: vec![alpha.clone(), beta],
        ..Default::default()
    };
    assert_eq!(
        collect_explicit_skill_mentions(&input, &ambiguous, &HashMap::new()),
        Vec::new()
    );

    let unique = TestLookup {
        skills: vec![alpha],
        ..Default::default()
    };
    assert_eq!(
        collect_explicit_skill_mentions(&input, &unique, &HashMap::from([("demo".to_string(), 1)])),
        Vec::new()
    );
}
