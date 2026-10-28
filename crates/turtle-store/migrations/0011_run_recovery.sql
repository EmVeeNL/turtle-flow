ALTER TABLE agent_runs ADD COLUMN transcript_spool_path TEXT;
ALTER TABLE agent_runs ADD COLUMN cancel_requested INTEGER NOT NULL DEFAULT 0;
CREATE UNIQUE INDEX IF NOT EXISTS ux_usage_run ON usage_records(run_id);
