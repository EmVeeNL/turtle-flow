-- One calibration update per run, including runs without an applicable budget.
CREATE TABLE estimation_observations (
  run_id TEXT PRIMARY KEY REFERENCES agent_runs(id),
  observed_at TEXT NOT NULL
);
