use codex_protocol::user_input::UserInput;
use pretty_assertions::assert_eq;

use super::*;
use crate::catalog::SkillResourceId;
use crate::catalog::SkillSourceKind;

fn entry(source: SkillSourceKind, authority: &str, package: &str) -> SkillCatalogEntry {
    SkillCatalogEntry::new(
        SkillPackageId(package.to_string()),
        SkillAuthority::new(source, authority),
        "demo",
        "Demo skill.",
        SkillResourceId::new(format!("{package}/SKILL.md")),
    )
}

#[test]
fn plain_names_fail_closed_across_provider_authorities() {
    let executor = entry(
        SkillSourceKind::Executor,
        "environment",
        "skill://environment/demo",
    );
    let host = entry(SkillSourceKind::Host, "host", "/skills/demo");
    let catalog = SkillCatalog {
        entries: vec![executor.clone(), host],
        warnings: Vec::new(),
    };

    assert_eq!(
        collect_explicit_skill_mentions(
            &[UserInput::Text {
                text: "$demo".to_string(),
                text_elements: Vec::new(),
            }],
            &catalog,
        ),
        Vec::new()
    );
    assert_eq!(
        collect_explicit_skill_mentions(
            &[UserInput::Mention {
                name: "demo".to_string(),
                path: "skill://environment/demo/SKILL.md".to_string(),
            }],
            &catalog,
        ),
        vec![executor]
    );
}
