use crate::watchdog_types::{WatchdogConfigLoadResult, WatchdogRepoConfig};
use std::fs;
use std::path::{Path, PathBuf};

pub fn repo_config_path(repo: &Path) -> PathBuf {
    repo.join(".sloposcope").join("watchdog.json")
}

pub fn save_repo_config(repo: &Path, config: &WatchdogRepoConfig) -> Result<(), String> {
    let path = repo_config_path(repo);
    fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| format!("Failed to create .sloposcope directory: {e}"))?;
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize watchdog config: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write watchdog config: {e}"))?;
    Ok(())
}

pub fn load_repo_config(repo: &Path) -> (WatchdogRepoConfig, WatchdogConfigLoadResult) {
    let path = repo_config_path(repo);
    let exists = path.exists();
    let path_label = path.display().to_string();

    if !exists {
        return (
            WatchdogRepoConfig::default(),
            WatchdogConfigLoadResult {
                path: path_label,
                exists: false,
                loaded: false,
                error: None,
            },
        );
    }

    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<WatchdogRepoConfig>(&raw) {
            Ok(config) => (
                config,
                WatchdogConfigLoadResult {
                    path: path_label,
                    exists: true,
                    loaded: true,
                    error: None,
                },
            ),
            Err(err) => (
                WatchdogRepoConfig::default(),
                WatchdogConfigLoadResult {
                    path: path_label,
                    exists: true,
                    loaded: false,
                    error: Some(format!("Invalid watchdog config JSON: {err}")),
                },
            ),
        },
        Err(err) => (
            WatchdogRepoConfig::default(),
            WatchdogConfigLoadResult {
                path: path_label,
                exists: true,
                loaded: false,
                error: Some(format!("Failed to read watchdog config: {err}")),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_uses_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (config, meta) = load_repo_config(dir.path());
        assert_eq!(config, WatchdogRepoConfig::default());
        assert!(!meta.exists);
        assert!(!meta.loaded);
        assert!(meta.error.is_none());
    }

    #[test]
    fn valid_config_is_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_dir = dir.path().join(".sloposcope");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::write(
            config_dir.join("watchdog.json"),
            r#"{
              "enabled": true,
              "triggerMode": "before_push",
              "requireActiveIssue": false,
              "maxFilesInSnapshot": 12,
              "maxDiffLinesPerFile": 80,
              "dedupeWindowSeconds": 42,
              "alertThreshold": "critical",
              "checks": {
                "issueAlignment": true,
                "instructionCompliance": false,
                "missingValidation": true,
                "costRisk": false
              }
            }"#,
        )
        .expect("write config");

        let (config, meta) = load_repo_config(dir.path());
        assert!(meta.exists);
        assert!(meta.loaded);
        assert_eq!(
            config.trigger_mode,
            crate::watchdog_types::WatchdogTriggerMode::BeforePush
        );
        assert!(!config.require_active_issue);
        assert_eq!(config.max_files_in_snapshot, 12);
        assert_eq!(
            config.alert_threshold,
            crate::watchdog_types::WatchdogAlertThreshold::Critical
        );
        assert!(!config.checks.instruction_compliance);
    }
}
