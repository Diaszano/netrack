use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, anyhow};
use chrono_tz::Tz;

pub const DEFAULT_PREVIEW_MESSAGE: &str = "Demonstração ativa. Modificações estão desabilitadas.";

#[derive(Debug, Clone)]
pub struct Settings {
    pub port: u16,
    pub data_dir: PathBuf,
    pub tz: Tz,
    pub run_on_startup: bool,
    pub preview_mode: bool,
    pub preview_message: Option<String>,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_map(&std::env::vars().collect())
    }

    pub fn from_map(vars: &HashMap<String, String>) -> anyhow::Result<Self> {
        let port = match vars.get("NETRACK_PORT") {
            Some(v) => v
                .parse::<u16>()
                .with_context(|| format!("invalid NETRACK_PORT: {v:?}"))?,
            None => 5216,
        };
        let data_dir = vars
            .get("NETRACK_DATA_DIR")
            .map_or_else(|| PathBuf::from("/data"), PathBuf::from);
        // Empty means unset: compose passes `TZ=${TZ}` through even when unset.
        let tz = match vars.get("TZ").map(|v| v.trim()).filter(|v| !v.is_empty()) {
            Some(v) => v
                .parse::<Tz>()
                .map_err(|e| anyhow!("invalid TZ {v:?}: {e}"))?,
            None => Tz::UTC,
        };
        let run_on_startup = match vars.get("RUN_TEST_ON_STARTUP") {
            Some(v) => match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" => true,
                "false" | "0" | "" => false,
                _ => return Err(anyhow!("invalid RUN_TEST_ON_STARTUP: {v:?}")),
            },
            None => false,
        };
        let preview_mode = match vars.get("PREVIEW_MODE") {
            Some(v) => match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" => true,
                "false" | "0" | "" => false,
                _ => return Err(anyhow!("invalid PREVIEW_MODE: {v:?}")),
            },
            None => false,
        };
        let preview_message = if preview_mode {
            let msg = vars
                .get("PREVIEW_MESSAGE")
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| DEFAULT_PREVIEW_MESSAGE.to_string());
            Some(msg)
        } else {
            None
        };
        Ok(Self {
            port,
            data_dir,
            tz,
            run_on_startup,
            preview_mode,
            preview_message,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn defaults_when_env_empty() {
        let s = Settings::from_map(&HashMap::new()).unwrap();
        assert_eq!(s.port, 5216);
        assert_eq!(s.data_dir, PathBuf::from("/data"));
        assert_eq!(s.tz, chrono_tz::UTC);
        assert!(!s.run_on_startup);
    }

    #[test]
    fn reads_all_vars() {
        let s = Settings::from_map(&map(&[
            ("NETRACK_PORT", "8080"),
            ("NETRACK_DATA_DIR", "/tmp/x"),
            ("TZ", "America/Sao_Paulo"),
            ("RUN_TEST_ON_STARTUP", "true"),
        ]))
        .unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.data_dir, PathBuf::from("/tmp/x"));
        assert_eq!(s.tz, chrono_tz::America::Sao_Paulo);
        assert!(s.run_on_startup);
    }

    #[test]
    fn empty_tz_is_utc() {
        for tz in ["", "  "] {
            let s = Settings::from_map(&map(&[("TZ", tz)])).unwrap();
            assert_eq!(s.tz, chrono_tz::UTC);
        }
    }

    #[test]
    fn rejects_bad_tz() {
        let err = Settings::from_map(&map(&[("TZ", "Mars/Base")])).unwrap_err();
        assert!(err.to_string().contains("TZ"));
    }
}
