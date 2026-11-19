use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use crate::backend::{ActiveRouteHandle, AudioBackend, AudioError};
use crate::dsp::matches_source_device;
use crate::store::{self, PersistedConfig};
use crate::types::{
    AppSettings, AudioDeviceInfo, DeviceChangeEvent, DeviceFlow, EngineSnapshot, LatencyBreakdown,
    Profile, RouteConfig, RouteStatus, RouteTelemetry,
};

type DeviceListListener = Arc<dyn Fn(Vec<AudioDeviceInfo>) + Send + Sync>;

const GAIN_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

struct RuntimeRoute {
    config: RouteConfig,
    status: RouteStatus,
    handle: Option<Box<dyn ActiveRouteHandle>>,
}

struct EngineState {
    routes: Vec<RuntimeRoute>,
    profiles: Vec<Profile>,
    settings: AppSettings,
    dirty: bool,
    last_flush: Instant,
    shutting_down: bool,
}

impl EngineState {
    fn snapshot_persisted(&self) -> PersistedConfig {
        PersistedConfig {
            routes: self.routes.iter().map(|r| r.config.clone()).collect(),
            profiles: self.profiles.clone(),
            settings: self.settings.clone(),
        }
    }
}

struct EngineInner {
    backend: Box<dyn AudioBackend>,
    config_dir: PathBuf,
    lifecycle_lock: Mutex<()>,
    state: Mutex<EngineState>,
    listeners: Mutex<Vec<DeviceListListener>>,
}

impl Drop for EngineInner {
    fn drop(&mut self) {
        shutdown_inner(self);
    }
}

/// Coordinates audio device enumeration, persistent routes/profiles, and hot-plug lifecycle.
pub struct UncordEngine {
    inner: Arc<EngineInner>,
}

impl UncordEngine {
    /// Creates an engine using the native platform audio backend (WASAPI on Windows)
    /// and the standard user configuration directory (`%APPDATA%\uncord`).
    pub fn new_default() -> Result<Self, AudioError> {
        #[cfg(windows)]
        {
            let backend = Box::new(crate::wasapi::WasapiBackend::new()?);
            let config_dir = store::default_config_dir();
            Self::new_with_backend(backend, config_dir)
        }
        #[cfg(not(windows))]
        {
            Err(AudioError::BackendInit(
                "WASAPI backend is only available on Windows".to_string(),
            ))
        }
    }

    /// Creates an engine with an injected `AudioBackend` and explicit config directory.
    pub fn new_with_backend(
        backend: Box<dyn AudioBackend>,
        config_dir: PathBuf,
    ) -> Result<Self, AudioError> {
        let persisted = store::load_config(&config_dir)?;
        let runtime_routes = persisted
            .routes
            .into_iter()
            .map(|config| RuntimeRoute {
                status: RouteStatus::Stopped,
                config,
                handle: None,
            })
            .collect();

        let inner = Arc::new(EngineInner {
            backend,
            config_dir,
            lifecycle_lock: Mutex::new(()),
            state: Mutex::new(EngineState {
                routes: runtime_routes,
                profiles: persisted.profiles,
                settings: persisted.settings,
                dirty: false,
                last_flush: Instant::now(),
                shutting_down: false,
            }),
            listeners: Mutex::new(Vec::new()),
        });

        let weak: Weak<EngineInner> = Arc::downgrade(&inner);
        inner.backend.subscribe_device_changes(Box::new(move |ev| {
            if let Some(strong) = weak.upgrade() {
                handle_device_event(&strong, ev);
            }
        }))?;

        // Start any persisted enabled routes without failing startup if a device is unplugged.
        reconcile_startup_routes(&inner);

        Ok(Self { inner })
    }

    /// Returns a full snapshot of version, active devices, configured routes, profiles, and settings.
    pub fn get_snapshot(&self) -> Result<EngineSnapshot, AudioError> {
        let devices = self.list_devices()?;
        let guard = self
            .inner
            .state
            .lock()
            .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
        Ok(EngineSnapshot {
            version: crate::VERSION.to_string(),
            devices,
            routes: guard.routes.iter().map(|r| r.config.clone()).collect(),
            profiles: guard.profiles.clone(),
            settings: guard.settings.clone(),
        })
    }

    /// Queries currently active audio devices from the backend.
    pub fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError> {
        self.inner.backend.list_devices()
    }

    /// Validates, optionally starts, and persists a new audio route.
    pub fn add_route(&self, mut config: RouteConfig) -> Result<RouteConfig, AudioError> {
        validate_route_config(&config)?;
        if config.id.trim().is_empty() {
            config.id = uuid::Uuid::new_v4().to_string();
        }

        let _op = self
            .inner
            .lifecycle_lock
            .lock()
            .map_err(|_| AudioError::Platform("engine lifecycle lock poisoned".into()))?;

        let (handle, status) = if config.enabled {
            let h = self.inner.backend.start_route(&config)?;
            (Some(h), RouteStatus::Running)
        } else {
            (None, RouteStatus::Stopped)
        };

        let persisted = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            if guard.routes.iter().any(|r| r.config.id == config.id) {
                if let Some(h) = handle {
                    drop(guard);
                    h.stop();
                }
                return Err(AudioError::InvalidConfig(format!(
                    "duplicate route id: {}",
                    config.id
                )));
            }
            guard.routes.push(RuntimeRoute {
                config: config.clone(),
                status,
                handle,
            });
            guard.dirty = false;
            guard.last_flush = Instant::now();
            guard.snapshot_persisted()
        };

        store::save_config(&self.inner.config_dir, &persisted)?;
        Ok(config)
    }

    /// Updates an existing route; applies gain/mute/channel changes live without glitching audio.
    pub fn update_route(&self, config: RouteConfig) -> Result<RouteConfig, AudioError> {
        validate_route_config(&config)?;

        let _op = self
            .inner
            .lifecycle_lock
            .lock()
            .map_err(|_| AudioError::Platform("engine lifecycle lock poisoned".into()))?;

        let needs_restart = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let Some(existing) = guard.routes.iter_mut().find(|r| r.config.id == config.id) else {
                return Err(AudioError::InvalidConfig(format!(
                    "route not found: {}",
                    config.id
                )));
            };

            let stream_unchanged = existing.config.source_device_id == config.source_device_id
                && existing.config.source_is_loopback == config.source_is_loopback
                && existing.config.target_device_id == config.target_device_id
                && existing.config.exclusive_output == config.exclusive_output
                && existing.config.latency_mode == config.latency_mode
                && existing.config.enabled == config.enabled;

            if stream_unchanged && config.enabled {
                if let Some(handle) = existing.handle.as_ref() {
                    handle.set_gain_db(config.gain_db);
                    handle.set_muted(config.muted);
                    handle.set_channel_mode(config.channel_mode);
                    existing.config = config.clone();
                    guard.dirty = false;
                    guard.last_flush = Instant::now();
                    let persisted = guard.snapshot_persisted();
                    drop(guard);
                    store::save_config(&self.inner.config_dir, &persisted)?;
                    return Ok(config);
                }
            }
            true
        };

        if needs_restart {
            let old_handle = {
                let mut guard = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
                let existing = guard
                    .routes
                    .iter_mut()
                    .find(|r| r.config.id == config.id)
                    .ok_or_else(|| {
                        AudioError::InvalidConfig(format!("route not found: {}", config.id))
                    })?;
                existing.handle.take()
            };

            if let Some(h) = old_handle {
                h.stop();
            }

            let (new_handle, new_status) = if config.enabled {
                match self.inner.backend.start_route(&config) {
                    Ok(h) => (Some(h), RouteStatus::Running),
                    Err(err) => {
                        if let Ok(mut guard) = self.inner.state.lock() {
                            if let Some(existing) =
                                guard.routes.iter_mut().find(|r| r.config.id == config.id)
                            {
                                existing.status = RouteStatus::Error {
                                    message: err.to_string(),
                                };
                            }
                        }
                        return Err(err);
                    }
                }
            } else {
                (None, RouteStatus::Stopped)
            };

            let persisted = {
                let mut guard = self
                    .inner
                    .state
                    .lock()
                    .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
                let existing = guard
                    .routes
                    .iter_mut()
                    .find(|r| r.config.id == config.id)
                    .ok_or_else(|| {
                        AudioError::InvalidConfig(format!("route not found: {}", config.id))
                    })?;
                existing.config = config.clone();
                existing.status = new_status;
                existing.handle = new_handle;
                guard.dirty = false;
                guard.last_flush = Instant::now();
                guard.snapshot_persisted()
            };

            store::save_config(&self.inner.config_dir, &persisted)?;
        }

        Ok(config)
    }

    /// Stops and removes a route by ID.
    pub fn remove_route(&self, route_id: &str) -> Result<(), AudioError> {
        let _op = self
            .inner
            .lifecycle_lock
            .lock()
            .map_err(|_| AudioError::Platform("engine lifecycle lock poisoned".into()))?;

        let (removed_handle, persisted) = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let Some(pos) = guard.routes.iter().position(|r| r.config.id == route_id) else {
                return Err(AudioError::InvalidConfig(format!(
                    "route not found: {route_id}"
                )));
            };
            let removed = guard.routes.remove(pos);
            guard.dirty = false;
            guard.last_flush = Instant::now();
            (removed.handle, guard.snapshot_persisted())
        };

        if let Some(h) = removed_handle {
            h.stop();
        }
        store::save_config(&self.inner.config_dir, &persisted)?;
        Ok(())
    }

    /// Atomically updates gain and mute state on a live route without interrupting audio.
    pub fn set_route_gain_mute(
        &self,
        route_id: &str,
        gain_db: f32,
        muted: bool,
    ) -> Result<(), AudioError> {
        if !gain_db.is_finite() {
            return Err(AudioError::InvalidConfig(
                "gain_db must be a finite number".to_string(),
            ));
        }

        let maybe_persisted = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let Some(entry) = guard.routes.iter_mut().find(|r| r.config.id == route_id) else {
                return Err(AudioError::InvalidConfig(format!(
                    "route not found: {route_id}"
                )));
            };

            entry.config.gain_db = gain_db;
            entry.config.muted = muted;
            if let Some(handle) = entry.handle.as_ref() {
                handle.set_gain_db(gain_db);
                handle.set_muted(muted);
            }

            if guard.last_flush.elapsed() >= GAIN_FLUSH_INTERVAL {
                guard.dirty = false;
                guard.last_flush = Instant::now();
                Some(guard.snapshot_persisted())
            } else {
                guard.dirty = true;
                None
            }
        };

        if let Some(persisted) = maybe_persisted {
            store::save_config(&self.inner.config_dir, &persisted)?;
        }

        Ok(())
    }

    /// Lists all saved routing profiles.
    pub fn list_profiles(&self) -> Vec<Profile> {
        self.inner
            .state
            .lock()
            .map(|g| g.profiles.clone())
            .unwrap_or_default()
    }

    /// Saves the current routes as a named profile and marks it active.
    pub fn save_profile(&self, name: String) -> Result<Profile, AudioError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(AudioError::InvalidConfig(
                "profile name cannot be empty".to_string(),
            ));
        }

        let (profile, persisted) = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let profile = Profile {
                id: uuid::Uuid::new_v4().to_string(),
                name: trimmed.to_string(),
                routes: guard.routes.iter().map(|r| r.config.clone()).collect(),
            };
            guard.settings.active_profile_id = Some(profile.id.clone());
            guard.profiles.push(profile.clone());
            guard.dirty = false;
            guard.last_flush = Instant::now();
            (profile, guard.snapshot_persisted())
        };

        store::save_config(&self.inner.config_dir, &persisted)?;
        Ok(profile)
    }

    /// Replaces active routes with the selected profile (by ID or exact name fallback) and restarts streams.
    pub fn load_profile(&self, profile_id: &str) -> Result<EngineSnapshot, AudioError> {
        let _op = self
            .inner
            .lifecycle_lock
            .lock()
            .map_err(|_| AudioError::Platform("engine lifecycle lock poisoned".into()))?;

        let (profile, old_handles) = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let found = guard
                .profiles
                .iter()
                .find(|p| p.id == profile_id)
                .or_else(|| {
                    guard
                        .profiles
                        .iter()
                        .find(|p| p.name.eq_ignore_ascii_case(profile_id))
                })
                .cloned()
                .ok_or_else(|| {
                    AudioError::InvalidConfig(format!("profile not found: {profile_id}"))
                })?;

            let mut old = Vec::new();
            for r in &mut guard.routes {
                if let Some(h) = r.handle.take() {
                    old.push(h);
                }
            }

            guard.routes = found
                .routes
                .iter()
                .cloned()
                .map(|config| RuntimeRoute {
                    status: RouteStatus::Stopped,
                    config,
                    handle: None,
                })
                .collect();
            guard.settings.active_profile_id = Some(found.id.clone());
            (found, old)
        };

        for h in old_handles {
            h.stop();
        }

        reconcile_routes_list(&self.inner, &profile.routes);

        let persisted = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            guard.dirty = false;
            guard.last_flush = Instant::now();
            guard.snapshot_persisted()
        };
        store::save_config(&self.inner.config_dir, &persisted)?;

        self.get_snapshot()
    }

    /// Deletes a saved profile by ID.
    pub fn delete_profile(&self, profile_id: &str) -> Result<(), AudioError> {
        let persisted = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            let Some(pos) = guard.profiles.iter().position(|p| p.id == profile_id) else {
                return Err(AudioError::InvalidConfig(format!(
                    "profile not found: {profile_id}"
                )));
            };
            guard.profiles.remove(pos);
            if guard.settings.active_profile_id.as_deref() == Some(profile_id) {
                guard.settings.active_profile_id = None;
            }
            guard.dirty = false;
            guard.last_flush = Instant::now();
            guard.snapshot_persisted()
        };

        store::save_config(&self.inner.config_dir, &persisted)?;
        Ok(())
    }

    /// Updates application settings and persists them atomically.
    pub fn update_settings(&self, settings: AppSettings) -> Result<AppSettings, AudioError> {
        let persisted = {
            let mut guard = self
                .inner
                .state
                .lock()
                .map_err(|_| AudioError::Platform("engine state lock poisoned".into()))?;
            guard.settings = settings.clone();
            guard.dirty = false;
            guard.last_flush = Instant::now();
            guard.snapshot_persisted()
        };

        store::save_config(&self.inner.config_dir, &persisted)?;
        Ok(settings)
    }

    /// Collects non-blocking real-time telemetry snapshots for every configured route.
    pub fn get_telemetry(&self) -> Vec<RouteTelemetry> {
        let Ok(guard) = self.inner.state.lock() else {
            return Vec::new();
        };

        guard
            .routes
            .iter()
            .map(|r| {
                if let Some(handle) = r.handle.as_ref() {
                    handle.telemetry()
                } else {
                    RouteTelemetry {
                        route_id: r.config.id.clone(),
                        status: r.status.clone(),
                        peak_left: 0.0,
                        peak_right: 0.0,
                        rms_left: 0.0,
                        rms_right: 0.0,
                        clipping: false,
                        buffer_fill_ratio: 0.0,
                        drift_ppm: 0.0,
                        xruns: 0,
                        latency: LatencyBreakdown::default(),
                    }
                }
            })
            .collect()
    }

    /// Registers a listener notified whenever audio endpoints are added, removed, or changed.
    pub fn on_device_change<F: Fn(Vec<AudioDeviceInfo>) + Send + Sync + 'static>(&self, cb: F) {
        if let Ok(mut guard) = self.inner.listeners.lock() {
            guard.push(Arc::new(cb));
        }
    }

    /// Flushes any pending configuration changes and cleanly stops all active audio streams.
    pub fn shutdown(&self) {
        shutdown_inner(&self.inner);
    }
}

fn shutdown_inner(inner: &EngineInner) {
    let (handles, dirty_snapshot) = {
        let Ok(mut guard) = inner.state.lock() else {
            return;
        };
        guard.shutting_down = true;
        let mut handles = Vec::new();
        for r in &mut guard.routes {
            if let Some(h) = r.handle.take() {
                handles.push(h);
            }
            if r.config.enabled {
                r.status = RouteStatus::Stopped;
            }
        }
        let dirty = if guard.dirty {
            guard.dirty = false;
            guard.last_flush = Instant::now();
            Some(guard.snapshot_persisted())
        } else {
            None
        };
        (handles, dirty)
    };

    if let Some(persisted) = dirty_snapshot {
        let _ = store::save_config(&inner.config_dir, &persisted);
    }

    for h in handles {
        h.stop();
    }
}

fn validate_route_config(config: &RouteConfig) -> Result<(), AudioError> {
    if config.source_device_id.trim().is_empty() {
        return Err(AudioError::InvalidConfig(
            "source_device_id cannot be empty".to_string(),
        ));
    }
    if config.target_device_id.trim().is_empty() {
        return Err(AudioError::InvalidConfig(
            "target_device_id cannot be empty".to_string(),
        ));
    }
    if !config.gain_db.is_finite() {
        return Err(AudioError::InvalidConfig(
            "gain_db must be finite".to_string(),
        ));
    }
    Ok(())
}

fn reconcile_startup_routes(inner: &EngineInner) {
    let Ok(_op) = inner.lifecycle_lock.lock() else {
        return;
    };
    let routes: Vec<RouteConfig> = {
        let Ok(guard) = inner.state.lock() else {
            return;
        };
        guard.routes.iter().map(|r| r.config.clone()).collect()
    };
    reconcile_routes_list(inner, &routes);
}

fn reconcile_routes_list(inner: &EngineInner, routes: &[RouteConfig]) {
    let auto_reconnect = inner
        .state
        .lock()
        .map(|g| g.settings.auto_reconnect)
        .unwrap_or(true);

    for cfg in routes {
        if !cfg.enabled {
            continue;
        }
        let (handle, status) = match inner.backend.start_route(cfg) {
            Ok(h) => (Some(h), RouteStatus::Running),
            Err(AudioError::DeviceNotFound(_)) if auto_reconnect => (None, RouteStatus::Reconnecting),
            Err(err) => (
                None,
                RouteStatus::Error {
                    message: err.to_string(),
                },
            ),
        };

        if let Ok(mut guard) = inner.state.lock() {
            if guard.shutting_down {
                if let Some(h) = handle {
                    drop(guard);
                    h.stop();
                }
                return;
            }
            if let Some(entry) = guard.routes.iter_mut().find(|r| r.config.id == cfg.id) {
                entry.handle = handle;
                entry.status = status;
            } else if let Some(h) = handle {
                drop(guard);
                h.stop();
            }
        }
    }
}

fn device_id_matches(route_device_id: &str, event_device_id: &str) -> bool {
    let canon_route = route_device_id
        .strip_prefix("loopback::")
        .unwrap_or(route_device_id);
    let canon_event = event_device_id
        .strip_prefix("loopback::")
        .unwrap_or(event_device_id);
    canon_route == canon_event
}

fn devices_present_for_route(devices: &[AudioDeviceInfo], route: &RouteConfig) -> bool {
    let is_loopback =
        route.source_is_loopback || route.source_device_id.starts_with("loopback::");

    let has_source = devices
        .iter()
        .any(|d| matches_source_device(d, &route.source_device_id, is_loopback));

    let has_target = devices
        .iter()
        .any(|d| d.flow == DeviceFlow::Render && d.id == route.target_device_id);

    has_source && has_target
}

fn handle_device_event(inner: &EngineInner, event: DeviceChangeEvent) {
    if let Ok(_op) = inner.lifecycle_lock.lock() {
        match &event {
            DeviceChangeEvent::Removed(removed_id) => {
                let to_stop = {
                    let Ok(mut guard) = inner.state.lock() else {
                        return;
                    };
                    if guard.shutting_down {
                        return;
                    }
                    let auto_reconnect = guard.settings.auto_reconnect;
                    let mut stopped = Vec::new();
                    for r in &mut guard.routes {
                        if device_id_matches(&r.config.source_device_id, removed_id)
                            || device_id_matches(&r.config.target_device_id, removed_id)
                        {
                            if let Some(h) = r.handle.take() {
                                stopped.push(h);
                            }
                            if r.config.enabled {
                                r.status = if auto_reconnect {
                                    RouteStatus::Reconnecting
                                } else {
                                    RouteStatus::Error {
                                        message: format!("audio device disconnected: {removed_id}"),
                                    }
                                };
                            }
                        }
                    }
                    stopped
                };

                for h in to_stop {
                    h.stop();
                }
            }
            DeviceChangeEvent::Added(_)
            | DeviceChangeEvent::StateChanged(_)
            | DeviceChangeEvent::DefaultChanged => {
                let (auto_reconnect, candidates) = {
                    let Ok(guard) = inner.state.lock() else {
                        return;
                    };
                    if guard.shutting_down {
                        return;
                    }
                    let list: Vec<RouteConfig> = guard
                        .routes
                        .iter()
                        .filter(|r| r.config.enabled && r.handle.is_none())
                        .map(|r| r.config.clone())
                        .collect();
                    (guard.settings.auto_reconnect, list)
                };

                if auto_reconnect && !candidates.is_empty() {
                    if let Ok(devices) = inner.backend.list_devices() {
                        for cfg in candidates {
                            if !devices_present_for_route(&devices, &cfg) {
                                continue;
                            }
                            let (handle, status) = match inner.backend.start_route(&cfg) {
                                Ok(h) => (Some(h), RouteStatus::Running),
                                Err(AudioError::DeviceNotFound(_)) => {
                                    (None, RouteStatus::Reconnecting)
                                }
                                Err(err) => (
                                    None,
                                    RouteStatus::Error {
                                        message: err.to_string(),
                                    },
                                ),
                            };

                            if let Ok(mut guard) = inner.state.lock() {
                                if guard.shutting_down {
                                    if let Some(h) = handle {
                                        drop(guard);
                                        h.stop();
                                    }
                                    break;
                                }
                                if let Some(entry) =
                                    guard.routes.iter_mut().find(|r| r.config.id == cfg.id)
                                {
                                    if entry.handle.is_none() && entry.config.enabled {
                                        entry.handle = handle;
                                        entry.status = status;
                                    } else if let Some(h) = handle {
                                        drop(guard);
                                        h.stop();
                                    }
                                } else if let Some(h) = handle {
                                    drop(guard);
                                    h.stop();
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Notify UI/CLI subscribers outside any engine state lock to avoid re-entrancy deadlocks.
    if let Ok(devices) = inner.backend.list_devices() {
        let listeners = inner
            .listeners
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        for cb in listeners {
            cb(devices.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::{default_mock_devices, MockAudioBackend};
    use crate::types::{ChannelMode, LatencyMode};

    fn make_temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("uncord-engine-{tag}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn engine_multi_route_gain_profiles_and_persistence() {
        let dir = make_temp_dir("crud");
        let (backend, _ctrl) = MockAudioBackend::new_with_controller();

        {
            let engine =
                UncordEngine::new_with_backend(Box::new(backend), dir.clone()).expect("init");
            let devices = engine.list_devices().expect("devices");
            assert!(devices.iter().any(|d| d.name.contains("Blackstar") && d.channels == 4));
            assert!(devices
                .iter()
                .any(|d| d.is_bluetooth_hfp_warning && d.name.contains("Hands-Free")));

            let r1 = engine
                .add_route(RouteConfig {
                    id: String::new(),
                    name: "Guitar -> BT".into(),
                    source_device_id: "mock-blackstar-usb".into(),
                    source_is_loopback: false,
                    channel_mode: ChannelMode::StereoPair { left: 2, right: 3 },
                    target_device_id: "mock-bt-a2dp".into(),
                    gain_db: 0.0,
                    muted: false,
                    enabled: true,
                    exclusive_output: false,
                    latency_mode: LatencyMode::UltraLow,
                })
                .expect("add route 1");

            let r2 = engine
                .add_route(RouteConfig {
                    id: String::new(),
                    name: "Loopback -> Monitors".into(),
                    source_device_id: "loopback::mock-wired-monitors".into(),
                    source_is_loopback: true,
                    channel_mode: ChannelMode::StereoPair { left: 0, right: 1 },
                    target_device_id: "mock-wired-monitors".into(),
                    gain_db: -6.0,
                    muted: false,
                    enabled: true,
                    exclusive_output: true,
                    latency_mode: LatencyMode::Low,
                })
                .expect("add route 2");

            // Exclusive mode rejected on Bluetooth A2DP.
            let excl_err = engine.add_route(RouteConfig {
                id: String::new(),
                name: "Bad Exclusive".into(),
                source_device_id: "mock-blackstar-usb".into(),
                source_is_loopback: false,
                channel_mode: ChannelMode::default(),
                target_device_id: "mock-bt-a2dp".into(),
                gain_db: 0.0,
                muted: false,
                enabled: true,
                exclusive_output: true,
                latency_mode: LatencyMode::UltraLow,
            });
            assert!(matches!(excl_err, Err(AudioError::ExclusiveModeRejected(_, _))));

            // Live lock-free gain/mute without restarting route.
            engine
                .set_route_gain_mute(&r1.id, -3.0, true)
                .expect("gain/mute");

            // Save profile, remove a route, then reload profile.
            let profile = engine
                .save_profile("Live Studio".into())
                .expect("save profile");
            assert_eq!(profile.routes.len(), 2);

            engine.remove_route(&r2.id).expect("remove r2");
            assert_eq!(engine.get_snapshot().unwrap().routes.len(), 1);

            let restored = engine.load_profile(&profile.id).expect("load profile");
            assert_eq!(restored.routes.len(), 2);

            let telemetry = engine.get_telemetry();
            assert_eq!(telemetry.len(), 2);
            assert!(telemetry.iter().all(|t| t.status == RouteStatus::Running));

            engine.shutdown();
        }

        // Verify persisted config survives engine restart.
        let (backend2, _) = MockAudioBackend::new_with_controller();
        let engine2 =
            UncordEngine::new_with_backend(Box::new(backend2), dir.clone()).expect("reload");
        let snap = engine2.get_snapshot().unwrap();
        assert_eq!(snap.routes.len(), 2);
        assert_eq!(snap.profiles.len(), 1);
        engine2.shutdown();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_auto_reconnects_route_on_hotplug() {
        let dir = make_temp_dir("hotplug");
        let (backend, ctrl) = MockAudioBackend::new_with_controller();
        let engine = UncordEngine::new_with_backend(Box::new(backend), dir.clone()).expect("init");

        let route = engine
            .add_route(RouteConfig {
                id: "hotplug-1".into(),
                name: "Blackstar -> Wired".into(),
                source_device_id: "mock-blackstar-usb".into(),
                source_is_loopback: false,
                channel_mode: ChannelMode::StereoPair { left: 0, right: 1 },
                target_device_id: "mock-wired-monitors".into(),
                gain_db: 0.0,
                muted: false,
                enabled: true,
                exclusive_output: false,
                latency_mode: LatencyMode::UltraLow,
            })
            .expect("add route");
        assert_eq!(ctrl.start_count(), 1);

        // Simulate unplugging the Blackstar USB amp.
        let blackstar = default_mock_devices()
            .into_iter()
            .find(|d| d.id == "mock-blackstar-usb")
            .unwrap();
        ctrl.remove_device("mock-blackstar-usb");

        let tel_after_unplug = engine.get_telemetry();
        assert_eq!(tel_after_unplug[0].route_id, route.id);
        assert_eq!(tel_after_unplug[0].status, RouteStatus::Reconnecting);

        // Plug Blackstar USB amp back in; engine must auto-reconnect.
        ctrl.add_device(blackstar);
        let tel_after_plug = engine.get_telemetry();
        assert_eq!(tel_after_plug[0].status, RouteStatus::Running);
        assert_eq!(ctrl.start_count(), 2);

        engine.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_update_route_live_vs_restart_and_settings_profiles() {
        let dir = make_temp_dir("update-live");
        let (backend, ctrl) = MockAudioBackend::new_with_controller();
        let engine = UncordEngine::new_with_backend(Box::new(backend), dir.clone()).expect("init");

        let mut route = engine
            .add_route(RouteConfig {
                id: "r-live".into(),
                name: "Blackstar".into(),
                source_device_id: "mock-blackstar-usb".into(),
                source_is_loopback: false,
                channel_mode: ChannelMode::StereoPair { left: 0, right: 1 },
                target_device_id: "mock-wired-monitors".into(),
                gain_db: 0.0,
                muted: false,
                enabled: true,
                exclusive_output: false,
                latency_mode: LatencyMode::UltraLow,
            })
            .expect("add route");
        assert_eq!(ctrl.start_count(), 1);

        // Changing channel_mode / gain_db / muted / name must NOT restart the audio stream.
        route.channel_mode = ChannelMode::MonoToStereo { channel: 3 };
        route.gain_db = -4.0;
        route.name = "Dry DI".into();
        let updated = engine.update_route(route.clone()).expect("live update");
        assert_eq!(updated.channel_mode, ChannelMode::MonoToStereo { channel: 3 });
        assert_eq!(ctrl.start_count(), 1);

        // Changing latency_mode restarts the stream cleanly.
        route.latency_mode = LatencyMode::Balanced;
        engine.update_route(route.clone()).expect("restart update");
        assert_eq!(ctrl.start_count(), 2);

        let p = engine.save_profile("DI Check".into()).expect("save profile");
        assert_eq!(engine.list_profiles().len(), 1);
        engine.delete_profile(&p.id).expect("delete profile");
        assert!(engine.list_profiles().is_empty());

        let updated_settings = engine
            .update_settings(AppSettings {
                minimize_to_tray_on_close: false,
                start_minimized: true,
                auto_reconnect: false,
                update_checks_enabled: false,
                active_profile_id: None,
                theme: "light".into(),
            })
            .expect("update settings");
        assert_eq!(updated_settings.theme, "light");

        engine.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
