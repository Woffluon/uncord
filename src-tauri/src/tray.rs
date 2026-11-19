use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::image::Image;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use uncord_core::engine::UncordEngine;
use uncord_core::types::{EngineSnapshot, Profile, RouteConfig};

use crate::ipc;
use crate::{
    finish_destroy_to_tray, show_or_recreate_main_window, AppLifecycleState, MAIN_WINDOW_LABEL,
};

pub const TRAY_ID: &str = "uncord-tray";
const MENU_SHOW_HIDE: &str = "tray_show_hide";
const MENU_QUICK_MUTE_ALL: &str = "tray_quick_mute_all";
const MENU_PROFILES_EMPTY: &str = "tray_profiles_empty";
const MENU_PROFILE_PREFIX: &str = "tray_profile:";
const MENU_QUIT: &str = "tray_quit";

fn build_tray_menu(
    app: &AppHandle,
    routes: &[RouteConfig],
    profiles: &[Profile],
    active_profile_id: Option<&str>,
) -> tauri::Result<Menu<Wry>> {
    let show_hide = MenuItemBuilder::with_id(MENU_SHOW_HIDE, "Show / Hide uncord").build(app)?;

    let has_routes = !routes.is_empty();
    let all_muted = has_routes && routes.iter().all(|route| route.muted);
    let quick_mute = CheckMenuItemBuilder::with_id(MENU_QUICK_MUTE_ALL, "Quick Mute All")
        .enabled(has_routes)
        .checked(all_muted)
        .build(app)?;

    let mut profiles_submenu = SubmenuBuilder::with_id(app, "tray_profiles", "Profiles");
    if profiles.is_empty() {
        let empty = MenuItemBuilder::with_id(MENU_PROFILES_EMPTY, "No profiles saved")
            .enabled(false)
            .build(app)?;
        profiles_submenu = profiles_submenu.item(&empty);
    } else {
        for profile in profiles {
            let item_id = format!("{MENU_PROFILE_PREFIX}{}", profile.id);
            let is_active = active_profile_id == Some(profile.id.as_str());
            let item = CheckMenuItemBuilder::with_id(item_id, &profile.name)
                .checked(is_active)
                .build(app)?;
            profiles_submenu = profiles_submenu.item(&item);
        }
    }
    let profiles_menu = profiles_submenu.build()?;

    let quit = MenuItemBuilder::with_id(MENU_QUIT, "Quit uncord").build(app)?;

    MenuBuilder::new(app)
        .item(&show_hide)
        .item(&quick_mute)
        .item(&profiles_menu)
        .separator()
        .item(&quit)
        .build()
}

fn load_tray_icon(app: &AppHandle) -> Option<Image<'static>> {
    Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
        .map(Image::to_owned)
        .ok()
        .or_else(|| app.default_window_icon().cloned().map(Image::to_owned))
}

/// Initializes the persistent Windows system tray icon and context menu.
pub fn setup_tray(app: &AppHandle, initial_snapshot: &EngineSnapshot) -> tauri::Result<()> {
    let menu = build_tray_menu(
        app,
        &initial_snapshot.routes,
        &initial_snapshot.profiles,
        initial_snapshot.settings.active_profile_id.as_deref(),
    )?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("uncord — Low-Latency Audio Router")
        .menu(&menu)
        // Left-click directly opens or recreates the main window rather than popping the menu.
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Err(err) = show_or_recreate_main_window(tray.app_handle()) {
                    eprintln!("[uncord] failed to open main window from tray click: {err}");
                }
            }
        })
        .on_menu_event(|app_handle, event| handle_tray_menu_event(app_handle, event.id().as_ref()));

    if let Some(icon) = load_tray_icon(app) {
        builder = builder.icon(icon);
    }

    builder.build(app)?;
    Ok(())
}

/// Refreshes the tray menu off the blocking path without stalling the UI event loop.
pub fn refresh_tray_menu_async(app: &AppHandle) {
    let Some(engine) = app.try_state::<Arc<UncordEngine>>() else {
        return;
    };
    let engine_arc = Arc::clone(engine.inner());
    let app_handle = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let Ok(snapshot) = engine_arc.get_snapshot() else {
            return;
        };
        apply_tray_menu_snapshot(&app_handle, snapshot);
    });
}

fn apply_tray_menu_snapshot(app: &AppHandle, snapshot: EngineSnapshot) {
    let app_for_main = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = app_for_main.tray_by_id(TRAY_ID) else {
            return;
        };
        match build_tray_menu(
            &app_for_main,
            &snapshot.routes,
            &snapshot.profiles,
            snapshot.settings.active_profile_id.as_deref(),
        ) {
            Ok(menu) => {
                if let Err(err) = tray.set_menu(Some(menu)) {
                    eprintln!("[uncord] failed to update tray menu: {err}");
                }
            }
            Err(err) => {
                eprintln!("[uncord] failed to build updated tray menu: {err}");
            }
        }
    });
}

fn handle_tray_menu_event(app: &AppHandle, menu_id: &str) {
    match menu_id {
        MENU_SHOW_HIDE => toggle_main_window(app),
        MENU_QUICK_MUTE_ALL => toggle_quick_mute_all(app),
        MENU_QUIT => quit_application(app),
        _ => {
            if let Some(profile_id) = menu_id.strip_prefix(MENU_PROFILE_PREFIX) {
                activate_profile_from_tray(app, profile_id.to_string());
            }
        }
    }
}

fn toggle_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let is_visible = window.is_visible().unwrap_or(true);
        let is_minimized = window.is_minimized().unwrap_or(false);
        if !is_visible || is_minimized {
            if let Err(err) = show_or_recreate_main_window(app) {
                eprintln!("[uncord] failed to restore main window from tray: {err}");
            }
        } else {
            finish_destroy_to_tray(app, window.destroy());
        }
    } else if let Err(err) = show_or_recreate_main_window(app) {
        eprintln!("[uncord] failed to recreate main window from tray: {err}");
    }
}

fn toggle_quick_mute_all(app: &AppHandle) {
    let Some(engine) = app.try_state::<Arc<UncordEngine>>() else {
        return;
    };
    let engine_arc = Arc::clone(engine.inner());
    let app_handle = app.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let Ok(mut snapshot) = engine_arc.get_snapshot() else {
            return;
        };
        if snapshot.routes.is_empty() {
            return;
        }

        let target_muted = !snapshot.routes.iter().all(|route| route.muted);
        for route in &mut snapshot.routes {
            if engine_arc
                .set_route_gain_mute(&route.id, route.gain_db, target_muted)
                .is_ok()
            {
                route.muted = target_muted;
            }
        }

        if app_handle.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
            let _ = app_handle.emit(ipc::EVENT_ROUTE_STATUS, &snapshot.routes);
        }
        apply_tray_menu_snapshot(&app_handle, snapshot);
    });
}

fn activate_profile_from_tray(app: &AppHandle, profile_id: String) {
    let Some(engine) = app.try_state::<Arc<UncordEngine>>() else {
        return;
    };
    let engine_arc = Arc::clone(engine.inner());
    let lifecycle = app.try_state::<Arc<AppLifecycleState>>().map(|s| Arc::clone(s.inner()));
    let app_handle = app.clone();

    tauri::async_runtime::spawn_blocking(move || match engine_arc.load_profile(&profile_id) {
        Ok(snapshot) => {
            if let Some(lifecycle_state) = lifecycle {
                lifecycle_state.sync_from_settings(&snapshot.settings);
            }
            if app_handle.get_webview_window(MAIN_WINDOW_LABEL).is_some() {
                let _ = app_handle.emit(ipc::EVENT_ROUTE_STATUS, &snapshot.routes);
            }
            apply_tray_menu_snapshot(&app_handle, snapshot);
        }
        Err(err) => {
            eprintln!("[uncord] failed to load profile {profile_id} from tray: {err}");
        }
    });
}

fn quit_application(app: &AppHandle) {
    if let Some(lifecycle) = app.try_state::<Arc<AppLifecycleState>>() {
        lifecycle.is_quitting.store(true, Ordering::SeqCst);
        lifecycle.telemetry_stop.store(true, Ordering::SeqCst);
    }
    if let Some(engine) = app.try_state::<Arc<UncordEngine>>() {
        engine.shutdown();
    }
    app.exit(0);
}
