use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::NaiveDate;

use super::Db;

const PREFIX: &str = "netrack-";
const SUFFIX: &str = ".db";

/// Writes `<dir>/backups/netrack-<today>.db` with `VACUUM INTO` (skipped if it
/// exists) and prunes all but the newest `keep` backups. `None` when `keep == 0`.
pub async fn run(
    db: &Db,
    dir: &Path,
    today: NaiveDate,
    keep: u32,
) -> anyhow::Result<Option<PathBuf>> {
    if keep == 0 {
        return Ok(None);
    }
    let backups = dir.join("backups");
    tokio::fs::create_dir_all(&backups)
        .await
        .with_context(|| format!("cannot create {}", backups.display()))?;
    let target = backups.join(format!("{PREFIX}{today}{SUFFIX}"));
    if !target.exists() {
        // Write aside and rename so a partial file never counts as a backup.
        let tmp = backups.join(format!("{PREFIX}{today}.tmp"));
        let _ = tokio::fs::remove_file(&tmp).await;
        let tmp_s = tmp.to_string_lossy().into_owned();
        db.call(move |conn| {
            conn.execute("VACUUM INTO ?1", [tmp_s])?;
            Ok(())
        })
        .await
        .context("VACUUM INTO failed")?;
        tokio::fs::rename(&tmp, &target).await?;
    }
    // The backup exists now; a prune failure must not report it as failed.
    if let Err(e) = prune(dir, keep as usize).await {
        tracing::warn!(error = %e, "cannot prune old backups");
    }
    Ok(Some(target))
}

/// True only for names written by [`run`]: `netrack-<YYYY-MM-DD>.db`. Also guards downloads
/// against path traversal, since such a name has no separator.
pub fn is_backup_name(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|n| n.strip_suffix(SUFFIX))
        .is_some_and(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
}

/// Backups in `<dir>/backups` as (file name, size in bytes), newest first. Empty if none.
pub async fn list(dir: &Path) -> anyhow::Result<Vec<(String, u64)>> {
    let mut out = Vec::new();
    let mut rd = match tokio::fs::read_dir(dir.join("backups")).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        if is_backup_name(&name) {
            out.push((name, e.metadata().await?.len()));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(out)
}

/// Deletes all but the newest `keep` files named exactly `netrack-<date>.db`.
async fn prune(dir: &Path, keep: usize) -> anyhow::Result<()> {
    for (name, _) in list(dir).await?.iter().skip(keep) {
        tokio::fs::remove_file(dir.join("backups").join(name)).await?;
    }
    Ok(())
}
