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
    let dir = dir.join("backups");
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("cannot create {}", dir.display()))?;
    let target = dir.join(format!("{PREFIX}{today}{SUFFIX}"));
    if !target.exists() {
        // Write aside and rename so a partial file never counts as a backup.
        let tmp = dir.join(format!("{PREFIX}{today}.tmp"));
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
    let mut names = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await?;
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(PREFIX) && name.ends_with(SUFFIX) {
            names.push(name);
        }
    }
    names.sort();
    let excess = names.len().saturating_sub(keep as usize);
    for name in &names[..excess] {
        tokio::fs::remove_file(dir.join(name)).await?;
    }
    Ok(Some(target))
}
