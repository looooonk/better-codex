use codex_extension_api::ContextualUserFragment;
use pretty_assertions::assert_eq;

use super::MAX_EXPLICIT_SKILL_PROMPT_BYTES;
use super::SkillInstructions;
use super::SkillResourceAccess;
use crate::tools::SkillToolAuthority;

#[test]
fn bounded_skill_instructions_include_typed_resource_access() {
    let (instructions, truncated) = SkillInstructions::bounded(
        "demo",
        "skill://root/demo/SKILL.md",
        "read references/details.md",
        Some(SkillResourceAccess {
            authority: SkillToolAuthority::Executor {
                id: "root".to_string(),
            },
            package: "skill://root/demo".to_string(),
            main_resource: "skill://root/demo/SKILL.md".to_string(),
        }),
    )
    .expect("bounded instructions");

    assert!(!truncated);
    assert_eq!(
        instructions.render(),
        "<skill>\n<name>demo</name>\n<path>skill://root/demo/SKILL.md</path>\n<resource_access>{\"authority\":{\"kind\":\"executor\",\"id\":\"root\"},\"package\":\"skill://root/demo\",\"main_resource\":\"skill://root/demo/SKILL.md\"}</resource_access>\nread references/details.md\n</skill>"
    );
}

#[test]
fn bounded_skill_instructions_reject_oversized_opaque_metadata() {
    let result = SkillInstructions::bounded(
        "demo",
        "skill://root/demo/SKILL.md",
        "body",
        Some(SkillResourceAccess {
            authority: SkillToolAuthority::Executor {
                id: "root".to_string(),
            },
            package: "p".repeat(MAX_EXPLICIT_SKILL_PROMPT_BYTES),
            main_resource: "skill://root/demo/SKILL.md".to_string(),
        }),
    );

    assert_eq!(result, None);
}
