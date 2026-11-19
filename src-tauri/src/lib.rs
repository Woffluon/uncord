use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use uncord_core::backend::{ActiveRouteHandle, AudioBackend, AudioError};
use uncord_core::engine::UncordEngine;
use uncord_core::types::{
    AppSettings, AudioDeviceInfo, DeviceChangeEvent, EngineSnapshot, RouteConfig,
};

pub mod ipc;
pub mod tray;
pub mod updater;

pub const MAIN_WINDOW_LABEL: &str = "main";

/// Coordinates explicit quit vs close-to-tray lifecycle without blocking the UI thread.
pub struct AppLifecycleState {
    pub is_quitting: AtomicBool,
    pub telemetry_stop: AtomicBool,
    pub minimize_to_tray_on_close: AtomicBool,
}

#[cfg(windows)]
mod win_mem {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn SetProcessWorkingSetSize(
            process: *mut c_void,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
        ) -> i32;
    }

    pub(super) fn trim_working_set() -> bool {
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle and `(usize::MAX, usize::MAX)`
        // (`(SIZE_T)-1`) requests an immediate OS working-set trim of cold UI/WebView2 pages.
        unsafe { SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX) != 0 }
    }
}

/// Releases cold UI/WebView2Loader pages from the OS working set after entering tray-only mode.
pub fn trim_working_set_async(app: &AppHandle) {
    #[cfg(windows)]
    {
        static TRIM_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let gen_id = TRIM_SEQ.fetch_add(1, Ordering::Relaxed) + 1;
        let app_handle = app.clone();
        let _ = std::thread::Builder::new()
            .name("uncord-ws-trim".to_string())
            .spawn(move || {
                for delay_ms in [350_u64, 650_u64] {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                    if TRIM_SEQ.load(Ordering::Relaxed) != gen_id {
                        return;
                    }
                    if app_handle.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
                        return;
                    }
                    let _ = win_mem::trim_working_set();
                }
            });
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

/// Finalizes tray transition after destroying the main WebView2 window and trims cold working-set pages.
pub fn finish_destroy_to_tray(app: &AppHandle, destroy_result: tauri::Result<()>) {
    if let Err(err) = destroy_result {
        eprintln!("[uncord] failed to destroy main window on tray transition: {err}");
        return;
    }
    trim_working_set_async(app);
}

impl AppLifecycleState {
    fn new(settings: &AppSettings) -> Self {
        Self {
            is_quitting: AtomicBool::new(false),
            telemetry_stop: AtomicBool::new(false),
            minimize_to_tray_on_close: AtomicBool::new(settings.minimize_to_tray_on_close),
        }
    }

    pub fn sync_from_settings(&self, settings: &AppSettings) {
        self.minimize_to_tray_on_close
            .store(settings.minimize_to_tray_on_close, Ordering::Relaxed);
    }
}

struct OfflineFallbackBackend {
    reason: String,
}

impl AudioBackend for OfflineFallbackBackend {
    fn list_devices(&self) -> Result<Vec<AudioDeviceInfo>, AudioError> {
        Ok(Vec::new())
    }

    fn start_route(&self, _config: &RouteConfig) -> Result<Box<dyn ActiveRouteHandle>, AudioError> {
        Err(AudioError::BackendInit(self.reason.clone()))
    }

    fn subscribe_device_changes(
        &self,
        _callback: Box<dyn Fn(DeviceChangeEvent) + Send + Sync>,
    ) -> Result<(), AudioError> {
        Ok(())
    }
}

fn fallback_config_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("uncord");
    }
    std::env::temp_dir().join("uncord")
}

fn init_engine() -> Option<Arc<UncordEngine>> {
    match UncordEngine::new_default() {
        Ok(engine) => Some(Arc::new(engine)),
        Err(err) => {
            let reason = err.to_string();
            eprintln!(
                "[uncord] failed to initialize default audio engine ({reason}); using offline fallback backend"
            );
            match UncordEngine::new_with_backend(
                Box::new(OfflineFallbackBackend { reason }),
                fallback_config_dir(),
            ) {
                Ok(fallback) => Some(Arc::new(fallback)),
                Err(fallback_err) => {
                    eprintln!(
                        "[uncord] failed to initialize offline fallback backend: {fallback_err}"
                    );
                    None
                }
            }
        }
    }
}

/// Shows and focuses the main WebView2 window, or recreates it after tray destruction.
pub fn show_or_recreate_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        window.show()?;
        window.set_focus()?;
        return Ok(());
    }

    let mut builder = WebviewWindowBuilder::new(
        app,
        MAIN_WINDOW_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("uncord")
    .inner_size(980.0, 680.0)
    .min_inner_size(760.0, 540.0)
    .resizable(true)
    .center();

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon)?;
    }

    let window = builder.build()?;
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

fn spawn_telemetry_loop(
    app: AppHandle,
    engine: Arc<UncordEngine>,
    lifecycle: Arc<AppLifecycleState>,
) {
    let _ = std::thread::Builder::new()
        .name("uncord-telemetry".to_string())
        .spawn(move || {
            while !lifecycle.telemetry_stop.load(Ordering::Relaxed)
                && !lifecycle.is_quitting.load(Ordering::Relaxed)
            {
                if app.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
                    let telemetry = engine.get_telemetry();
                    let _ = app.emit(ipc::EVENT_TELEMETRY, telemetry);
                    std::thread::sleep(Duration::from_millis(33));
                } else {
                    // Sleep longer when the UI window is destroyed to keep tray idle overhead minimal.
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        });
}

/// Returns true when startup should stay headless in the system tray without creating a window.
pub fn should_start_minimized<'a, I>(args: I, settings: &AppSettings) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    settings.start_minimized || args.into_iter().any(|arg| arg == "--start-minimized")
}

/// Entry point for the uncord Tauri v2 Windows desktop shell.
pub fn run() {
    let start_minimized_arg = std::env::args().any(|arg| arg == "--start-minimized");

    let app = match tauri::Builder::default()
        .setup(move |app| {
            // Initialize audio engine on a fresh MTA-compatible thread if needed so main STA COM is unaffected.
            let engine = std::thread::Builder::new()
                .name("uncord-engine-init".to_string())
                .spawn(init_engine)
                .ok()
                .and_then(|handle| handle.join().ok().flatten())
                .ok_or_else(|| {
                    std::io::Error::other("unable to initialize uncord audio engine or fallback")
                })?;

            let snapshot = engine.get_snapshot().unwrap_or_else(|err| {
                eprintln!("[uncord] initial snapshot failed ({err}); using defaults");
                EngineSnapshot {
                    version: uncord_core::VERSION.to_string(),
                    devices: Vec::new(),
                    routes: Vec::new(),
                    profiles: Vec::new(),
                    settings: AppSettings::default(),
                }
            });

            let lifecycle = Arc::new(AppLifecycleState::new(&snapshot.settings));
            app.manage(Arc::clone(&engine));
            app.manage(Arc::clone(&lifecycle));

            let hotplug_app = app.handle().clone();
            engine.on_device_change(move |devices| {
                if hotplug_app.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
                    let _ = hotplug_app.emit(ipc::EVENT_DEVICES_CHANGED, &devices);
                }
            });

            tray::setup_tray(app.handle(), &snapshot)?;
            spawn_telemetry_loop(
                app.handle().clone(),
                Arc::clone(&engine),
                Arc::clone(&lifecycle),
            );

            let updater = updater::AppUpdater::start(app.handle().clone());
            app.manage(Arc::clone(&updater));

            if snapshot.settings.update_checks_enabled {
                let app_for_update = app.handle().clone();
                let updater_for_startup = Arc::clone(&updater);
                let lifecycle_for_startup = Arc::clone(&lifecycle);
                let _ = std::thread::Builder::new()
                    .name("uncord-update-startup".to_string())
                    .spawn(move || {
                        std::thread::sleep(Duration::from_millis(1500));
                        if !lifecycle_for_startup.is_quitting.load(Ordering::SeqCst) {
                            let _ = updater_for_startup.check_for_updates(&app_for_update);
                        }
                    });
            }

            let arg_flag = if start_minimized_arg {
                ["--start-minimized"].as_slice()
            } else {
                &[]
            };
            if should_start_minimized(arg_flag.iter().copied(), &snapshot.settings) {
                trim_working_set_async(app.handle());
            } else {
                show_or_recreate_main_window(app.handle())?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != MAIN_WINDOW_LABEL {
                    return;
                }
                let app = window.app_handle();
                let Some(lifecycle) = app.try_state::<Arc<AppLifecycleState>>() else {
                    return;
                };
                if lifecycle.is_quitting.load(Ordering::SeqCst) {
                    return;
                }

                if lifecycle.minimize_to_tray_on_close.load(Ordering::Relaxed) {
                    // Prevent Tauri's standard close and destroy the WebView2 process to drop RAM to tray-only.
                    api.prevent_close();
                    finish_destroy_to_tray(app, window.destroy());
                } else {
                    lifecycle.is_quitting.store(true, Ordering::SeqCst);
                    lifecycle.telemetry_stop.store(true, Ordering::SeqCst);
                    if let Some(engine) = app.try_state::<Arc<UncordEngine>>() {
                        engine.shutdown();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_snapshot,
            ipc::list_devices,
            ipc::add_route,
            ipc::update_route,
            ipc::remove_route,
            ipc::set_route_gain_mute,
            ipc::list_profiles,
            ipc::save_profile,
            ipc::load_profile,
            ipc::delete_profile,
            ipc::update_settings,
            ipc::get_telemetry,
            ipc::get_update_state,
            ipc::check_for_updates,
            ipc::cancel_update_check,
            ipc::download_update,
            ipc::install_update,
            ipc::open_release_page,
        ])
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(err) => {
            eprintln!("[uncord] failed to build Tauri desktop shell: {err}");
            return;
        }
    };

    app.run(|app_handle, event| match event {
        RunEvent::ExitRequested { api, .. } => {
            let is_quitting = app_handle
                .try_state::<Arc<AppLifecycleState>>()
                .is_some_and(|state| state.is_quitting.load(Ordering::SeqCst));
            if !is_quitting {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            if let Some(lifecycle) = app_handle.try_state::<Arc<AppLifecycleState>>() {
                lifecycle.telemetry_stop.store(true, Ordering::SeqCst);
            }
            if let Some(updater) = app_handle.try_state::<Arc<updater::AppUpdater>>() {
                updater.shutdown();
            }
            if let Some(engine) = app_handle.try_state::<Arc<UncordEngine>>() {
                engine.shutdown();
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_minimized_respects_cli_flag_and_settings() {
        let mut settings = AppSettings::default();
        assert!(!should_start_minimized(["uncord.exe"], &settings));
        assert!(should_start_minimized(
            ["uncord.exe", "--start-minimized"],
            &settings
        ));

        settings.start_minimized = true;
        assert!(should_start_minimized(["uncord.exe"], &settings));
    }

    #[test]
    fn lifecycle_state_syncs_minimize_to_tray_setting() {
        let mut settings = AppSettings::default();
        let state = AppLifecycleState::new(&settings);
        assert!(state.minimize_to_tray_on_close.load(Ordering::Relaxed));
        assert!(!state.is_quitting.load(Ordering::SeqCst));

        settings.minimize_to_tray_on_close = false;
        state.sync_from_settings(&settings);
        assert!(!state.minimize_to_tray_on_close.load(Ordering::Relaxed));
    }

    #[test]
    fn offline_fallback_backend_reports_reason_without_panicking() {
        let backend = OfflineFallbackBackend {
            reason: "no audio service".to_string(),
        };
        assert!(backend.list_devices().expect("empty device list").is_empty());
        assert!(backend
            .subscribe_device_changes(Box::new(|_| {}))
            .is_ok());

        let dummy_route = RouteConfig {
            id: "r1".to_string(),
            name: "Test".to_string(),
            source_device_id: "s".to_string(),
            source_is_loopback: false,
            channel_mode: uncord_core::types::ChannelMode::default(),
            target_device_id: "t".to_string(),
            gain_db: 0.0,
            muted: false,
            enabled: true,
            exclusive_output: false,
            latency_mode: uncord_core::types::LatencyMode::default(),
        };
        let err = backend
            .start_route(&dummy_route)
            .err()
            .expect("offline fallback must reject route start");
        assert!(err.to_string().contains("no audio service"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_working_set_trim_succeeds() {
        assert!(win_mem::trim_working_set());
    }
}
