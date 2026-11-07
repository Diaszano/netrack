pub mod routes;

use rusqlite::params;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

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

#[derive(Debug, Clone, Deserialize, PartialEq)]
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

impl Serialize for NodeResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(7))?;
        map.serialize_entry("id", &self.id)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("url", &self.url)?;
        map.serialize_entry("hasPassword", &self.has_password)?;
        map.serialize_entry("has_password", &self.has_password)?;
        map.serialize_entry("createdAt", &self.created_at)?;
        map.serialize_entry("created_at", &self.created_at)?;
        map.end()
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NodeStatusResponse {
    pub online: bool,
    pub version: Option<String>,
    #[serde(alias = "latest_test")]
    pub latest_test: Option<SpeedtestRow>,
    pub error: Option<String>,
}

impl Serialize for NodeStatusResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry("online", &self.online)?;
        map.serialize_entry("version", &self.version)?;
        map.serialize_entry("latestTest", &self.latest_test)?;
        map.serialize_entry("latest_test", &self.latest_test)?;
        map.serialize_entry("error", &self.error)?;
        map.end()
    }
}

pub async fn list(db: &Db) -> anyhow::Result<Vec<Node>> {
    db.call(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, name, url, password, created_at FROM nodes ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Node {
                id: row.get(0)?,
                name: row.get(1)?,
                url: row.get(2)?,
                password: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> anyhow::Result<Option<Node>> {
    db.call(move |conn| {
        let mut stmt =
            conn.prepare("SELECT id, name, url, password, created_at FROM nodes WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Node {
                id: row.get(0)?,
                name: row.get(1)?,
                url: row.get(2)?,
                password: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
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
        let current: Option<Node> = {
            let mut stmt =
                tx.prepare("SELECT id, name, url, password, created_at FROM nodes WHERE id = ?1")?;
            let mut rows = stmt.query_map(params![id], |row| {
                Ok(Node {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    url: row.get(2)?,
                    password: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?;
            if let Some(r) = rows.next() {
                Some(r?)
            } else {
                None
            }
        };

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
