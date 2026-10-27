ALTER TABLE dashboard_login_codes
  ADD COLUMN scope TEXT NOT NULL DEFAULT 'full' CHECK (scope IN ('full', 'read_only'));
