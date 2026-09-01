use codex_extension_api::ContextualUserFragment;
use pretty_assertions::assert_eq;

use crate::catalog::SkillAuthority;
use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillPackageId;
use crate::catalog::SkillResourceId;
use crate::catalog::SkillSourceKind;
use crate::catalog_prompt::HOST_ALIAS_INSTRUCTIONS;
use crate::catalog_prompt::RESOURCE_ALIAS_INSTRUCTIONS;
use crate::catalog_prompt::SkillPromptKind;

use super::SkillCatalogRenderPolicy;
use super::SkillLine;
use super::SkillMetadataBudget;
use super::aliased_metadata_overhead_cost;
use super::available_skills_fragment;
use super::build_alias_plan;
use super::metadata_line_cost;
use super::render_combined_available_skills;
use super::render_extension_catalog;
use super::render_skill_locator_with_aliases;
use super::skill_metadata_budget;

fn entry(
    source: SkillSourceKind,
    authority: &str,
    root: &str,
    name: &str,
    description: &str,
) -> SkillCatalogEntry {
    let package = format!("{root}/{name}");
    SkillCatalogEntry::new(
        SkillPackageId(package.clone()),
        SkillAuthority::new(source, authority),
        name,
        description,
        SkillResourceId::new(format!("{package}/SKILL.md")),
    )
    .with_display_path(format!("{package}/SKILL.md"))
    .with_alias_root(root)
}

#[test]
fn metadata_budget_uses_context_percentage_or_character_fallback() {
    assert_eq!(
        SkillMetadataBudget::Tokens(2_000),
        skill_metadata_budget(Some(100_000))
    );
    assert_eq!(
        SkillMetadataBudget::Characters(8_000),
        skill_metadata_budget(/*context_window*/ None)
    );
}

#[test]
fn unified_host_catalog_uses_h_aliases_only_under_pressure() {
    let root = "/workspace/skills-with-a-long-shared-prefix";
    let catalog = SkillCatalog {
        entries: (0..12)
            .map(|index| {
                entry(
                    SkillSourceKind::Host,
                    "host",
                    root,
                    &format!("skill-{index}"),
                    "A deliberately long description that makes aliases useful under pressure.",
                )
            })
            .collect(),
        warnings: Vec::new(),
    };

    let roomy = available_skills_fragment(
        &catalog,
        false,
        SkillCatalogRenderPolicy::ExtensionCompatible,
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("catalog should render")
    .body();
    assert!(!roomy.contains("### Skill roots"));
    assert!(roomy.contains(&format!("(file: {root}/skill-0/SKILL.md)")));

    let visible_entries = catalog.entries.iter().collect::<Vec<_>>();
    let plan = build_alias_plan(&visible_entries).expect("host aliases should build");
    let table_cost = aliased_metadata_overhead_cost(
        SkillMetadataBudget::Characters(usize::MAX),
        SkillPromptKind::HostAliases,
        &plan.root_lines(),
        true,
    );
    let alias_minimum = visible_entries.iter().fold(table_cost, |cost, entry| {
        cost.saturating_add(
            SkillLine::with_locator(
                entry,
                SkillCatalogRenderPolicy::ExtensionCompatible,
                render_skill_locator_with_aliases(entry, &plan),
            )
            .minimum_cost(SkillMetadataBudget::Characters(usize::MAX)),
        )
    });
    let absolute_minimum = visible_entries.iter().fold(0usize, |cost, entry| {
        cost.saturating_add(
            SkillLine::new(entry, SkillCatalogRenderPolicy::ExtensionCompatible)
                .minimum_cost(SkillMetadataBudget::Characters(usize::MAX)),
        )
    });
    assert!(alias_minimum < absolute_minimum);

    let pressured = available_skills_fragment(
        &catalog,
        true,
        SkillCatalogRenderPolicy::ExtensionCompatible,
        SkillMetadataBudget::Characters(alias_minimum),
    )
    .expect("catalog should render")
    .body();
    assert!(pressured.contains(&format!("- `h0` = `{root}`")));
    assert!(pressured.contains("(file: h0/skill-0/SKILL.md)"));
    assert!(!pressured.contains("- `r0` ="));
}

#[test]
fn combined_catalogs_keep_provider_aliases_distinct() {
    let executor_root = "skill://executor/workspace/skills-with-a-long-prefix";
    let orchestrator_root = "skill://orchestrator/packages-with-a-long-prefix";
    let host_root = "/workspace/host-skills-with-a-long-prefix";
    let catalog = |source, authority: &str, root: &str, prefix: &str| SkillCatalog {
        entries: (0..3)
            .map(|index| {
                entry(
                    source.clone(),
                    authority,
                    root,
                    &format!("{prefix}-{index}"),
                    "A long description that makes compact aliases useful.",
                )
            })
            .collect(),
        warnings: Vec::new(),
    };
    let rendered = render_combined_available_skills(
        &catalog(
            SkillSourceKind::Executor,
            "executor",
            executor_root,
            "executor",
        ),
        &catalog(
            SkillSourceKind::Orchestrator,
            "orchestrator",
            orchestrator_root,
            "orchestrator",
        ),
        &catalog(SkillSourceKind::Host, "host", host_root, "host"),
        SkillMetadataBudget::Characters(900),
        true,
    );

    assert_eq!(
        vec![format!("- `e0` = `{executor_root}`")],
        rendered
            .executor
            .expect("executor catalog should render")
            .skill_root_lines
    );
    assert_eq!(
        vec![format!("- `o0` = `{orchestrator_root}`")],
        rendered
            .orchestrator
            .expect("orchestrator catalog should render")
            .skill_root_lines
    );
    assert_eq!(
        vec![format!("- `h0` = `{host_root}`")],
        rendered
            .host
            .expect("host catalog should render")
            .skill_root_lines
    );
}

#[test]
fn combined_catalogs_emit_one_bounded_usage_block() {
    let executor_root = "skill://executor/workspace/skills-with-a-long-prefix";
    let orchestrator_root = "skill://orchestrator/packages-with-a-long-prefix";
    let host_root = "/workspace/host-skills-with-a-long-prefix";
    let catalog = |source, authority: &str, root: &str, prefix: &str| SkillCatalog {
        entries: (0..3)
            .map(|index| {
                entry(
                    source.clone(),
                    authority,
                    root,
                    &format!("{prefix}-{index}"),
                    "A long description that makes compact aliases useful.",
                )
            })
            .collect(),
        warnings: Vec::new(),
    };
    let executor = catalog(
        SkillSourceKind::Executor,
        "executor",
        executor_root,
        "executor",
    );
    let orchestrator = catalog(
        SkillSourceKind::Orchestrator,
        "orchestrator",
        orchestrator_root,
        "orchestrator",
    );
    let host = catalog(SkillSourceKind::Host, "host", host_root, "host");
    let empty_orchestrator = SkillCatalog::default();
    let budget = SkillMetadataBudget::Characters(900);

    for orchestrator in [&empty_orchestrator, &orchestrator] {
        let rendered =
            render_combined_available_skills(&executor, orchestrator, &host, budget, true);
        let catalogs = [
            rendered.executor.as_ref(),
            rendered.orchestrator.as_ref(),
            rendered.host.as_ref(),
        ];
        let resource_aliases = catalogs[..2]
            .iter()
            .flatten()
            .any(|catalog| !catalog.skill_root_lines.is_empty());
        let host_aliases = catalogs[2].is_some_and(|catalog| !catalog.skill_root_lines.is_empty());
        if !orchestrator.entries.is_empty() {
            assert!(resource_aliases);
            assert!(host_aliases);
        }

        let alias_instruction_cost = resource_aliases
            .then(|| metadata_line_cost(budget, RESOURCE_ALIAS_INSTRUCTIONS))
            .unwrap_or_default()
            .saturating_add(
                host_aliases
                    .then(|| metadata_line_cost(budget, HOST_ALIAS_INSTRUCTIONS))
                    .unwrap_or_default(),
            );
        let metadata_cost =
            catalogs
                .into_iter()
                .flatten()
                .fold(alias_instruction_cost, |used, catalog| {
                    let root_cost = (!catalog.skill_root_lines.is_empty())
                        .then(|| {
                            aliased_metadata_overhead_cost(
                                budget,
                                catalog.prompt_kind,
                                &catalog.skill_root_lines,
                                false,
                            )
                        })
                        .unwrap_or_default();
                    catalog
                        .skill_lines
                        .iter()
                        .fold(used.saturating_add(root_cost), |used, line| {
                            used.saturating_add(metadata_line_cost(budget, line))
                        })
                });
        assert!(metadata_cost <= budget.limit());

        let body = [rendered.executor, rendered.orchestrator, rendered.host]
            .into_iter()
            .flatten()
            .filter_map(|catalog| catalog.into_fragment())
            .map(|fragment| fragment.body())
            .collect::<String>();
        assert_eq!(1, body.matches("### How to use skills").count());
        assert_eq!(
            if resource_aliases { 1 } else { 0 },
            body.matches(RESOURCE_ALIAS_INSTRUCTIONS).count()
        );
        assert_eq!(
            if host_aliases { 1 } else { 0 },
            body.matches(HOST_ALIAS_INSTRUCTIONS).count()
        );
    }
}

#[test]
fn extension_render_reports_bounded_omissions() {
    let catalog = SkillCatalog {
        entries: (0..100)
            .map(|index| {
                entry(
                    SkillSourceKind::Executor,
                    "executor",
                    "skill://executor/skills",
                    &format!("skill-{index}"),
                    &"description".repeat(100),
                )
            })
            .collect(),
        warnings: Vec::new(),
    };

    let (fragment, report) = render_extension_catalog(&catalog, false, Some(1_000));

    assert!(fragment.is_some());
    assert_eq!(100, report.total_count);
    assert!(report.omitted_count > 0);
    assert!(report.warning_message().is_some());
}
