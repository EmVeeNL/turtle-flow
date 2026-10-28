//! Forward-only migrations (`docs/DATA-MODEL.md` §6). `PRAGMA user_version` holds the schema
//! generation; a database from a newer build is refused, and an existing database is backed up
//! with `VACUUM INTO` before any migration runs.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{Result, StoreError};

pub(crate) struct Migration {
    pub(crate) version: u32,
    pub(crate) sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/0002_estimation_observations.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../migrations/0003_provider_quota.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("../migrations/0004_dashboard_login.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("../migrations/0005_dashboard_scope.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("../migrations/0006_resources_fts.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("../migrations/0007_approval_integrity.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("../migrations/0008_capability_snapshots.sql"),
    },
    // Version 9 is reserved for another in-flight change; versions only need to increase.
    Migration {
        version: 10,
        sql: include_str!("../migrations/0010_task_links.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("../migrations/0011_run_recovery.sql"),
    },
];

/// Newest schema generation this build understands.
#[must_use]
pub fn latest_version() -> u32 {
    MIGRATIONS.last().map_or(0, |m| m.version)
}

pub fn current_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

/// Applies pending migrations. `backup_dir` is where the pre-migration backup of an existing
/// (non-empty) database goes; `None` skips backups (in-memory databases).
pub fn migrate(conn: &mut Connection, backup_dir: Option<&Path>, stamp: &str) -> Result<()> {
    migrate_with(conn, MIGRATIONS, backup_dir, stamp)
}

pub(crate) fn migrate_with(
    conn: &mut Connection,
    migrations: &[Migration],
    backup_dir: Option<&Path>,
    stamp: &str,
) -> Result<()> {
    let found = current_version(conn)?;
    let supported = migrations.last().map_or(0, |m| m.version);
    if found > supported {
        return Err(StoreError::SchemaTooNew { found, supported });
    }
    if found == supported {
        return Ok(());
    }
    if found > 0
        && let Some(dir) = backup_dir
    {
        std::fs::create_dir_all(dir)?;
        let target = dir.join(format!("turtle-pre-v{supported}-{stamp}.db"));
        conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    }
    for m in migrations.iter().filter(|m| m.version > found) {
        let tx = conn.transaction()?;
        tx.execute_batch(m.sql)?;
        // `user_version` cannot be bound as a parameter; the value is a trusted constant.
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_empty_database_and_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate(&mut conn, None, "t").expect("first");
        assert_eq!(current_version(&conn).expect("version"), latest_version());
        migrate(&mut conn, None, "t").expect("second is a no-op");
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('jobs','events','effects','store_meta')",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(tables, 4);
    }

    #[test]
    fn upgrading_v1_preserves_estimation_buckets() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate_with(&mut conn, &MIGRATIONS[..1], None, "t").expect("v1");
        conn.execute(
            "INSERT INTO estimation_buckets(bucket_key,level,n,mean_ln,m2_ln,reservoir,updated_at)
            VALUES ('r:implementer|c:S',3,1,7.0,0.0,'[1200]','2026-01-01')",
            [],
        )
        .expect("bucket");
        migrate(&mut conn, None, "t").expect("v2");
        let count: i64 = conn
            .query_row(
                "SELECT n FROM estimation_buckets WHERE bucket_key='r:implementer|c:S'",
                [],
                |r| r.get(0),
            )
            .expect("bucket retained");
        assert_eq!(count, 1);
        let observations: i64 = conn
            .query_row("SELECT count(*) FROM estimation_observations", [], |r| {
                r.get(0)
            })
            .expect("new table");
        assert_eq!(observations, 0);
    }

    #[test]
    fn upgrading_v2_adds_provider_limit_reset_and_quota_history() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate_with(&mut conn, &MIGRATIONS[..2], None, "t").expect("v2");
        assert_eq!(current_version(&conn).expect("version"), 2);
        migrate(&mut conn, None, "t").expect("v3");
        let reset_column: i64 = conn.query_row(
            "SELECT count(*) FROM pragma_table_info('provider_limits_observed') WHERE name='resets_at'",
            [], |row| row.get(0),
        ).expect("column");
        let snapshots: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='provider_quota_snapshots'",
            [], |row| row.get(0),
        ).expect("table");
        assert_eq!(reset_column, 1);
        assert_eq!(snapshots, 1);
    }

    #[test]
    fn upgrading_v3_adds_one_use_dashboard_codes_with_scopes() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate_with(&mut conn, &MIGRATIONS[..3], None, "t").expect("v3");
        assert_eq!(current_version(&conn).expect("version"), 3);
        migrate(&mut conn, None, "t").expect("latest dashboard schema");
        let tables: i64=conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('dashboard_login_codes','api_command_keys')",
            [], |r|r.get(0)
        ).expect("tables");
        assert_eq!(tables, 2);
        let scope_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('dashboard_login_codes') WHERE name='scope'",
                [],
                |r| r.get(0),
            )
            .expect("scope column");
        assert_eq!(scope_column, 1);
        assert_eq!(current_version(&conn).expect("version"), latest_version());
    }

    #[test]
    fn upgrading_v7_replaces_unused_provider_capabilities_and_can_resume() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate_with(&mut conn, &MIGRATIONS[..7], None, "t").expect("v7");
        migrate_with(&mut conn, &MIGRATIONS[..8], None, "t").expect("v8");
        let tables = |conn: &Connection| -> Vec<String> {
            conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN
                 ('provider_capabilities','capability_snapshots','run_capability_snapshots')
                 ORDER BY name",
            )
            .expect("query")
            .query_map([], |r| r.get(0))
            .expect("rows")
            .collect::<std::result::Result<_, _>>()
            .expect("names")
        };
        assert_eq!(
            tables(&conn),
            ["capability_snapshots", "run_capability_snapshots"]
        );
        // A marker left behind by an interrupted upgrade re-runs the migration harmlessly.
        conn.pragma_update(None, "user_version", 7).expect("marker");
        migrate_with(&mut conn, &MIGRATIONS[..8], None, "t").expect("resume");
        migrate(&mut conn, None, "t").expect("v9");
        assert_eq!(current_version(&conn).expect("version"), latest_version());
        assert_eq!(tables(&conn).len(), 2);
    }

    #[test]
    fn upgrading_v8_adds_task_indexes_and_can_resume() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate_with(&mut conn, &MIGRATIONS[..8], None, "t").expect("v8");
        migrate(&mut conn, None, "t").expect("latest");
        let indexes = |conn: &Connection| -> i64 {
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name IN
                 ('ix_tasks_job_revision','ix_runs_task','ix_gates_task','ix_commit_records_job_task')",
                [],
                |r| r.get(0),
            )
            .expect("count")
        };
        assert_eq!(indexes(&conn), 4);
        // Replay only 0010: 0011 adds columns, which SQLite cannot add twice.
        conn.pragma_update(None, "user_version", 8).expect("marker");
        migrate_with(&mut conn, &MIGRATIONS[..9], None, "t").expect("resume");
        assert_eq!(indexes(&conn), 4);
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().expect("open");
        conn.pragma_update(None, "user_version", latest_version() + 1)
            .expect("set");
        let err = migrate(&mut conn, None, "t").expect_err("too new");
        assert!(matches!(err, StoreError::SchemaTooNew { .. }));
    }

    #[test]
    fn backs_up_an_existing_database_before_migrating() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("turtle.db");
        let v1 = [Migration {
            version: 1,
            sql: "CREATE TABLE notes(text); INSERT INTO notes VALUES ('keep me');",
        }];
        let v2 = [
            Migration {
                version: 1,
                sql: v1[0].sql,
            },
            Migration {
                version: 2,
                sql: "ALTER TABLE notes ADD COLUMN pinned INTEGER DEFAULT 0;",
            },
        ];
        let mut conn = Connection::open(&db).expect("open");
        migrate_with(&mut conn, &v1, Some(&dir.path().join("backups")), "s1").expect("v1");
        assert!(
            !dir.path().join("backups").exists(),
            "fresh database needs no backup"
        );

        migrate_with(&mut conn, &v2, Some(&dir.path().join("backups")), "s2").expect("v2");
        assert_eq!(current_version(&conn).expect("version"), 2);

        let backup = dir.path().join("backups/turtle-pre-v2-s2.db");
        let old = Connection::open(&backup).expect("backup opens");
        assert_eq!(
            current_version(&old).expect("version"),
            1,
            "backup is the pre-migration state"
        );
        let text: String = old
            .query_row("SELECT text FROM notes", [], |r| r.get(0))
            .expect("data");
        assert_eq!(text, "keep me");
    }
}
