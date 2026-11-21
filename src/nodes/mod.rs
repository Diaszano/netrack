pub mod routes;

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::db::tests_repo::SpeedtestRow;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: i64,
    pub name: String,
    pub url: String,
    pub password: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeResponse {
    pub id: i64,
    pub name: String,
    pub url: String,
    #[serde(alias = "has_password")]
    pub has_password: bool,
    #[serde(alias = "created_at")]
    pub created_at: i64,
}

impl From<Node> for NodeResponse {
    fn from(n: Node) -> Self {
        let has_password = n.password.as_ref().is_some_and(|p| !p.trim().is_empty());
        Self {
            id: n.id,
            name: n.name,
            url: n.url,
            has_password,
            created_at: n.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeStatusResponse {
    pub online: bool,
    pub version: Option<String>,
    #[serde(alias = "latest_test")]
    pub latest_test: Option<SpeedtestRow>,
    pub error: Option<String>,
}

const NODE_COLUMNS: &str = "id, name, url, password, created_at";

fn map_node(row: &Row) -> rusqlite::Result<Node> {
    Ok(Node {
        id: row.get(0)?,
        name: row.get(1)?,
        url: row.get(2)?,
        password: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub async fn list(db: &Db) -> anyhow::Result<Vec<Node>> {
    db.call(|conn| {
        let mut stmt =
            conn.prepare(&format!("SELECT {NODE_COLUMNS} FROM nodes ORDER BY id ASC"))?;
        let rows = stmt.query_map([], map_node)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> anyhow::Result<Option<Node>> {
    db.call(move |conn| {
        Ok(conn
            .query_row(
                &format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?1"),
                params![id],
                map_node,
            )
            .optional()?)
    })
    .await
}

pub async fn insert(
    db: &Db,
    name: String,
    url: String,
    password: Option<String>,
) -> anyhow::Result<Node> {
    let now = chrono::Utc::now().timestamp_millis();
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO nodes (name, url, password, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![name, url, password, now],
        )?;
        let id = conn.last_insert_rowid();
        Ok(Node {
            id,
            name,
            url,
            password,
            created_at: now,
        })
    })
    .await
}

pub async fn update(
    db: &Db,
    id: i64,
    new_name: Option<String>,
    new_url: Option<String>,
    new_password: Option<Option<String>>,
) -> anyhow::Result<Option<Node>> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        let current = tx
            .query_row(
                &format!("SELECT {NODE_COLUMNS} FROM nodes WHERE id = ?1"),
                params![id],
                map_node,
            )
            .optional()?;

        let mut node = match current {
            Some(n) => n,
            None => return Ok(None),
        };

        if let Some(name) = new_name {
            node.name = name;
        }
        if let Some(url) = new_url {
            node.url = url;
        }
        if let Some(pwd_opt) = new_password {
            node.password = pwd_opt;
        }

        tx.execute(
            "UPDATE nodes SET name = ?1, url = ?2, password = ?3 WHERE id = ?4",
            params![node.name, node.url, node.password, id],
        )?;
        tx.commit()?;
        Ok(Some(node))
    })
    .await
}

pub async fn delete(db: &Db, id: i64) -> anyhow::Result<bool> {
    db.call(move |conn| {
        let affected = conn.execute("DELETE FROM nodes WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    })
    .await
}
