//! Runtime configuration. Everything has a default and can be overridden by an
//! optional TOML file and environment variables; no provider is hardcoded.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const DEFAULT_POLL_SECS: u64 = 60;
pub const DEFAULT_MAX_AGE_SECS: i64 = 30 * 60;
/// How long past `max_age` the last known bar stays visible (dimmed, with an
/// age suffix) before it is replaced by an explicit "stale" note.
pub const DEFAULT_GRACE_SECS: i64 = 6 * 60 * 60;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub poll_secs: Option<u64>,
    pub max_age_secs: Option<i64>,
    pub grace_secs: Option<i64>,
    pub section_id: Option<String>,
    /// Restrict output to these omp provider ids (in this order). Empty means
    /// every provider that has fresh data.
    pub providers: Vec<String>,
    /// omp provider id -> display label. Unlisted providers get a label
    /// derived from the id.
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct Config {
    pub db_path: PathBuf,
    pub poll_secs: u64,
    pub max_age_ms: i64,
    pub grace_ms: i64,
    pub section_id: String,
    pub providers: Vec<String>,
    pub labels: BTreeMap<String, String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

pub fn parse_file(text: &str) -> Result<FileConfig, toml::de::Error> {
    toml::from_str(text)
}

fn env_parse<T: std::str::FromStr>(key: &str) -> Option<T> {
    std::env::var(key).ok()?.trim().parse().ok()
}

pub fn load(config_path: Option<&Path>) -> Result<Config, String> {
    let file = match config_path.filter(|p| p.exists()) {
        Some(p) => {
            let text = std::fs::read_to_string(p).map_err(|e| format!("read {}: {e}", p.display()))?;
            parse_file(&text).map_err(|e| format!("parse {}: {e}", p.display()))?
        }
        None => FileConfig::default(),
    };
    let db_path = std::env::var_os("OMP_AGENT_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".omp/agent/agent.db"));
    let poll_secs = env_parse("USAGE_POLL_INTERVAL")
        .or(file.poll_secs)
        .unwrap_or(DEFAULT_POLL_SECS)
        .max(5);
    let max_age_secs = env_parse("USAGE_MAX_AGE_SECS")
        .or(file.max_age_secs)
        .unwrap_or(DEFAULT_MAX_AGE_SECS)
        .max(1);
    let grace_secs = env_parse("USAGE_GRACE_SECS")
        .or(file.grace_secs)
        .unwrap_or(DEFAULT_GRACE_SECS)
        .max(0);
    Ok(Config {
        db_path,
        poll_secs,
        max_age_ms: max_age_secs.saturating_mul(1000),
        grace_ms: grace_secs.saturating_mul(1000),
        section_id: std::env::var("USAGE_SECTION_ID")
            .ok()
            .or(file.section_id)
            .unwrap_or_else(|| "usage".to_owned()),
        providers: file.providers,
        labels: file.labels,
    })
}

/// `openai-codex` -> `Openai Codex` when no label is configured.
pub fn derive_label(provider_id: &str) -> String {
    provider_id
        .split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_valid() {
        let c = parse_file("").unwrap();
        assert!(c.providers.is_empty() && c.labels.is_empty());
    }

    #[test]
    fn unknown_keys_are_rejected_not_ignored() {
        assert!(parse_file("polll_secs = 5").is_err());
    }

    #[test]
    fn labels_and_providers_parse() {
        let c = parse_file("providers=[\"anthropic\"]\n[labels]\nanthropic=\"Claude\"").unwrap();
        assert_eq!(c.providers, ["anthropic"]);
        assert_eq!(c.labels["anthropic"], "Claude");
    }

    #[test]
    fn label_derivation() {
        assert_eq!(derive_label("openai-codex"), "Openai Codex");
        assert_eq!(derive_label("zai"), "Zai");
        assert_eq!(derive_label("--"), "");
    }
}
