-- Per-task lookups for the `tasks` table that v1 created but nothing wrote until now. Runs, gate
-- results, commit records and usage already carry `task_id`; these indexes make per-task status
-- cheap. Every statement is idempotent, so a migration resumed under an older schema marker
-- applies cleanly. (0009 is reserved for another change.)
CREATE INDEX IF NOT EXISTS ix_tasks_job_revision ON tasks(job_id, plan_revision_id, ordinal);
CREATE INDEX IF NOT EXISTS ix_runs_task ON agent_runs(task_id) WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_gates_task ON gate_results(task_id, created_at)
  WHERE task_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS ix_commit_records_job_task ON commit_records(job_id, task_id);
