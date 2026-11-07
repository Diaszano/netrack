pub const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE speedtests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  created_at INTEGER NOT NULL,
  local_hour INTEGER NOT NULL,
  local_weekday INTEGER NOT NULL,
  kind TEXT NOT NULL,
  provider TEXT NOT NULL,
  server_id TEXT, server_name TEXT, server_host TEXT, result_id TEXT,
  ping REAL, jitter REAL, download REAL, upload REAL,
  duration REAL,
  error TEXT
);
CREATE INDEX speedtests_created_at ON speedtests(created_at);
"#,
    r#"
CREATE TABLE sessions (
  token TEXT PRIMARY KEY,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_expires_at ON sessions(expires_at);
"#,
    r#"
CREATE TABLE integrations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  channel_type TEXT NOT NULL,
  config_json TEXT NOT NULL,
  events_json TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 1
);
"#,
    r#"
CREATE TABLE nodes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  url TEXT NOT NULL,
  password TEXT,
  created_at INTEGER NOT NULL
);
"#,
];

use rusqlite::OptionalExtension;

/// Applies pending migrations, tracking progress in `meta.schema_version`.
pub fn run(conn: &mut rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let current: usize = match stored {
        Some(v) => v
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid meta.schema_version: {v:?}"))?,
        None => 0,
    };
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES ('schema_version', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [(i + 1).to_string()],
        )?;
        tx.commit()?;
    }
    Ok(())
}
