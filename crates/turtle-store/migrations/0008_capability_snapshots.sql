-- Versioned provider capability snapshots. They replace `provider_capabilities`, which v1
-- created but nothing ever wrote. Every statement is idempotent, so a migration resumed under an
-- older schema marker applies cleanly.
DROP TABLE IF EXISTS provider_capabilities;

-- `version` counts snapshots per provider. `content_hash` covers provider, CLI version and
-- capabilities; an identical consecutive snapshot is not stored again.
CREATE TABLE IF NOT EXISTS capability_snapshots (
  id            TEXT PRIMARY KEY,
  provider      TEXT NOT NULL,
  version       INTEGER NOT NULL CHECK (version >= 1),
  cli_version   TEXT,
  capabilities  TEXT NOT NULL CHECK (json_valid(capabilities)),
  content_hash  TEXT NOT NULL,
  discovered_at TEXT NOT NULL,
  UNIQUE (provider, version)
);

-- The snapshot a run started with: its provider's latest one when the run row is written.
CREATE TABLE IF NOT EXISTS run_capability_snapshots (
  run_id      TEXT PRIMARY KEY REFERENCES agent_runs(id),
  snapshot_id TEXT NOT NULL REFERENCES capability_snapshots(id)
);
CREATE INDEX IF NOT EXISTS ix_run_capability_snapshots_snapshot
  ON run_capability_snapshots(snapshot_id);
