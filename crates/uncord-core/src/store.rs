use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::backend::AudioError;
use crate::types::{AppSettings, Profile, RouteConfig};

const CONFIG_FILE_NAME: &str = "config.json";
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// On-disk JSON payload persisted under `%APPDATA%\uncord\config.json` (or test `config_dir`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PersistedConfig {
    pub routes: Vec<RouteConfig>,
    pub profiles: Vec<Profile>,
    pub settings: AppSettings,
}

/// Returns `%APPDATA%\uncord` on Windows (or platform equivalent config directory).
pub fn default_config_dir() -> PathBuf {
    if let Some(base) = directories::BaseDirs::new() {
        return base.config_dir().join("uncord");
    }
    std::env::temp_dir().join("uncord")
}

/// Loads persisted routes, profiles, and settings from `config_dir/config.json`.
/// Returns defaults if the file does not yet exist.
pub fn load_config(config_dir: &Path) -> Result<PersistedConfig, AudioError> {
    let path = config_dir.join(CONFIG_FILE_NAME);
    if !path.exists() {
        return Ok(PersistedConfig::default());
    }

    let raw = fs::read_to_string(&path).map_err(|err| {
        AudioError::BackendInit(format!("failed to read {}: {err}", path.display()))
    })?;

    serde_json::from_str::<PersistedConfig>(&raw).map_err(|err| {
        AudioError::InvalidConfig(format!("failed to parse {}: {err}", path.display()))
    })
}

/// Atomically writes `config` to `config_dir/config.json` via temp file + sync + rename.
pub fn save_config(config_dir: &Path, config: &PersistedConfig) -> Result<(), AudioError> {
    fs::create_dir_all(config_dir).map_err(|err| {
        AudioError::BackendInit(format!(
            "failed to create config directory {}: {err}",
            config_dir.display()
        ))
    })?;

    let target_path = config_dir.join(CONFIG_FILE_NAME);
    let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!("{CONFIG_FILE_NAME}.tmp.{}.{seq}", std::process::id());
    let tmp_path = config_dir.join(tmp_name);

    let json = serde_json::to_string_pretty(config)
        .map_err(|err| AudioError::InvalidConfig(format!("failed to serialize config: {err}")))?;

    let write_res = (|| -> std::io::Result<()> {
        let mut file = File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(AudioError::BackendInit(format!(
            "failed to write temporary config {}: {err}",
            tmp_path.display()
        )));
    }

    if let Err(err) = fs::rename(&tmp_path, &target_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AudioError::BackendInit(format!(
            "failed to commit {}: {err}",
            target_path.display()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChannelMode, LatencyMode};

    #[test]
    fn load_missing_returns_default_and_save_is_atomic() {
        let dir = std::env::temp_dir().join(format!("uncord-store-test-{}", uuid::Uuid::new_v4()));
        let loaded = load_config(&dir).expect("missing config should return default");
        assert_eq!(loaded, PersistedConfig::default());

        let state = PersistedConfig {
            routes: vec![RouteConfig {
                id: "r1".into(),
                name: "Blackstar to Sony".into(),
                source_device_id: "src-1".into(),
                source_is_loopback: false,
                channel_mode: ChannelMode::StereoPair { left: 2, right: 3 },
                target_device_id: "dst-1".into(),
                gain_db: -3.5,
                muted: false,
                enabled: true,
                exclusive_output: false,
                latency_mode: LatencyMode::UltraLow,
            }],
            profiles: vec![Profile {
                id: "p1".into(),
                name: "Practice".into(),
                routes: Vec::new(),
            }],
            settings: AppSettings {
                minimize_to_tray_on_close: true,
                start_minimized: true,
                auto_reconnect: true,
                update_checks_enabled: true,
                active_profile_id: Some("p1".into()),
                theme: "light".into(),
            },
        };

        save_config(&dir, &state).expect("save should succeed");
        let reloaded = load_config(&dir).expect("reloaded config should match");
        assert_eq!(reloaded, state);

        // Ensure no leftover .tmp files remain in dir.
        let entries: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["config.json".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }
}
