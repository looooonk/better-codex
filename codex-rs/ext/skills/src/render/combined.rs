use crate::catalog::SkillCatalog;
use crate::catalog_prompt::HOST_ALIAS_INSTRUCTIONS;
use crate::catalog_prompt::RESOURCE_ALIAS_INSTRUCTIONS;
use crate::fragments::SkillsUsage;

use super::AvailableSkillsRender;
use super::CatalogLines;
use super::RenderedSkillCatalogs;
use super::SkillCatalogRenderPolicy;
use super::SkillMetadataBudget;
use super::aliased_metadata_overhead_cost;
use super::allocation::RenderedSkillLine;
use super::allocation::RenderedSkillLines;
use super::allocation::SkillLine;
use super::allocation::SkillLineAllocation;
use super::allocation::allocate_skill_lines;
use super::allocation::render_allocated_skill_lines;
use super::metadata_line_cost;
use super::omission_marker;
use super::render_available_skills;

pub(crate) fn render_combined_available_skills(
    executor_catalog: &SkillCatalog,
    orchestrator_catalog: &SkillCatalog,
    host_catalog: &SkillCatalog,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> RenderedSkillCatalogs {
    let mut executor_entries = executor_catalog
        .entries
        .iter()
        .filter(|entry| entry.is_model_visible())
        .collect::<Vec<_>>();
    let mut orchestrator_entries = orchestrator_catalog
        .entries
        .iter()
        .filter(|entry| entry.is_model_visible())
        .collect::<Vec<_>>();
    let mut host_entries = host_catalog
        .entries
        .iter()
        .filter(|entry| entry.is_model_visible())
        .collect::<Vec<_>>();
    SkillCatalogRenderPolicy::ExtensionCompatible.order_entries(&mut executor_entries);
    SkillCatalogRenderPolicy::ExtensionCompatible.order_entries(&mut orchestrator_entries);
    SkillCatalogRenderPolicy::CoreCompatible.order_entries(&mut host_entries);
    let nonempty_catalog_count = [
        !executor_entries.is_empty(),
        !orchestrator_entries.is_empty(),
        !host_entries.is_empty(),
    ]
    .into_iter()
    .filter(|nonempty| *nonempty)
    .count();
    if nonempty_catalog_count <= 1 {
        return RenderedSkillCatalogs {
            executor: render_available_skills(
                executor_catalog,
                SkillCatalogRenderPolicy::ExtensionCompatible,
                budget,
                include_skills_usage_instructions,
            ),
            orchestrator: render_available_skills(
                orchestrator_catalog,
                SkillCatalogRenderPolicy::ExtensionCompatible,
                budget,
                include_skills_usage_instructions,
            ),
            host: render_available_skills(
                host_catalog,
                SkillCatalogRenderPolicy::CoreCompatible,
                budget,
                include_skills_usage_instructions,
            ),
        };
    }

    let extension_policy = SkillCatalogRenderPolicy::ExtensionCompatible;
    let host_policy = SkillCatalogRenderPolicy::CoreCompatible;
    let absolute = render_combined_lines(
        CatalogLines::unaliased(&executor_entries, extension_policy),
        CatalogLines::unaliased(&orchestrator_entries, extension_policy),
        CatalogLines::unaliased(&host_entries, host_policy),
        budget,
    );

    let mut selected = absolute;
    if !combined_catalog_fully_rendered(&selected) {
        let host_only_aliases = build_aliased_combined_catalog(
            CatalogLines::unaliased(&executor_entries, extension_policy),
            CatalogLines::unaliased(&orchestrator_entries, extension_policy),
            CatalogLines::aliased(&host_entries, host_policy),
            budget,
            include_skills_usage_instructions,
        );
        let executor_only_aliases = build_aliased_combined_catalog(
            CatalogLines::aliased(&executor_entries, extension_policy),
            CatalogLines::unaliased(&orchestrator_entries, extension_policy),
            CatalogLines::unaliased(&host_entries, host_policy),
            budget,
            include_skills_usage_instructions,
        );
        let orchestrator_only_aliases = build_aliased_combined_catalog(
            CatalogLines::unaliased(&executor_entries, extension_policy),
            CatalogLines::aliased(&orchestrator_entries, extension_policy),
            CatalogLines::unaliased(&host_entries, host_policy),
            budget,
            include_skills_usage_instructions,
        );
        let all_source_aliases = build_aliased_combined_catalog(
            CatalogLines::aliased(&executor_entries, extension_policy),
            CatalogLines::aliased(&orchestrator_entries, extension_policy),
            CatalogLines::aliased(&host_entries, host_policy),
            budget,
            include_skills_usage_instructions,
        );

        for candidate in [
            host_only_aliases,
            executor_only_aliases,
            orchestrator_only_aliases,
            all_source_aliases,
        ]
        .into_iter()
        .flatten()
        {
            if combined_render_is_better(
                &candidate,
                &selected,
                budget,
                include_skills_usage_instructions,
            ) {
                selected = candidate;
            }
        }
    }

    assign_combined_usage(&mut selected, include_skills_usage_instructions);

    RenderedSkillCatalogs {
        executor: Some(selected.executor),
        orchestrator: Some(selected.orchestrator),
        host: Some(selected.host),
    }
}

struct CombinedAvailableSkillsRender {
    executor: AvailableSkillsRender,
    orchestrator: AvailableSkillsRender,
    host: AvailableSkillsRender,
}

fn render_combined_lines(
    executor: CatalogLines<'_>,
    orchestrator: CatalogLines<'_>,
    host: CatalogLines<'_>,
    budget: SkillMetadataBudget,
) -> CombinedAvailableSkillsRender {
    let executor_end = executor.skills.len();
    let orchestrator_end = executor_end.saturating_add(orchestrator.skills.len());
    let mut lines = executor.skills;
    lines.extend(orchestrator.skills);
    lines.extend(host.skills);
    let mut allocations = allocate_skill_lines(&lines, budget);
    let omission_marker = reserve_non_host_omission_marker(
        &lines,
        executor_end,
        orchestrator_end,
        budget,
        &mut allocations,
    );
    let (executor_omission_marker, orchestrator_omission_marker) =
        if executor_end == orchestrator_end {
            (omission_marker, None)
        } else {
            (None, omission_marker)
        };

    CombinedAvailableSkillsRender {
        executor: render_combined_group(
            &lines[..executor_end],
            &allocations[..executor_end],
            executor.prompt_kind,
            executor.root_lines,
            executor_omission_marker,
        ),
        orchestrator: render_combined_group(
            &lines[executor_end..orchestrator_end],
            &allocations[executor_end..orchestrator_end],
            orchestrator.prompt_kind,
            orchestrator.root_lines,
            orchestrator_omission_marker,
        ),
        host: render_combined_group(
            &lines[orchestrator_end..],
            &allocations[orchestrator_end..],
            host.prompt_kind,
            host.root_lines,
            /*omission_marker*/ None,
        ),
    }
}

fn render_combined_group(
    skill_lines: &[SkillLine<'_>],
    allocations: &[SkillLineAllocation],
    prompt_kind: crate::catalog_prompt::SkillPromptKind,
    skill_root_lines: Vec<String>,
    omission_marker: Option<String>,
) -> AvailableSkillsRender {
    let RenderedSkillLines {
        mut lines,
        omitted_count,
        truncated_description_chars,
        truncated_description_count,
    } = render_allocated_skill_lines(skill_lines, allocations);
    if let Some(marker) = omission_marker {
        lines.push(RenderedSkillLine { line: marker });
    }
    AvailableSkillsRender {
        prompt_kind,
        skill_root_lines,
        skill_lines: lines.into_iter().map(|rendered| rendered.line).collect(),
        preserve_empty_fragment: false,
        usage: SkillsUsage::Omit,
        report: super::SkillRenderReport {
            total_count: skill_lines.len(),
            included_count: skill_lines.len().saturating_sub(omitted_count),
            omitted_count,
            truncated_description_chars,
            truncated_description_count,
        },
    }
}

fn build_aliased_combined_catalog(
    executor: CatalogLines<'_>,
    orchestrator: CatalogLines<'_>,
    host: CatalogLines<'_>,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> Option<CombinedAvailableSkillsRender> {
    if [
        &executor.root_lines,
        &orchestrator.root_lines,
        &host.root_lines,
    ]
    .into_iter()
    .all(Vec::is_empty)
    {
        return None;
    }

    let resource_aliases = !executor.root_lines.is_empty() || !orchestrator.root_lines.is_empty();
    let host_aliases = !host.root_lines.is_empty();
    let table_cost = [&executor, &orchestrator, &host]
        .into_iter()
        .filter(|catalog| !catalog.root_lines.is_empty())
        .map(|catalog| {
            aliased_metadata_overhead_cost(budget, catalog.prompt_kind, &catalog.root_lines, false)
        })
        .fold(0usize, usize::saturating_add)
        .saturating_add(combined_alias_instruction_cost(
            budget,
            include_skills_usage_instructions,
            resource_aliases,
            host_aliases,
        ));
    if table_cost >= budget.limit() {
        return None;
    }

    let adjusted_limit = budget.limit().saturating_sub(table_cost);
    let adjusted_budget = match budget {
        SkillMetadataBudget::Tokens(_) => SkillMetadataBudget::Tokens(adjusted_limit),
        SkillMetadataBudget::Characters(_) => SkillMetadataBudget::Characters(adjusted_limit),
    };
    Some(render_combined_lines(
        executor,
        orchestrator,
        host,
        adjusted_budget,
    ))
}

fn combined_catalog_fully_rendered(rendered: &CombinedAvailableSkillsRender) -> bool {
    [&rendered.executor, &rendered.orchestrator, &rendered.host]
        .into_iter()
        .all(|catalog| {
            catalog.report.omitted_count == 0 && catalog.report.truncated_description_chars == 0
        })
}

fn combined_render_is_better(
    candidate: &CombinedAvailableSkillsRender,
    current: &CombinedAvailableSkillsRender,
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
) -> bool {
    let priority = |rendered: &CombinedAvailableSkillsRender| {
        (
            rendered.executor.report.included_count,
            rendered.orchestrator.report.included_count,
            rendered.host.report.included_count,
        )
    };
    if priority(candidate) != priority(current) {
        return priority(candidate) > priority(current);
    }

    let truncated_chars = |rendered: &CombinedAvailableSkillsRender| {
        [&rendered.executor, &rendered.orchestrator, &rendered.host]
            .into_iter()
            .fold(0usize, |total, catalog| {
                total.saturating_add(catalog.report.truncated_description_chars)
            })
    };
    if truncated_chars(candidate) != truncated_chars(current) {
        return truncated_chars(candidate) < truncated_chars(current);
    }

    combined_available_skills_cost(budget, candidate, include_skills_usage_instructions)
        < combined_available_skills_cost(budget, current, include_skills_usage_instructions)
}

fn combined_available_skills_cost(
    budget: SkillMetadataBudget,
    rendered: &CombinedAvailableSkillsRender,
    include_skills_usage_instructions: bool,
) -> usize {
    let catalogs = [&rendered.executor, &rendered.orchestrator, &rendered.host];
    let resource_aliases = !rendered.executor.skill_root_lines.is_empty()
        || !rendered.orchestrator.skill_root_lines.is_empty();
    let host_aliases = !rendered.host.skill_root_lines.is_empty();
    catalogs.into_iter().fold(
        combined_alias_instruction_cost(
            budget,
            include_skills_usage_instructions,
            resource_aliases,
            host_aliases,
        ),
        |used, catalog| {
            let root_cost = if !catalog.skill_root_lines.is_empty() {
                aliased_metadata_overhead_cost(
                    budget,
                    catalog.prompt_kind,
                    &catalog.skill_root_lines,
                    false,
                )
            } else {
                Default::default()
            };
            catalog
                .skill_lines
                .iter()
                .fold(used.saturating_add(root_cost), |used, line| {
                    used.saturating_add(metadata_line_cost(budget, line))
                })
        },
    )
}

fn assign_combined_usage(
    rendered: &mut CombinedAvailableSkillsRender,
    include_skills_usage_instructions: bool,
) {
    if !include_skills_usage_instructions {
        return;
    }

    let usage = SkillsUsage::Combined {
        resource_aliases: !rendered.executor.skill_root_lines.is_empty()
            || !rendered.orchestrator.skill_root_lines.is_empty(),
        host_aliases: !rendered.host.skill_root_lines.is_empty(),
    };
    if let Some(catalog) = [
        &mut rendered.executor,
        &mut rendered.orchestrator,
        &mut rendered.host,
    ]
    .into_iter()
    .find(|catalog| catalog.preserve_empty_fragment || !catalog.skill_lines.is_empty())
    {
        catalog.usage = usage;
    }
}

fn combined_alias_instruction_cost(
    budget: SkillMetadataBudget,
    include_skills_usage_instructions: bool,
    resource_aliases: bool,
    host_aliases: bool,
) -> usize {
    if !include_skills_usage_instructions {
        return 0;
    }

    let resource_cost = if resource_aliases {
        metadata_line_cost(budget, RESOURCE_ALIAS_INSTRUCTIONS)
    } else {
        Default::default()
    };
    let host_cost = if host_aliases {
        metadata_line_cost(budget, HOST_ALIAS_INSTRUCTIONS)
    } else {
        Default::default()
    };
    resource_cost.saturating_add(host_cost)
}

fn reserve_non_host_omission_marker(
    skill_lines: &[SkillLine<'_>],
    executor_end: usize,
    orchestrator_end: usize,
    budget: SkillMetadataBudget,
    allocations: &mut [SkillLineAllocation],
) -> Option<String> {
    loop {
        let omitted_count = allocations[..orchestrator_end]
            .iter()
            .filter(|allocation| matches!(allocation, SkillLineAllocation::Omitted))
            .count();
        if omitted_count == 0 {
            return None;
        }

        let marker = omission_marker(omitted_count);
        let used = allocated_skill_lines_cost(skill_lines, allocations, budget);
        if used.saturating_add(metadata_line_cost(budget, &marker)) <= budget.limit() {
            return Some(marker);
        }

        let index = (orchestrator_end..allocations.len())
            .rev()
            .chain((executor_end..orchestrator_end).rev())
            .chain((0..executor_end).rev())
            .find(|index| {
                matches!(
                    allocations[*index],
                    SkillLineAllocation::DescriptionChars(_)
                )
            })?;
        allocations[index] = SkillLineAllocation::Omitted;
    }
}

fn allocated_skill_lines_cost(
    skill_lines: &[SkillLine<'_>],
    allocations: &[SkillLineAllocation],
    budget: SkillMetadataBudget,
) -> usize {
    skill_lines
        .iter()
        .zip(allocations)
        .fold(0usize, |used, (line, allocation)| match allocation {
            SkillLineAllocation::Omitted => used,
            SkillLineAllocation::DescriptionChars(description_chars) => {
                used.saturating_add(metadata_line_cost(
                    budget,
                    &line.render_with_description_chars(*description_chars),
                ))
            }
        })
}
