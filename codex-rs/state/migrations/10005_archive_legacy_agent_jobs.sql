ALTER TABLE agent_jobs RENAME TO better_legacy_agent_jobs;
ALTER TABLE agent_job_items RENAME TO better_legacy_agent_job_items;

DROP INDEX IF EXISTS idx_agent_jobs_status;
DROP INDEX IF EXISTS idx_agent_job_items_status;
