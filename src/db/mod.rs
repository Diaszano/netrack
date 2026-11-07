pub mod config;
mod migrations;
pub mod tests_repo;

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{Context, anyhow};
use chrono::{Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension};

/// Cheap-to-clone handle to a single SQLite connection.
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
    tz: Tz,
}

/// Local hour (0..23) and weekday (0=Mon..6=Sun) of a unix-ms timestamp in `tz`.
pub fn local_fields(tz: Tz, created_at_ms: i64) -> (u32, u32) {
    let dt = Utc
        .timestamp_millis_opt(created_at_ms)
        .single()
        .unwrap_or_default()
        .with_timezone(&tz);
    (dt.hour(), dt.weekday().num_days_from_monday())
}

impl Db {
    pub async fn open(path: &Path, tz: Tz) -> anyhow::Result<Db> {
        let path = path.to_owned();
        Self::init(move || Connection::open(&path).map_err(Into::into), tz).await
    }

    pub async fn open_in_memory(tz: Tz) -> anyhow::Result<Db> {
        Self::init(|| Connection::open_in_memory().map_err(Into::into), tz).await
    }

    async fn init(
        open: impl FnOnce() -> anyhow::Result<Connection> + Send + 'static,
        tz: Tz,
    ) -> anyhow::Result<Db> {
        let conn = tokio::task::spawn_blocking(move || -> anyhow::Result<Connection> {
            let mut conn = open()?;
            conn.busy_timeout(std::time::Duration::from_millis(5000))?;
            // journal_mode returns a row; in-memory DBs report "memory", which is fine.
            conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))?;
            migrations::run(&mut conn)?;
            sync_tz(&mut conn, tz)?;
            Ok(conn)
        })
        .await
        .context("database init task panicked")??;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
            tz,
        })
    }

    pub fn tz(&self) -> Tz {
        self.tz
    }

    pub async fn call<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&mut Connection) -> anyhow::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn
                .lock()
                .map_err(|_| anyhow!("database mutex poisoned"))?;
            f(&mut guard)
        })
        .await
        .context("database task panicked")?
    }
}

/// Recomputes `local_hour`/`local_weekday` for all rows if the stored tz differs, then stores `tz`.
fn sync_tz(conn: &mut Connection, tz: Tz) -> anyhow::Result<()> {
    let tz_name = tz.name();
    let stored: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'tz'", [], |r| r.get(0))
        .optional()?;
    if stored.as_deref() == Some(tz_name) {
        return Ok(());
    }
    let tx = conn.transaction()?;
    if stored.is_some() {
        let rows: Vec<(i64, i64)> = tx
            .prepare("SELECT id, created_at FROM speedtests")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut update =
            tx.prepare("UPDATE speedtests SET local_hour = ?1, local_weekday = ?2 WHERE id = ?3")?;
        for (id, created_at) in rows {
            let (hour, weekday) = local_fields(tz, created_at);
            update.execute((hour, weekday, id))?;
        }
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('tz', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [tz_name],
    )?;
    tx.commit()?;
    Ok(())
}
