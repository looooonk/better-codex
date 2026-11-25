//! The Cloud scenarios supply the available installations. Keep reads on the real
//! named executor without also advertising skills installed on the developer's machine.

use std::sync::Arc;

use codex_exec_server::EnvironmentManager;
use codex_skills_extension::ExecutorSkillProvider;
use codex_skills_extension::SkillProvider;
use codex_skills_extension::catalog::SkillCatalog;
use codex_skills_extension::catalog::SkillReadResult;
use codex_skills_extension::catalog::SkillSearchResult;
use codex_skills_extension::provider::SkillListQuery;
use codex_skills_extension::provider::SkillProviderFuture;
use codex_skills_extension::provider::SkillReadRequest;
use codex_skills_extension::provider::SkillSearchRequest;

pub(super) fn empty_catalog() -> Arc<dyn SkillProvider> {
    Arc::new(EmptyExecutorCatalog(
        ExecutorSkillProvider::new_with_restriction_product(
            Arc::new(EnvironmentManager::default_for_tests()),
            /*restriction_product*/ None,
        ),
    ))
}

struct EmptyExecutorCatalog(ExecutorSkillProvider);

impl SkillProvider for EmptyExecutorCatalog {
    fn list(&self, _query: SkillListQuery) -> SkillProviderFuture<'_, SkillCatalog> {
        Box::pin(async { Ok(SkillCatalog::default()) })
    }

    fn read<'a>(
        &'a self,
        request: SkillReadRequest<'a>,
    ) -> SkillProviderFuture<'a, SkillReadResult> {
        self.0.read(request)
    }

    fn search(&self, request: SkillSearchRequest) -> SkillProviderFuture<'_, SkillSearchResult> {
        self.0.search(request)
    }
}
