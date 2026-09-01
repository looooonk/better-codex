use super::*;
use std::collections::HashMap;
use std::sync::Arc;

use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

fn make_skill(name: &str, scope: SkillScope) -> SkillMetadata {
    SkillMetadata {
        name: name.to_string(),
        description: "desc".to_string(),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: test_path_buf(&format!("/tmp/{name}/SKILL.md")).abs(),
        scope,
        plugin_id: None,
    }
}
fn make_skill_with_description(name: &str, scope: SkillScope, description: &str) -> SkillMetadata {
    let mut skill = make_skill(name, scope);
    skill.description = description.to_string();
    skill
}

fn expected_skill_line(skill: &SkillMetadata, description: &str) -> String {
    SkillLine::new(skill).render_with_description(description)
}

fn normalized_path(path: &AbsolutePathBuf) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn outcome_with_roots(skills: Vec<SkillMetadata>, roots: Vec<AbsolutePathBuf>) -> SkillLoadOutcome {
    let skill_root_by_path = skills
        .iter()
        .filter_map(|skill| {
            roots
                .iter()
                .find(|root| {
                    skill
                        .path_to_skills_md
                        .as_path()
                        .starts_with(root.as_path())
                })
                .map(|root| (skill.path_to_skills_md.clone(), root.clone()))
        })
        .collect::<HashMap<_, _>>();
    SkillLoadOutcome {
        skills,
        skill_roots: roots,
        skill_root_by_path: Arc::new(skill_root_by_path),
        ..Default::default()
    }
}

fn build_available_skills_from_metadata(
    skills: &[SkillMetadata],
    budget: SkillMetadataBudget,
) -> Option<AvailableSkills> {
    build_available_skills_from_lines(
        ordered_absolute_skill_lines(skills),
        skills.len(),
        budget,
        SkillPathAliases::default(),
    )
}

#[test]
fn skill_usage_instructions_require_complete_main_agent_reads() {
    for instructions in [
        SKILLS_HOW_TO_USE_WITH_ABSOLUTE_PATHS,
        SKILLS_HOW_TO_USE_WITH_ALIASES,
    ] {
        assert!(instructions.contains("read its `SKILL.md` completely"));
        assert!(instructions.contains("continue until EOF"));
        assert!(instructions.contains(
            "The main agent must read each required instruction or reference file itself"
        ));
        assert!(
            instructions.contains(
                "Do not delegate reading, summarizing, or interpreting skill instructions"
            )
        );
        assert!(
            instructions.contains(
                "Subagents may still perform task work when the selected skill allows it"
            )
        );
        assert!(instructions.contains(
                "Progressive disclosure applies to selecting relevant files, not partially reading a selected instruction file"
            ));
        assert!(!instructions.contains("Read only enough to follow the workflow"));
    }
}

#[test]
fn default_budget_uses_two_percent_of_full_context_window() {
    assert_eq!(
        default_skill_metadata_budget(Some(200_000)),
        SkillMetadataBudget::Tokens(4_000)
    );
    assert_eq!(
        default_skill_metadata_budget(Some(99)),
        SkillMetadataBudget::Tokens(1)
    );
}

#[test]
fn default_budget_falls_back_to_characters_without_context_window() {
    assert_eq!(
        default_skill_metadata_budget(/*context_window*/ None),
        SkillMetadataBudget::Characters(DEFAULT_SKILL_METADATA_CHAR_BUDGET)
    );
    assert_eq!(
        default_skill_metadata_budget(Some(-1)),
        SkillMetadataBudget::Characters(DEFAULT_SKILL_METADATA_CHAR_BUDGET)
    );
}

#[test]
fn default_context_caps_descriptions_without_mutating_metadata() {
    let description = "\u{1F4A1}".repeat(MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS + 1);
    let skill = make_skill_with_description("long-skill", SkillScope::Repo, &description);
    let expected_description = "\u{1F4A1}".repeat(
        MAX_DEFAULT_CONTEXT_SKILL_DESCRIPTION_CHARS
            - TRUNCATED_SKILL_DESCRIPTION_SUFFIX.chars().count(),
    ) + TRUNCATED_SKILL_DESCRIPTION_SUFFIX;

    let rendered = build_available_skills_from_metadata(
        std::slice::from_ref(&skill),
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("skill should render");

    assert_eq!(skill.description, description);
    assert_eq!(
        rendered.skill_lines,
        vec![expected_skill_line(&skill, &expected_description)]
    );
}

#[test]
fn budgeted_rendering_truncates_descriptions_equally_before_omitting_skills() {
    let alpha = make_skill_with_description("alpha-skill", SkillScope::Repo, "abcdef");
    let beta = make_skill_with_description("beta-skill", SkillScope::Repo, "uvwxyz");
    let minimum_cost = SkillLine::new(&alpha)
        .minimum_cost(SkillMetadataBudget::Characters(usize::MAX))
        + SkillLine::new(&beta).minimum_cost(SkillMetadataBudget::Characters(usize::MAX));
    let budget = SkillMetadataBudget::Characters(minimum_cost + 6);

    let rendered = build_available_skills_from_metadata(&[beta.clone(), alpha.clone()], budget)
        .expect("skills should render");

    assert_eq!(rendered.report.included_count, 2);
    assert_eq!(rendered.report.omitted_count, 0);
    assert_eq!(rendered.report.truncated_description_chars, 8);
    assert_eq!(rendered.warning_message, None);
    assert_eq!(
        rendered.skill_lines,
        vec![
            expected_skill_line(&alpha, "ab"),
            expected_skill_line(&beta, "uv"),
        ]
    );
}

#[test]
fn budgeted_rendering_does_not_warn_when_average_description_truncation_is_within_threshold() {
    let alpha = make_skill_with_description("alpha-skill", SkillScope::Repo, "abcdefghij");
    let beta = make_skill_with_description("beta-skill", SkillScope::Repo, "uvwxyzabcd");
    let minimum_cost = SkillLine::new(&alpha)
        .minimum_cost(SkillMetadataBudget::Characters(usize::MAX))
        + SkillLine::new(&beta).minimum_cost(SkillMetadataBudget::Characters(usize::MAX));
    let budget = SkillMetadataBudget::Characters(minimum_cost + 6);

    let rendered =
        build_available_skills_from_metadata(&[alpha, beta], budget).expect("skills should render");

    assert_eq!(rendered.report.included_count, 2);
    assert_eq!(rendered.report.omitted_count, 0);
    assert_eq!(rendered.report.truncated_description_chars, 16);
    assert_eq!(rendered.report.truncated_description_count, 2);
    assert_eq!(rendered.warning_message, None);
}

#[test]
fn budgeted_rendering_warns_when_average_description_truncation_exceeds_threshold() {
    let long_description = "a".repeat(250);
    let long_skill = make_skill_with_description("long-skill", SkillScope::Repo, &long_description);
    let empty_skill = make_skill_with_description("empty-skill", SkillScope::Repo, "");
    let minimum_cost = SkillLine::new(&long_skill)
        .minimum_cost(SkillMetadataBudget::Characters(usize::MAX))
        + SkillLine::new(&empty_skill).minimum_cost(SkillMetadataBudget::Characters(usize::MAX));
    let budget = SkillMetadataBudget::Characters(minimum_cost + 49);

    let rendered = build_available_skills_from_metadata(&[long_skill, empty_skill], budget)
        .expect("skills should render");

    assert_eq!(rendered.report.total_count, 2);
    assert_eq!(rendered.report.included_count, 2);
    assert_eq!(rendered.report.omitted_count, 0);
    assert_eq!(rendered.report.truncated_description_chars, 202);
    assert_eq!(rendered.report.truncated_description_count, 1);
    assert_eq!(
            rendered.warning_message,
            Some(
                "Skill descriptions were shortened to fit the skills context budget. Codex can still see every skill, but some descriptions are shorter. Disable unused skills or plugins to leave more room for the rest."
                    .to_string()
            )
        );
}

#[test]
fn budgeted_rendering_token_budget_truncation_warning_mentions_two_percent() {
    let long_description = "a".repeat(1000);
    let long_skill = make_skill_with_description("long-skill", SkillScope::Repo, &long_description);
    let minimum_cost =
        SkillLine::new(&long_skill).minimum_cost(SkillMetadataBudget::Tokens(usize::MAX));
    let budget = SkillMetadataBudget::Tokens(minimum_cost + 1);

    let rendered =
        build_available_skills_from_metadata(&[long_skill], budget).expect("skills should render");

    assert_eq!(
        rendered.warning_message,
        Some(SKILL_DESCRIPTION_TRUNCATED_WARNING_WITH_PERCENT.to_string())
    );
}

#[test]
fn budgeted_rendering_redistributes_unused_description_budget() {
    let short = make_skill_with_description("short-skill", SkillScope::Repo, "x");
    let long = make_skill_with_description("long-skill", SkillScope::Repo, "abcdefghi");
    let minimum_cost = SkillLine::new(&short)
        .minimum_cost(SkillMetadataBudget::Characters(usize::MAX))
        + SkillLine::new(&long).minimum_cost(SkillMetadataBudget::Characters(usize::MAX));
    let budget = SkillMetadataBudget::Characters(minimum_cost + 11);

    let rendered = build_available_skills_from_metadata(&[short.clone(), long.clone()], budget)
        .expect("skills should render");

    assert_eq!(rendered.report.included_count, 2);
    assert_eq!(rendered.report.omitted_count, 0);
    assert_eq!(rendered.warning_message, None);
    assert_eq!(
        rendered.skill_lines,
        vec![
            expected_skill_line(&long, "abcdefgh"),
            expected_skill_line(&short, "x"),
        ]
    );
}

#[test]
fn budgeted_rendering_preserves_prompt_priority_when_minimum_lines_exceed_budget() {
    let system = make_skill("system-skill", SkillScope::System);
    let user = make_skill("user-skill", SkillScope::User);
    let repo = make_skill("repo-skill", SkillScope::Repo);
    let admin = make_skill("admin-skill", SkillScope::Admin);
    let system_cost = SkillMetadataBudget::Characters(usize::MAX)
        .cost(&format!("{}\n", SkillLine::new(&system).render_minimum()));
    let admin_cost = SkillMetadataBudget::Characters(usize::MAX)
        .cost(&format!("{}\n", SkillLine::new(&admin).render_minimum()));
    let budget = SkillMetadataBudget::Characters(system_cost + admin_cost);

    let rendered = build_available_skills_from_metadata(&[system, user, repo, admin], budget)
        .expect("skills should render");

    assert_eq!(rendered.report.included_count, 2);
    assert_eq!(rendered.report.omitted_count, 2);
    assert_eq!(
            rendered.warning_message,
            Some(
                "Exceeded skills context budget. All skill descriptions were removed and 2 additional skills were not included in the model-visible skills list."
                    .to_string()
            )
        );
    let rendered_text = rendered.skill_lines.join("\n");
    assert!(rendered_text.contains("- system-skill:"));
    assert!(rendered_text.contains("- admin-skill:"));
    assert!(!rendered_text.contains("desc"));
    assert!(!rendered_text.contains("- repo-skill:"));
    assert!(!rendered_text.contains("- user-skill:"));
}

#[test]
fn budgeted_rendering_keeps_scanning_after_oversized_entry() {
    let mut oversized = make_skill("oversized-system-skill", SkillScope::System);
    oversized.description = "desc ".repeat(100);
    let repo = make_skill("repo-skill", SkillScope::Repo);
    let repo_cost = SkillMetadataBudget::Characters(usize::MAX)
        .cost(&format!("{}\n", SkillLine::new(&repo).render_full()));
    let budget = SkillMetadataBudget::Characters(repo_cost);

    let rendered =
        build_available_skills_from_metadata(&[oversized, repo], budget).expect("skills render");

    assert_eq!(rendered.report.included_count, 1);
    assert_eq!(rendered.report.omitted_count, 1);
    assert_eq!(
            rendered.warning_message,
            Some(
                "Exceeded skills context budget. All skill descriptions were removed and 1 additional skill was not included in the model-visible skills list."
                    .to_string()
            )
        );
    let rendered_text = rendered.skill_lines.join("\n");
    assert!(!rendered_text.contains("- oversized-system-skill:"));
    assert!(rendered_text.contains("- repo-skill:"));
}

#[test]
fn outcome_rendering_omits_aliases_when_absolute_plan_has_no_budget_pressure() {
    let root = test_path_buf("/tmp/skills").abs();
    let alpha_path = root.join("alpha/SKILL.md");
    let beta_path = root.join("beta/SKILL.md");
    let outcome = outcome_with_roots(
        vec![
            skill_with_path("alpha-skill", &alpha_path),
            skill_with_path("beta-skill", &beta_path),
        ],
        vec![root],
    );

    let rendered = build_available_skills(
        &outcome,
        SkillMetadataBudget::Characters(usize::MAX),
        SkillRenderSideEffects::None,
    )
    .expect("skills should render");

    assert!(rendered.skill_root_lines.is_empty());
    assert_eq!(rendered.report.included_count, 2);
}

#[test]
fn outcome_rendering_uses_aliases_when_they_allow_more_skills_to_fit() {
    let root = test_path_buf(
            "/Users/xl/.codex/plugins/cache/openai-curated/example/hash1234567890/skills-with-a-very-long-shared-prefix",
        )
        .abs();
    let skills = (0..12)
        .map(|index| {
            let name = format!("shared-root-skill-{index}");
            skill_with_path(&name, &root.join(format!("skill-{index}/SKILL.md")))
        })
        .collect::<Vec<_>>();
    let outcome = outcome_with_roots(skills.clone(), vec![root]);
    let absolute_minimum = skills.iter().fold(0usize, |cost, skill| {
        cost.saturating_add(
            SkillLine::new(skill).minimum_cost(SkillMetadataBudget::Characters(usize::MAX)),
        )
    });
    let plan = build_alias_plan(
        &outcome,
        &skills,
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");
    let alias_minimum = skills.iter().fold(plan.table_cost, |cost, skill| {
        cost.saturating_add(
            SkillLine::with_path(skill, render_skill_path_with_aliases(skill, &plan))
                .minimum_cost(SkillMetadataBudget::Characters(usize::MAX)),
        )
    });
    assert!(
        alias_minimum < absolute_minimum,
        "test fixture should make aliases cheaper"
    );

    let rendered = build_available_skills(
        &outcome,
        SkillMetadataBudget::Characters(alias_minimum),
        SkillRenderSideEffects::None,
    )
    .expect("skills should render");

    assert_eq!(rendered.report.included_count, skills.len());
    assert_eq!(rendered.report.omitted_count, 0);
    assert_eq!(
            rendered.skill_root_lines,
            vec![format!(
                "- `r0` = `{}`",
                normalized_path(
                    &test_path_buf(
                        "/Users/xl/.codex/plugins/cache/openai-curated/example/hash1234567890/skills-with-a-very-long-shared-prefix"
                    )
                    .abs()
                )
            )]
        );
    let rendered_text = rendered.skill_lines.join("\n");
    assert!(rendered_text.contains("r0/skill-0/SKILL.md"));
    assert!(rendered_text.contains("r0/skill-11/SKILL.md"));
}

#[test]
fn outcome_rendering_uses_marketplace_root_for_single_skill_plugin_versions() {
    let github_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/skills").abs();
    let marketplace_root = test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated").abs();
    let github = skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md"));
    let outcome = outcome_with_roots(vec![github.clone()], vec![github_root.clone()]);
    let plan = build_alias_plan(
        &outcome,
        &[github],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");

    assert_eq!(
        plan.aliases.skill_root_lines,
        vec![format!("- `r0` = `{}`", normalized_path(&marketplace_root))]
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md")),
            &plan
        ),
        "r0/github/hash123/skills/gh-fix-ci/SKILL.md"
    );
}

#[test]
fn outcome_rendering_uses_skill_root_for_multiple_skills_in_one_plugin_version() {
    let github_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/skills").abs();
    let fix_ci = skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md"));
    let yeet = skill_with_path("github:yeet", &github_root.join("yeet/SKILL.md"));
    let outcome = outcome_with_roots(
        vec![fix_ci.clone(), yeet.clone()],
        vec![github_root.clone()],
    );
    let plan = build_alias_plan(
        &outcome,
        &[fix_ci, yeet],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");

    assert_eq!(
        plan.aliases.skill_root_lines,
        vec![format!("- `r0` = `{}`", normalized_path(&github_root))]
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md")),
            &plan
        ),
        "r0/gh-fix-ci/SKILL.md"
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:yeet", &github_root.join("yeet/SKILL.md")),
            &plan
        ),
        "r0/yeet/SKILL.md"
    );
}

#[test]
fn outcome_rendering_counts_plugin_version_skills_before_budget_omission() {
    let root = test_path_buf(
            "/Users/xl/.codex/plugins/cache/openai-curated/example/hash1234567890/skills-with-a-very-long-shared-prefix",
        )
        .abs();
    let alpha = skill_with_path("alpha-skill", &root.join("alpha/SKILL.md"));
    let beta = skill_with_path("beta-skill", &root.join("beta/SKILL.md"));
    let outcome = outcome_with_roots(vec![alpha.clone(), beta.clone()], vec![root.clone()]);
    let plan = build_alias_plan(
        &outcome,
        &[alpha.clone(), beta.clone()],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");
    let alpha_cost = SkillMetadataBudget::Characters(usize::MAX).cost(&format!(
        "{}\n",
        SkillLine::with_path(&alpha, render_skill_path_with_aliases(&alpha, &plan))
            .render_minimum()
    ));
    let rendered = build_aliased_available_skills(
        &outcome,
        &[alpha, beta],
        SkillMetadataBudget::Characters(plan.table_cost + alpha_cost),
    )
    .expect("skills should render");

    assert_eq!(rendered.report.included_count, 1);
    assert_eq!(
        rendered.skill_root_lines,
        vec![format!("- `r0` = `{}`", normalized_path(&root))]
    );
    assert_eq!(
        rendered.skill_lines,
        vec!["- alpha-skill: (file: r0/alpha/SKILL.md)"]
    );
}

#[test]
fn outcome_rendering_uses_each_skill_root_for_multiple_roots_in_one_plugin_version() {
    let skills_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/skills").abs();
    let extra_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/extra-skills")
            .abs();
    let fix_ci = skill_with_path("github:gh-fix-ci", &skills_root.join("gh-fix-ci/SKILL.md"));
    let yeet = skill_with_path("github:yeet", &extra_root.join("yeet/SKILL.md"));
    let outcome = outcome_with_roots(
        vec![fix_ci.clone(), yeet.clone()],
        vec![skills_root.clone(), extra_root.clone()],
    );
    let plan = build_alias_plan(
        &outcome,
        &[fix_ci, yeet],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");

    assert_eq!(
        plan.aliases.skill_root_lines,
        vec![
            format!("- `r0` = `{}`", normalized_path(&skills_root)),
            format!("- `r1` = `{}`", normalized_path(&extra_root)),
        ]
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:gh-fix-ci", &skills_root.join("gh-fix-ci/SKILL.md")),
            &plan
        ),
        "r0/gh-fix-ci/SKILL.md"
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:yeet", &extra_root.join("yeet/SKILL.md")),
            &plan
        ),
        "r1/yeet/SKILL.md"
    );
}

#[test]
fn outcome_rendering_extracts_plugin_marketplace_root_for_multiple_plugins() {
    let github_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/skills").abs();
    let slack_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/slack/hash456/skills").abs();
    let marketplace_root = test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated").abs();
    let github = skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md"));
    let slack = skill_with_path(
        "slack:daily-digest",
        &slack_root.join("daily-digest/SKILL.md"),
    );
    let outcome = outcome_with_roots(
        vec![github.clone(), slack.clone()],
        vec![github_root.clone(), slack_root.clone()],
    );
    let plan = build_alias_plan(
        &outcome,
        &[github, slack],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");

    assert_eq!(
        plan.aliases.skill_root_lines,
        vec![format!("- `r0` = `{}`", normalized_path(&marketplace_root))]
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:gh-fix-ci", &github_root.join("gh-fix-ci/SKILL.md")),
            &plan
        ),
        "r0/github/hash123/skills/gh-fix-ci/SKILL.md"
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path(
                "slack:daily-digest",
                &slack_root.join("daily-digest/SKILL.md")
            ),
            &plan
        ),
        "r0/slack/hash456/skills/daily-digest/SKILL.md"
    );
}

#[test]
fn outcome_rendering_uses_one_marketplace_root_for_multiple_plugin_versions() {
    let skills_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash123/skills").abs();
    let extra_root =
        test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated/github/hash456/extra-skills")
            .abs();
    let marketplace_root = test_path_buf("/Users/xl/.codex/plugins/cache/openai-curated").abs();
    let fix_ci = skill_with_path("github:gh-fix-ci", &skills_root.join("gh-fix-ci/SKILL.md"));
    let yeet = skill_with_path("github:yeet", &extra_root.join("yeet/SKILL.md"));
    let outcome = outcome_with_roots(
        vec![fix_ci.clone(), yeet.clone()],
        vec![skills_root.clone(), extra_root.clone()],
    );
    let plan = build_alias_plan(
        &outcome,
        &[fix_ci, yeet],
        SkillMetadataBudget::Characters(usize::MAX),
    )
    .expect("alias plan should build");

    assert_eq!(
        plan.aliases.skill_root_lines,
        vec![format!("- `r0` = `{}`", normalized_path(&marketplace_root))]
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:gh-fix-ci", &skills_root.join("gh-fix-ci/SKILL.md")),
            &plan
        ),
        "r0/github/hash123/skills/gh-fix-ci/SKILL.md"
    );
    assert_eq!(
        render_skill_path_with_aliases(
            &skill_with_path("github:yeet", &extra_root.join("yeet/SKILL.md")),
            &plan
        ),
        "r0/github/hash456/extra-skills/yeet/SKILL.md"
    );
}

fn skill_with_path(name: &str, path: &AbsolutePathBuf) -> SkillMetadata {
    let mut skill = make_skill(name, SkillScope::User);
    skill.path_to_skills_md = path.clone();
    skill
}
