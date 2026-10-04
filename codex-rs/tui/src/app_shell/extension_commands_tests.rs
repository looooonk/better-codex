use super::*;
use pretty_assertions::assert_eq;

#[test]
fn skill_toggle_preserves_names_and_requires_explicit_state() {
    assert_eq!(
        skill_command("project review on").unwrap(),
        ExtensionCommand::Skill {
            name: "project review".to_string(),
            enabled: true,
        }
    );
    assert!(skill_command("project review maybe").is_err());
    assert!(skill_command("on").is_err());
}

#[test]
fn hook_trust_requires_the_reviewed_hash() {
    assert_eq!(
        hook_command("/workspace".into(), "trust workspace-hook 123abc").unwrap(),
        ExtensionCommand::Hook {
            cwd: "/workspace".into(),
            key: "workspace-hook".to_string(),
            change: HookChange::Trust("123abc".to_string()),
        }
    );
    assert!(hook_command("/workspace".into(), "trust workspace-hook").is_err());
}
