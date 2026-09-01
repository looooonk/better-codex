use pretty_assertions::assert_eq;

use crate::catalog::SkillAuthority;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillPackageId;
use crate::catalog::SkillResourceId;
use crate::catalog::SkillSourceKind;

use super::AliasPlan;
use super::build_catalog_alias_plan;

#[test]
fn assigns_aliases_in_first_seen_order() {
    let roots = [
        "/skills/beta",
        "/skills/alpha",
        "/skills/beta",
        "/skills/alpha",
    ];
    let plan = AliasPlan::build("r", &roots).expect("alias plan should build");

    assert_eq!(
        vec!["- `r0` = `/skills/beta`", "- `r1` = `/skills/alpha`"],
        plan.root_lines()
    );
}

#[test]
fn reuses_aliases_for_duplicate_roots() {
    let root = "/skills/shared";
    let plan = AliasPlan::build("r", &[root, root, root]).expect("alias plan should build");

    assert_eq!(vec!["- `r0` = `/skills/shared`"], plan.root_lines());
    assert_eq!(
        Some("r0/alpha/SKILL.md".to_string()),
        plan.shorten("/skills/shared/alpha/SKILL.md")
    );
    assert_eq!(
        Some("r0/beta/SKILL.md".to_string()),
        plan.shorten("/skills/shared/beta/SKILL.md")
    );
}

#[test]
fn shortens_filesystem_and_skill_resource_locators() {
    for (prefix, root, locator, expected) in [
        (
            "r",
            "/skills/shared",
            "/skills/shared/alpha/SKILL.md",
            "r0/alpha/SKILL.md",
        ),
        (
            "e",
            "skill://executor/workspace/skills",
            "skill://executor/workspace/skills/alpha/SKILL.md",
            "e0/alpha/SKILL.md",
        ),
        ("o", "skill://plugin", "skill://plugin/alpha", "o0/alpha"),
    ] {
        let plan = AliasPlan::build(prefix, &[root, root]).expect("alias plan should build");

        assert_eq!(Some(expected.to_string()), plan.shorten(locator));
    }
}

#[test]
fn rejects_unknown_roots_and_locators_outside_the_root() {
    let root = "/skills/shared";
    let plan = AliasPlan::build("r", &[root, root]).expect("alias plan should build");

    assert_eq!(None, plan.shorten("/skills/other/alpha"));
    assert_eq!(None, plan.shorten("/skills/shared-other/alpha"));
    assert_eq!(None, plan.shorten(root));
}

#[test]
fn returns_none_when_no_roots_are_provided() {
    assert!(AliasPlan::build("r", &[]).is_none());
}

#[test]
fn aliases_roots_used_by_only_one_skill() {
    let root = "/skills/singleton";
    let plan = AliasPlan::build("r", &[root]).expect("singleton root should produce an alias");

    assert_eq!(vec!["- `r0` = `/skills/singleton`"], plan.root_lines());
    assert_eq!(
        Some("r0/alpha/SKILL.md".to_string()),
        plan.shorten("/skills/singleton/alpha/SKILL.md")
    );
}

fn catalog_entry(
    source: SkillSourceKind,
    authority: &str,
    package: &str,
    root: &str,
) -> SkillCatalogEntry {
    SkillCatalogEntry::new(
        SkillPackageId(package.to_string()),
        SkillAuthority::new(source, authority),
        "demo",
        "Demo skill.",
        SkillResourceId::new(format!("{package}/SKILL.md")),
    )
    .with_alias_root(root)
}

#[test]
fn unified_catalog_aliases_use_distinct_provider_namespaces() {
    let host = catalog_entry(
        SkillSourceKind::Host,
        "host",
        "/skills/host/demo",
        "/skills/host",
    );
    let executor = catalog_entry(
        SkillSourceKind::Executor,
        "executor",
        "skill://executor/skills/demo",
        "skill://executor/skills",
    );
    let orchestrator = catalog_entry(
        SkillSourceKind::Orchestrator,
        "orchestrator",
        "skill://orchestrator/demo",
        "skill://orchestrator",
    );

    let host_plan = build_catalog_alias_plan(&[&host]).expect("host aliases should build");
    let executor_plan =
        build_catalog_alias_plan(&[&executor]).expect("executor aliases should build");
    let orchestrator_plan =
        build_catalog_alias_plan(&[&orchestrator]).expect("orchestrator aliases should build");

    assert_eq!(vec!["- `h0` = `/skills/host`"], host_plan.root_lines());
    assert_eq!(
        vec!["- `e0` = `skill://executor/skills`"],
        executor_plan.root_lines()
    );
    assert_eq!(
        vec!["- `o0` = `skill://orchestrator`"],
        orchestrator_plan.root_lines()
    );
}

#[test]
fn catalog_aliases_do_not_change_legacy_r_namespace() {
    let legacy = AliasPlan::build("r", &["/skills/legacy"]).expect("legacy aliases should build");
    let host = catalog_entry(
        SkillSourceKind::Host,
        "host",
        "/skills/host/demo",
        "/skills/host",
    );
    let catalog = build_catalog_alias_plan(&[&host]).expect("catalog aliases should build");

    assert_eq!(vec!["- `r0` = `/skills/legacy`"], legacy.root_lines());
    assert_eq!(vec!["- `h0` = `/skills/host`"], catalog.root_lines());
}

#[test]
fn host_marketplace_aliases_use_shared_or_version_roots() {
    let marketplace = "/Users/test/.codex/plugins/cache/openai-curated";
    let version_root = format!("{marketplace}/example/1.0.0/skills");
    let first = catalog_entry(
        SkillSourceKind::Host,
        "host",
        &format!("{version_root}/first"),
        &version_root,
    );
    let second = catalog_entry(
        SkillSourceKind::Host,
        "host",
        &format!("{version_root}/second"),
        &version_root,
    );

    let single = build_catalog_alias_plan(&[&first]).expect("single host alias should build");
    let multiple = build_catalog_alias_plan(&[&first, &second]).expect("host aliases should build");

    assert_eq!(
        vec![format!("- `h0` = `{marketplace}`")],
        single.root_lines()
    );
    assert_eq!(
        vec![format!("- `h0` = `{version_root}`")],
        multiple.root_lines()
    );
}
