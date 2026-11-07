use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Cloudflare,
    #[serde(rename = "librespeed")]
    LibreSpeed,
    Ookla,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExpectedMode {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PasswordMode {
    #[default]
    None,
    Public,
    Protected,
}

impl PasswordMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PasswordMode::None => "none",
            PasswordMode::Public => "public",
            PasswordMode::Protected => "protected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub provider: Provider,
    pub cron: String,
    pub schedule_offset: bool,
    pub retention_days: u32,
    pub expected_mode: ExpectedMode,
    pub expected_ping: f64,
    pub expected_download: f64,
    pub expected_upload: f64,
    pub libre_server_id: Option<u32>,
    pub libre_url: Option<String>,
    pub interface: Option<String>,
    pub paused: bool,
    pub password_mode: PasswordMode,
    #[serde(skip)]
    pub password_hash: String,
    pub ookla_license_agreed: bool,
    pub ookla_server_id: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("unknown config key: {0}")]
    UnknownKey(String),
    #[error("invalid value for {key}: {reason}")]
    Invalid { key: String, reason: String },
    #[error(transparent)]
    Db(#[from] anyhow::Error),
}

pub const DEFAULTS: &[(&str, &str)] = &[
    ("provider", "cloudflare"),
    ("cron", "0 * * * *"),
    ("schedule_offset", "true"),
    ("retention_days", "365"),
    ("expected_mode", "auto"),
    ("expected_ping", "25"),
    ("expected_download", "100"),
    ("expected_upload", "50"),
    ("libre_server_id", "auto"),
    ("libre_url", "none"),
    ("interface", "auto"),
    ("paused", "false"),
    ("password_mode", "none"),
    ("password_hash", ""),
    ("ookla_license_agreed", "false"),
    ("ookla_server_id", "auto"),
];

fn invalid(key: &str, reason: impl Into<String>) -> ConfigError {
    ConfigError::Invalid {
        key: key.to_owned(),
        reason: reason.into(),
    }
}

fn parse_bool(key: &str, v: &str) -> Result<bool, ConfigError> {
    match v {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(invalid(key, "expected true or false")),
    }
}

fn parse_number(key: &str, v: &str) -> Result<f64, ConfigError> {
    match v.parse::<f64>() {
        Ok(n) if n.is_finite() && n >= 0.0 => Ok(n),
        _ => Err(invalid(key, "expected a number >= 0")),
    }
}

fn parse_provider(key: &str, v: &str) -> Result<Provider, ConfigError> {
    match v {
        "cloudflare" => Ok(Provider::Cloudflare),
        "librespeed" => Ok(Provider::LibreSpeed),
        "ookla" => Ok(Provider::Ookla),
        _ => Err(invalid(key, "expected cloudflare, librespeed or ookla")),
    }
}

fn parse_mode(key: &str, v: &str) -> Result<ExpectedMode, ConfigError> {
    match v {
        "auto" => Ok(ExpectedMode::Auto),
        "manual" => Ok(ExpectedMode::Manual),
        _ => Err(invalid(key, "expected auto or manual")),
    }
}

fn parse_retention(key: &str, v: &str) -> Result<u32, ConfigError> {
    match v.parse::<u32>() {
        Ok(n) if (1..=10000).contains(&n) => Ok(n),
        _ => Err(invalid(key, "expected an integer between 1 and 10000")),
    }
}

fn parse_server_id(key: &str, v: &str) -> Result<Option<u32>, ConfigError> {
    if v == "auto" {
        return Ok(None);
    }
    v.parse::<u32>()
        .map(Some)
        .map_err(|_| invalid(key, "expected auto or an integer"))
}

fn parse_url(key: &str, v: &str) -> Result<Option<String>, ConfigError> {
    if v == "none" {
        return Ok(None);
    }
    if v.starts_with("http://") || v.starts_with("https://") {
        Ok(Some(v.to_owned()))
    } else {
        Err(invalid(key, "expected none or an http(s):// URL"))
    }
}

fn parse_interface(v: &str) -> Option<String> {
    (v != "auto" && !v.is_empty()).then(|| v.to_owned())
}

fn parse_password_mode(key: &str, v: &str) -> Result<PasswordMode, ConfigError> {
    match v {
        "none" => Ok(PasswordMode::None),
        "public" => Ok(PasswordMode::Public),
        "protected" => Ok(PasswordMode::Protected),
        _ => Err(invalid(key, "expected none, public, or protected")),
    }
}

fn validate_cron(key: &str, v: &str) -> Result<(), ConfigError> {
    v.parse::<croner::Cron>()
        .map(|_| ())
        .map_err(|e| invalid(key, e.to_string()))
}

/// Validates a single key/value pair.
fn validate(key: &str, v: &str) -> Result<(), ConfigError> {
    match key {
        "provider" => parse_provider(key, v).map(|_| ()),
        "cron" => validate_cron(key, v),
        "schedule_offset" | "paused" => parse_bool(key, v).map(|_| ()),
        "retention_days" => parse_retention(key, v).map(|_| ()),
        "expected_mode" => parse_mode(key, v).map(|_| ()),
        "expected_ping" | "expected_download" | "expected_upload" => {
            parse_number(key, v).map(|_| ())
        }
        "libre_server_id" => parse_server_id(key, v).map(|_| ()),
        "libre_url" => parse_url(key, v).map(|_| ()),
        "interface" => Ok(()),
        "ookla_license_agreed" => parse_bool(key, v).map(|_| ()),
        "ookla_server_id" => parse_server_id(key, v).map(|_| ()),
        "password_mode" => Err(invalid(
            key,
            "cannot set password_mode directly; use /api/auth/password",
        )),
        "password_hash" => Err(invalid(
            key,
            "cannot set password_hash directly; use /api/auth/password",
        )),
        _ => Err(ConfigError::UnknownKey(key.to_owned())),
    }
}

pub async fn load(db: &Db) -> anyhow::Result<AppConfig> {
    let stored: HashMap<String, String> = db
        .call(|conn| {
            let rows = conn
                .prepare("SELECT key, value FROM config")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .await?;
    let get = |key: &str| -> &str {
        stored
            .get(key)
            .map(String::as_str)
            .or_else(|| DEFAULTS.iter().find(|(k, _)| *k == key).map(|(_, d)| *d))
            .expect("every config key has a default")
    };
    let cfg = (|| -> Result<AppConfig, ConfigError> {
        Ok(AppConfig {
            provider: parse_provider("provider", get("provider"))?,
            cron: get("cron").to_owned(),
            schedule_offset: parse_bool("schedule_offset", get("schedule_offset"))?,
            retention_days: parse_retention("retention_days", get("retention_days"))?,
            expected_mode: parse_mode("expected_mode", get("expected_mode"))?,
            expected_ping: parse_number("expected_ping", get("expected_ping"))?,
            expected_download: parse_number("expected_download", get("expected_download"))?,
            expected_upload: parse_number("expected_upload", get("expected_upload"))?,
            libre_server_id: parse_server_id("libre_server_id", get("libre_server_id"))?,
            libre_url: parse_url("libre_url", get("libre_url"))?,
            interface: parse_interface(get("interface")),
            paused: parse_bool("paused", get("paused"))?,
            password_mode: parse_password_mode("password_mode", get("password_mode"))?,
            password_hash: get("password_hash").to_owned(),
            ookla_license_agreed: parse_bool("ookla_license_agreed", get("ookla_license_agreed"))?,
            ookla_server_id: parse_server_id("ookla_server_id", get("ookla_server_id"))?,
        })
    })()
    .map_err(|e| anyhow::anyhow!("stored config is invalid: {e}"))?;
    Ok(cfg)
}

pub async fn set(db: &Db, key: &str, value: &str) -> Result<(), ConfigError> {
    validate(key, value)?;
    let (key, value) = (key.to_owned(), value.to_owned());
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&key, &value],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn set_password_and_mode(
    db: &Db,
    hash: &str,
    mode: PasswordMode,
) -> Result<(), ConfigError> {
    let hash = hash.to_owned();
    let mode_str = mode.as_str().to_owned();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO config (key, value) VALUES ('password_hash', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&hash],
        )?;
        tx.execute(
            "INSERT INTO config (key, value) VALUES ('password_mode', ?1) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [&mode_str],
        )?;
        tx.commit()?;
        Ok(())
    })
    .await?;
    Ok(())
}
