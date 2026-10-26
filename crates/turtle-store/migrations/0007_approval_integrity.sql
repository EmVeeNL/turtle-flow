-- The hmac column exists in v1. Store::open signs pre-v7 rows with the installation key
-- after this migration and before any approval can be read.
CREATE INDEX IF NOT EXISTS ix_approvals_job_gate_status
ON approvals(job_id, gate_kind, status, created_at);
