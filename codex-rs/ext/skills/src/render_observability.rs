use codex_extension_api::ExtensionMetrics;
use codex_otel::THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_METRIC;
use codex_otel::THREAD_SKILLS_ENABLED_TOTAL_METRIC;
use codex_otel::THREAD_SKILLS_KEPT_TOTAL_METRIC;
use codex_otel::THREAD_SKILLS_TRUNCATED_METRIC;

use crate::render::SkillMetadataBudget;
use crate::render::SkillRenderReport;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CatalogSurface {
    ExecutorWorldState,
    OrchestratorWorldState,
    HostWorldState,
}

impl CatalogSurface {
    fn as_str(self) -> &'static str {
        match self {
            Self::ExecutorWorldState => "executor_world_state",
            Self::OrchestratorWorldState => "orchestrator_world_state",
            Self::HostWorldState => "host_world_state",
        }
    }
}

pub(crate) fn record_catalog_render(
    extension_metrics: Option<&dyn ExtensionMetrics>,
    catalog_surface: CatalogSurface,
    budget: SkillMetadataBudget,
    report: &SkillRenderReport,
) {
    let samples = [
        (
            THREAD_SKILLS_ENABLED_TOTAL_METRIC,
            i64::try_from(report.total_count).unwrap_or(i64::MAX),
        ),
        (
            THREAD_SKILLS_KEPT_TOTAL_METRIC,
            i64::try_from(report.included_count).unwrap_or(i64::MAX),
        ),
        (
            THREAD_SKILLS_TRUNCATED_METRIC,
            if report.omitted_count > 0 { 1 } else { 0 },
        ),
        (
            THREAD_SKILLS_DESCRIPTION_TRUNCATED_CHARS_METRIC,
            i64::try_from(report.truncated_description_chars).unwrap_or(i64::MAX),
        ),
    ];
    if let Some(extension_metrics) = extension_metrics {
        let tags = [("catalog_surface", catalog_surface.as_str())];
        for (name, value) in samples {
            extension_metrics.histogram(name, value, &tags);
        }
    }

    if report.omitted_count > 0 || report.truncated_description_chars > 0 {
        tracing::info!(
            budget_limit = budget.limit(),
            total_skills = report.total_count,
            included_skills = report.included_count,
            omitted_skills = report.omitted_count,
            truncated_description_chars_per_skill = report.average_truncated_description_chars(),
            truncated_skill_descriptions = report.truncated_description_count,
            "truncated skill metadata to fit skills context budget"
        );
    }
}

#[cfg(test)]
#[path = "render_observability_tests.rs"]
mod tests;
