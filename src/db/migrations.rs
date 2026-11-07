pub const MIGRATIONS: &[&str] = &[r#"
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
"#];

/// Applies pending migrations, tracking progress in `meta.schema_version`.
pub fn run(conn: &mut rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")?;
    let current: usize = conn
        .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
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
