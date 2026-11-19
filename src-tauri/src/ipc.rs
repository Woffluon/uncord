use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, State};
use uncord_core::engine::UncordEngine;
use uncord_core::types::{
    AppSettings, AudioDeviceInfo, ChannelMode, EngineSnapshot, LatencyMode, Profile, RouteConfig,
    RouteTelemetry,
};

use crate::{tray, AppLifecycleState};

pub const EVENT_TELEMETRY: &str = "uncord://telemetry";
pub const EVENT_DEVICES_CHANGED: &str = "uncord://devices-changed";
pub const EVENT_ROUTE_STATUS: &str = "uncord://route-status";

/// Payload for adding a new audio route from the UI.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRouteRequest {
    pub name: String,
    pub source_device_id: String,
    pub source_is_loopback: bool,
    pub channel_mode: ChannelMode,
    pub target_device_id: String,
    pub gain_db: f32,
    pub exclusive_output: bool,
    pub latency_mode: LatencyMode,
}

/// Frozen IPC command contract exposed by the Tauri shell to `src/lib/ipc.ts`.
pub trait UncordIpcContract {
    fn get_snapshot(&self) -> Result<EngineSnapshot, String>;
    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, String>;
    fn add_route(&self, req: AddRouteRequest) -> Result<RouteConfig, String>;
    fn update_route(&self, route: RouteConfig) -> Result<RouteConfig, String>;
    fn remove_route(&self, route_id: String) -> Result<(), String>;
    fn set_route_gain_mute(&self, route_id: String, gain_db: f32, muted: bool)
        -> Result<(), String>;
    fn list_profiles(&self) -> Result<Vec<Profile>, String>;
    fn save_profile(&self, name: String) -> Result<Profile, String>;
    fn load_profile(&self, profile_id: String) -> Result<EngineSnapshot, String>;
    fn delete_profile(&self, profile_id: String) -> Result<(), String>;
    fn update_settings(&self, settings: AppSettings) -> Result<AppSettings, String>;
    fn get_telemetry(&self) -> Result<Vec<RouteTelemetry>, String>;
}

fn next_route_id() -> String {
    static ROUTE_SEQ: AtomicU64 = AtomicU64::new(1);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let seq = ROUTE_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("route-{now_ms:x}-{seq}")
}

impl UncordIpcContract for UncordEngine {
    fn get_snapshot(&self) -> Result<EngineSnapshot, String> {
        UncordEngine::get_snapshot(self).map_err(|err| err.to_string())
    }

    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, String> {
        UncordEngine::list_devices(self).map_err(|err| err.to_string())
    }

    fn add_route(&self, req: AddRouteRequest) -> Result<RouteConfig, String> {
        let config = RouteConfig {
            id: next_route_id(),
            name: req.name,
            source_device_id: req.source_device_id,
            source_is_loopback: req.source_is_loopback,
            channel_mode: req.channel_mode,
            target_device_id: req.target_device_id,
            gain_db: req.gain_db,
            muted: false,
            enabled: true,
            exclusive_output: req.exclusive_output,
            latency_mode: req.latency_mode,
        };
        UncordEngine::add_route(self, config).map_err(|err| err.to_string())
    }

    fn update_route(&self, route: RouteConfig) -> Result<RouteConfig, String> {
        UncordEngine::update_route(self, route).map_err(|err| err.to_string())
    }

    fn remove_route(&self, route_id: String) -> Result<(), String> {
        UncordEngine::remove_route(self, &route_id).map_err(|err| err.to_string())
    }

    fn set_route_gain_mute(
        &self,
        route_id: String,
        gain_db: f32,
        muted: bool,
    ) -> Result<(), String> {
        UncordEngine::set_route_gain_mute(self, &route_id, gain_db, muted)
            .map_err(|err| err.to_string())
    }

    fn list_profiles(&self) -> Result<Vec<Profile>, String> {
        Ok(UncordEngine::list_profiles(self))
    }

    fn save_profile(&self, name: String) -> Result<Profile, String> {
        UncordEngine::save_profile(self, name).map_err(|err| err.to_string())
    }

    fn load_profile(&self, profile_id: String) -> Result<EngineSnapshot, String> {
        UncordEngine::load_profile(self, &profile_id).map_err(|err| err.to_string())
    }

    fn delete_profile(&self, profile_id: String) -> Result<(), String> {
        UncordEngine::delete_profile(self, &profile_id).map_err(|err| err.to_string())
    }

    fn update_settings(&self, settings: AppSettings) -> Result<AppSettings, String> {
        UncordEngine::update_settings(self, settings).map_err(|err| err.to_string())
    }

    fn get_telemetry(&self) -> Result<Vec<RouteTelemetry>, String> {
        Ok(UncordEngine::get_telemetry(self))
    }
}

async fn run_engine_blocking<T, F>(engine: &State<'_, Arc<UncordEngine>>, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&UncordEngine) -> Result<T, String> + Send + 'static,
{
    let engine_arc = Arc::clone(engine.inner());
    tauri::async_runtime::spawn_blocking(move || task(engine_arc.as_ref()))
        .await
        .map_err(|err| format!("engine worker task failed: {err}"))?
}

/// Returns the full engine snapshot including discovered devices, routes, profiles, and settings.
#[tauri::command]
pub async fn get_snapshot(engine: State<'_, Arc<UncordEngine>>) -> Result<EngineSnapshot, String> {
    run_engine_blocking(&engine, UncordIpcContract::get_snapshot).await
}

/// Enumerates active capture, render, and loopback audio devices.
#[tauri::command]
pub async fn list_devices(
    engine: State<'_, Arc<UncordEngine>>,
) -> Result<Vec<AudioDeviceInfo>, String> {
    run_engine_blocking(&engine, UncordIpcContract::list_devices).await
}

/// Creates and starts a new audio route from UI request parameters.
#[tauri::command]
pub async fn add_route(
    app: AppHandle,
    engine: State<'_, Arc<UncordEngine>>,
    req: AddRouteRequest,
) -> Result<RouteConfig, String> {
    let route = run_engine_blocking(&engine, move |eng| UncordIpcContract::add_route(eng, req)).await?;
    tray::refresh_tray_menu_async(&app);
    Ok(route)
}

/// Updates an existing route's configuration and restarts its stream if needed.
#[tauri::command]
pub async fn update_route(
    app: AppHandle,
    engine: State<'_, Arc<UncordEngine>>,
    route: RouteConfig,
) -> Result<RouteConfig, String> {
    let updated =
        run_engine_blocking(&engine, move |eng| UncordIpcContract::update_route(eng, route)).await?;
    tray::refresh_tray_menu_async(&app);
    Ok(updated)
}

/// Stops and removes an audio route by ID.
#[tauri::command]
pub async fn remove_route(
    app: AppHandle,
    engine: State<'_, Arc<UncordEngine>>,
    route_id: String,
) -> Result<(), String> {
    run_engine_blocking(&engine, move |eng| {
        UncordIpcContract::remove_route(eng, route_id)
    })
    .await?;
    tray::refresh_tray_menu_async(&app);
    Ok(())
}

/// Adjusts live gain and mute atomically without restarting the audio stream.
#[tauri::command]
pub fn set_route_gain_mute(
    engine: State<'_, Arc<UncordEngine>>,
    route_id: String,
    gain_db: f32,
    muted: bool,
) -> Result<(), String> {
    UncordIpcContract::set_route_gain_mute(engine.inner().as_ref(), route_id, gain_db, muted)
}

/// Lists all saved routing profiles.
#[tauri::command]
pub async fn list_profiles(engine: State<'_, Arc<UncordEngine>>) -> Result<Vec<Profile>, String> {
    run_engine_blocking(&engine, UncordIpcContract::list_profiles).await
}

/// Captures current routes into a named profile and refreshes the tray submenu.
#[tauri::command]
pub async fn save_profile(
    app: AppHandle,
    engine: State<'_, Arc<UncordEngine>>,
    name: String,
) -> Result<Profile, String> {
    let profile =
        run_engine_blocking(&engine, move |eng| UncordIpcContract::save_profile(eng, name)).await?;
    tray::refresh_tray_menu_async(&app);
    Ok(profile)
}

/// Activates a saved profile and returns the resulting engine snapshot.
#[tauri::command]
pub async fn load_profile(
    app: AppHandle,
    lifecycle: State<'_, Arc<AppLifecycleState>>,
    engine: State<'_, Arc<UncordEngine>>,
    profile_id: String,
) -> Result<EngineSnapshot, String> {
    let snapshot = run_engine_blocking(&engine, move |eng| {
        UncordIpcContract::load_profile(eng, profile_id)
    })
    .await?;
    lifecycle.sync_from_settings(&snapshot.settings);
    tray::refresh_tray_menu_async(&app);
    Ok(snapshot)
}

/// Deletes a saved profile by ID and refreshes the tray submenu.
#[tauri::command]
pub async fn delete_profile(
    app: AppHandle,
    engine: State<'_, Arc<UncordEngine>>,
    profile_id: String,
) -> Result<(), String> {
    run_engine_blocking(&engine, move |eng| {
        UncordIpcContract::delete_profile(eng, profile_id)
    })
    .await?;
    tray::refresh_tray_menu_async(&app);
    Ok(())
}

/// Persists updated application settings and updates the close-to-tray flag atomically.
#[tauri::command]
pub async fn update_settings(
    lifecycle: State<'_, Arc<AppLifecycleState>>,
    engine: State<'_, Arc<UncordEngine>>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let saved = run_engine_blocking(&engine, move |eng| {
        UncordIpcContract::update_settings(eng, settings)
    })
    .await?;
    lifecycle.sync_from_settings(&saved);
    Ok(saved)
}

/// Reads current lock-free telemetry snapshots for all configured routes.
#[tauri::command]
pub fn get_telemetry(engine: State<'_, Arc<UncordEngine>>) -> Result<Vec<RouteTelemetry>, String> {
    UncordIpcContract::get_telemetry(engine.inner().as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncord_core::backend::{ActiveRouteHandle, AudioBackend, AudioError};
    use uncord_core::types::DeviceChangeEvent;

    struct DummyBackend;

    impl AudioBackend for DummyBackend {
        fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError> {
            Ok(Vec::new())
        }

        fn start_route(
            &self,
            _config: &RouteConfig,
        ) -> Result<Box<dyn ActiveRouteHandle>, AudioError> {
            Err(AudioError::BackendInit("dummy".to_string()))
        }

        fn subscribe_device_changes(
            &self,
            _callback: Box<dyn Fn(DeviceChangeEvent) + Send + Sync>,
        ) -> Result<(), AudioError> {
            Ok(())
        }
    }

    #[test]
    fn add_route_request_deserializes_camel_case_payload() {
        let json = r#"{
            "name": "Amp to Buds",
            "sourceDeviceId": "src-1",
            "sourceIsLoopback": false,
            "channelMode": { "kind": "stereoPair", "left": 0, "right": 1 },
            "targetDeviceId": "dst-1",
            "gainDb": -3.5,
            "exclusiveOutput": false,
            "latencyMode": "ultraLow"
        }"#;

        let req: AddRouteRequest = serde_json::from_str(json).expect("valid camelCase AddRouteRequest");
        assert_eq!(req.name, "Amp to Buds");
        assert_eq!(req.source_device_id, "src-1");
        assert!(!req.source_is_loopback);
        assert_eq!(req.channel_mode, ChannelMode::StereoPair { left: 0, right: 1 });
        assert_eq!(req.target_device_id, "dst-1");
        assert!((req.gain_db - (-3.5)).abs() < f32::EPSILON);
        assert!(!req.exclusive_output);
        assert_eq!(req.latency_mode, LatencyMode::UltraLow);
    }

    #[test]
    fn route_ids_are_unique() {
        let first = next_route_id();
        let second = next_route_id();
        assert_ne!(first, second);
        assert!(first.starts_with("route-"));
    }

    #[test]
    fn engine_implements_frozen_ipc_contract() {
        let temp_dir = std::env::temp_dir().join(format!("uncord-ipc-test-{}", std::process::id()));
        let engine = UncordEngine::new_with_backend(Box::new(DummyBackend), temp_dir.clone())
            .expect("dummy backend engine");

        let snapshot = UncordIpcContract::get_snapshot(&engine).expect("snapshot");
        assert_eq!(snapshot.version, uncord_core::VERSION);
        assert!(UncordIpcContract::list_devices(&engine).is_ok());
        assert!(UncordIpcContract::list_profiles(&engine).is_ok());
        assert!(UncordIpcContract::get_telemetry(&engine).is_ok());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
