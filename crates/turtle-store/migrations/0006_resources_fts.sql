CREATE INDEX ix_resource_versions_resource ON resource_versions(resource_id, version);
CREATE INDEX ix_resource_attachments_scope ON resource_attachments(scope_kind, scope_id);
CREATE TRIGGER chunks_fts_ai AFTER INSERT ON chunks BEGIN
  INSERT INTO chunks_fts(rowid, text, heading_path) VALUES (new.rowid, new.text, new.heading_path);
END;
CREATE TRIGGER chunks_fts_ad AFTER DELETE ON chunks BEGIN
  INSERT INTO chunks_fts(chunks_fts, rowid, text, heading_path) VALUES ('delete', old.rowid, old.text, old.heading_path);
END;
INSERT INTO chunks_fts(chunks_fts) VALUES ('rebuild');
