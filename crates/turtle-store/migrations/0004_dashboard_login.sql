CREATE TABLE dashboard_login_codes (
  code_hash TEXT PRIMARY KEY,
  expires_at TEXT NOT NULL,
  used_at TEXT
);
CREATE INDEX ix_dashboard_login_expiry ON dashboard_login_codes(expires_at);

CREATE TABLE api_command_keys (
  key TEXT PRIMARY KEY,
  job_id TEXT NOT NULL REFERENCES jobs(id),
  command TEXT NOT NULL,
  resulting_version INTEGER NOT NULL,
  created_at TEXT NOT NULL
);
CREATE INDEX ix_api_command_keys_created ON api_command_keys(created_at);
