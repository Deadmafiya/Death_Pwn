use std::path::PathBuf;

use crate::error::{DeathpwnError, Result};

/// One OpenAI-compatible provider endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConfig {
    pub url: String,
    pub key: String,
    pub model: String,
}

/// Runtime configuration, loaded once at startup from the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub provider_a: ProviderConfig,
    pub provider_b: ProviderConfig,
    pub shell: String,
    pub max_goal_steps: u32,
    pub max_corrections: u32,
    pub artifacts_dir: PathBuf,
    pub http_timeout_secs: u64,
    pub preferences: std::collections::HashMap<String, String>,
}

impl Config {
    /// Load and validate configuration from the process environment.
    /// Missing required vars are a hard error naming the offending var.
    pub fn from_env() -> Result<Config> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Core loader parameterized over a variable lookup. Kept private and used
    /// by tests so they never mutate the global, racy process environment.
    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config> {
        let required = |name: &str| -> Result<String> {
            match get(name) {
                Some(v) if !v.is_empty() => Ok(v),
                _ => Err(DeathpwnError::Config(format!(
                    "missing required env var: {name}"
                ))),
            }
        };

        let provider_a = ProviderConfig {
            url: required("DEATHPWN_PROVIDER_A_URL")?,
            key: required("DEATHPWN_PROVIDER_A_KEY")?,
            model: required("DEATHPWN_PROVIDER_A_MODEL")?,
        };
        let provider_b = ProviderConfig {
            url: required("DEATHPWN_PROVIDER_B_URL")?,
            key: required("DEATHPWN_PROVIDER_B_KEY")?,
            model: required("DEATHPWN_PROVIDER_B_MODEL")?,
        };

        let shell = get("DEATHPWN_SHELL")
            .filter(|s| !s.is_empty())
            .or_else(|| get("SHELL").filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "/bin/sh".to_string());

        let max_goal_steps = parse_or_default(&get, "DEATHPWN_MAX_GOAL_STEPS", 12u32)?;
        let max_corrections = parse_or_default(&get, "DEATHPWN_MAX_CORRECTIONS", 2u32)?;
        let http_timeout_secs = parse_or_default(&get, "DEATHPWN_HTTP_TIMEOUT_SECS", 30u64)?;

        let artifacts_dir = resolve_artifacts_dir(&get);
        let preferences = load_preferences(&get)?;

        Ok(Config {
            provider_a,
            provider_b,
            shell,
            max_goal_steps,
            max_corrections,
            artifacts_dir,
            http_timeout_secs,
            preferences,
        })
    }
}

/// Resolve the preference.json path and parse it as a HashMap.
fn load_preferences(
    get: &impl Fn(&str) -> Option<String>,
) -> Result<std::collections::HashMap<String, String>> {
    let path = if let Some(file) = get("DEATHPWN_PREFERENCE_FILE").filter(|s| !s.is_empty()) {
        let p = PathBuf::from(file);
        if !p.exists() {
            return Err(DeathpwnError::Config(format!(
                "configured DEATHPWN_PREFERENCE_FILE does not exist: {}",
                p.display()
            )));
        }
        Some(p)
    } else {
        let mut resolved = None;
        if let Some(xdg) = get("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
            let p = PathBuf::from(xdg).join("deathpwn").join("preference.json");
            if p.exists() {
                resolved = Some(p);
            }
        }
        if resolved.is_none() {
            if let Some(home) = get("HOME").filter(|s| !s.is_empty()) {
                let p = PathBuf::from(home)
                    .join(".config")
                    .join("deathpwn")
                    .join("preference.json");
                if p.exists() {
                    resolved = Some(p);
                }
            }
        }
        if resolved.is_none() {
            let p = PathBuf::from("preference.json");
            if p.exists() {
                resolved = Some(p);
            }
        }
        resolved
    };

    if let Some(p) = path {
        let content = std::fs::read_to_string(&p).map_err(|e| {
            DeathpwnError::Config(format!(
                "failed to read preference file {}: {e}",
                p.display()
            ))
        })?;
        let map: std::collections::HashMap<String, String> = serde_json::from_str(&content)
            .map_err(|e| {
                DeathpwnError::Config(format!(
                    "failed to parse preference file {} as JSON: {e}",
                    p.display()
                ))
            })?;
        Ok(map)
    } else {
        Ok(std::collections::HashMap::new())
    }
}

/// Parse an optional numeric env var, falling back to `default` when unset/empty.
/// A present-but-unparseable value is a config error that names the var.
fn parse_or_default<T>(get: &impl Fn(&str) -> Option<String>, name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr,
{
    match get(name) {
        Some(raw) if !raw.is_empty() => raw
            .parse::<T>()
            .map_err(|_| DeathpwnError::Config(format!("invalid value for {name}: {raw:?}"))),
        _ => Ok(default),
    }
}

/// Resolve the artifacts root: explicit override, else `$XDG_DATA_HOME/deathpwn`,
/// else `$HOME/.local/share/deathpwn`, else a relative `.local/share/deathpwn`.
fn resolve_artifacts_dir(get: &impl Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(dir) = get("DEATHPWN_ARTIFACTS_DIR").filter(|s| !s.is_empty()) {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = get("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(xdg).join("deathpwn");
    }
    if let Some(home) = get("HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(home).join(".local/share/deathpwn");
    }
    PathBuf::from(".local/share/deathpwn")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn all_required() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(
            "DEATHPWN_PROVIDER_A_URL".into(),
            "https://a.example/v1".into(),
        );
        m.insert("DEATHPWN_PROVIDER_A_KEY".into(), "key-a".into());
        m.insert("DEATHPWN_PROVIDER_A_MODEL".into(), "model-a".into());
        m.insert(
            "DEATHPWN_PROVIDER_B_URL".into(),
            "https://b.example/v1".into(),
        );
        m.insert("DEATHPWN_PROVIDER_B_KEY".into(), "key-b".into());
        m.insert("DEATHPWN_PROVIDER_B_MODEL".into(), "model-b".into());
        m
    }

    fn lookup(m: HashMap<String, String>) -> impl Fn(&str) -> Option<String> {
        move |k| m.get(k).cloned()
    }

    #[test]
    fn reads_required_provider_vars() {
        let cfg = Config::from_lookup(lookup(all_required())).unwrap();
        assert_eq!(cfg.provider_a.url, "https://a.example/v1");
        assert_eq!(cfg.provider_a.key, "key-a");
        assert_eq!(cfg.provider_a.model, "model-a");
        assert_eq!(cfg.provider_b.url, "https://b.example/v1");
        assert_eq!(cfg.provider_b.key, "key-b");
        assert_eq!(cfg.provider_b.model, "model-b");
    }

    #[test]
    fn missing_required_var_names_it() {
        let mut m = all_required();
        m.remove("DEATHPWN_PROVIDER_B_MODEL");
        let err = Config::from_lookup(lookup(m)).unwrap_err();
        match err {
            DeathpwnError::Config(msg) => {
                assert!(
                    msg.contains("DEATHPWN_PROVIDER_B_MODEL"),
                    "error should name the missing var, was: {msg}"
                );
            }
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn applies_defaults_when_optional_absent() {
        let cfg = Config::from_lookup(lookup(all_required())).unwrap();
        assert_eq!(cfg.shell, "/bin/sh");
        assert_eq!(cfg.max_goal_steps, 12);
        assert_eq!(cfg.max_corrections, 2);
        assert_eq!(cfg.http_timeout_secs, 30);
        // No DEATHPWN_ARTIFACTS_DIR / XDG_DATA_HOME / HOME in the lookup.
        assert_eq!(cfg.artifacts_dir, PathBuf::from(".local/share/deathpwn"));
    }

    #[test]
    fn shell_prefers_deathpwn_then_shell_env() {
        let mut m = all_required();
        m.insert("SHELL".into(), "/usr/bin/zsh".into());
        let cfg = Config::from_lookup(lookup(m.clone())).unwrap();
        assert_eq!(cfg.shell, "/usr/bin/zsh");

        m.insert("DEATHPWN_SHELL".into(), "/usr/bin/fish".into());
        let cfg = Config::from_lookup(lookup(m)).unwrap();
        assert_eq!(cfg.shell, "/usr/bin/fish");
    }

    #[test]
    fn overrides_numeric_and_artifacts_path() {
        let mut m = all_required();
        m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "5".into());
        m.insert("DEATHPWN_MAX_CORRECTIONS".into(), "3".into());
        m.insert("DEATHPWN_HTTP_TIMEOUT_SECS".into(), "45".into());
        m.insert("DEATHPWN_ARTIFACTS_DIR".into(), "/tmp/dp".into());
        let cfg = Config::from_lookup(lookup(m)).unwrap();
        assert_eq!(cfg.max_goal_steps, 5);
        assert_eq!(cfg.max_corrections, 3);
        assert_eq!(cfg.http_timeout_secs, 45);
        assert_eq!(cfg.artifacts_dir, PathBuf::from("/tmp/dp"));
    }

    #[test]
    fn xdg_data_home_used_for_artifacts_default() {
        let mut m = all_required();
        m.insert("XDG_DATA_HOME".into(), "/xdg".into());
        let cfg = Config::from_lookup(lookup(m)).unwrap();
        assert_eq!(cfg.artifacts_dir, PathBuf::from("/xdg/deathpwn"));
    }

    #[test]
    fn bad_numeric_is_config_error_naming_var() {
        let mut m = all_required();
        m.insert("DEATHPWN_MAX_GOAL_STEPS".into(), "not-a-number".into());
        let err = Config::from_lookup(lookup(m)).unwrap_err();
        match err {
            DeathpwnError::Config(msg) => {
                assert!(
                    msg.contains("DEATHPWN_MAX_GOAL_STEPS"),
                    "error should name the bad var, was: {msg}"
                );
            }
            other => panic!("expected Config error, got {other:?}"),
        }
    }

    #[test]
    fn loads_valid_preference_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("preference.json");
        std::fs::write(&file_path, r#"{"host discovery": "sudo arp-scan --local"}"#).unwrap();

        let mut m = all_required();
        m.insert(
            "DEATHPWN_PREFERENCE_FILE".into(),
            file_path.to_str().unwrap().into(),
        );

        let cfg = Config::from_lookup(lookup(m)).unwrap();
        assert_eq!(
            cfg.preferences.get("host discovery").unwrap(),
            "sudo arp-scan --local"
        );
    }

    #[test]
    fn error_on_invalid_preference_json() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("preference.json");
        std::fs::write(&file_path, "not-json").unwrap();

        let mut m = all_required();
        m.insert(
            "DEATHPWN_PREFERENCE_FILE".into(),
            file_path.to_str().unwrap().into(),
        );

        let err = Config::from_lookup(lookup(m)).unwrap_err();
        assert!(err.to_string().contains("failed to parse preference file"));
    }

    #[test]
    fn error_on_missing_configured_preference_file() {
        let mut m = all_required();
        m.insert(
            "DEATHPWN_PREFERENCE_FILE".into(),
            "/nonexistent/path/preference.json".into(),
        );

        let err = Config::from_lookup(lookup(m)).unwrap_err();
        assert!(err
            .to_string()
            .contains("configured DEATHPWN_PREFERENCE_FILE does not exist"));
    }
}
