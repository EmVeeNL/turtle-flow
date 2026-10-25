ALTER TABLE provider_limits_observed ADD COLUMN resets_at TEXT;

CREATE TABLE provider_quota_snapshots (
  id TEXT PRIMARY KEY,
  provider TEXT NOT NULL,
  observed_at TEXT NOT NULL,
  window_name TEXT NOT NULL,
  utilization REAL NOT NULL CHECK (utilization >= 0.0 AND utilization <= 1.0),
  resets_at TEXT,
  status TEXT NOT NULL CHECK (status IN ('allowed','warning','rejected')),
  run_id TEXT REFERENCES agent_runs(id)
);
CREATE INDEX ix_provider_quota_latest
  ON provider_quota_snapshots(provider, window_name, observed_at DESC);

-- Keep bounded history per provider/window. Latest observations remain available for routing
-- and CLI display; older samples are not needed for the M3 behavior.
CREATE TRIGGER trg_provider_quota_bound AFTER INSERT ON provider_quota_snapshots
BEGIN
  DELETE FROM provider_quota_snapshots
  WHERE id IN (
    SELECT id FROM provider_quota_snapshots
    WHERE provider = NEW.provider AND window_name = NEW.window_name
    ORDER BY observed_at DESC, rowid DESC LIMIT -1 OFFSET 100
  );
END;
