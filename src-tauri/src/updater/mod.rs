mod download;
mod manifest;
mod transport;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

pub(crate) use manifest::VerifiedUpdate;

pub const CURRENT_VERSION: &str = uncord_core::VERSION;
pub const EVENT_UPDATE_STATE: &str = "uncord://update-state";

const CHECK_BACKOFF_SCHEDULE: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_millis(1000),
    Duration::from_millis(2000),
];

#[derive(Debug, thiserror::Error)]
pub(crate) enum UpdateFailure {
    #[error("Update operation was cancelled")]
    Cancelled,
    #[error("{0}")]
    Rejected(&'static str),
    #[error("{operation}: {source}")]
    Transport {
        operation: &'static str,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("{operation}: {source}")]
    Manifest {
        operation: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("{operation}: {source}")]
    Version {
        operation: &'static str,
        #[source]
        source: semver::Error,
    },
}

impl UpdateFailure {
    fn is_network_error(&self) -> bool {
        matches!(self, Self::Transport { .. } | Self::Io { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStatusKind {
    Ready,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verified,
    Installing,
    Error,
}

/// Serialized update state exposed to the Tauri frontend via IPC & events.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateSnapshot {
    pub status: UpdateStatusKind,
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub verifying: bool,
    pub error_message: Option<String>,
}

enum UpdateLifecycle {
    Ready,
    Checking,
    UpToDate,
    Available(VerifiedUpdate),
    Downloading {
        update: VerifiedUpdate,
        downloaded: u64,
        total: u64,
        verifying: bool,
    },
    Verified {
        path: PathBuf,
        update: VerifiedUpdate,
    },
    Installing(VerifiedUpdate),
    Error {
        message: String,
        last_available: Option<VerifiedUpdate>,
    },
}

impl UpdateLifecycle {
    fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing(_)
        )
    }

    fn snapshot(&self) -> UpdateStateSnapshot {
        let current_version = CURRENT_VERSION.to_string();
        match self {
            Self::Ready => UpdateStateSnapshot {
                status: UpdateStatusKind::Ready,
                current_version,
                available_version: None,
                release_url: None,
                release_notes: None,
                downloaded_bytes: 0,
                total_bytes: 0,
                verifying: false,
                error_message: None,
            },
            Self::Checking => UpdateStateSnapshot {
                status: UpdateStatusKind::Checking,
                current_version,
                available_version: None,
                release_url: None,
                release_notes: None,
                downloaded_bytes: 0,
                total_bytes: 0,
                verifying: false,
                error_message: None,
            },
            Self::UpToDate => UpdateStateSnapshot {
                status: UpdateStatusKind::UpToDate,
                current_version,
                available_version: None,
                release_url: None,
                release_notes: None,
                downloaded_bytes: 0,
                total_bytes: 0,
                verifying: false,
                error_message: None,
            },
            Self::Available(update) => UpdateStateSnapshot {
                status: UpdateStatusKind::Available,
                current_version,
                available_version: Some(update.version().to_string()),
                release_url: Some(update.release_url().to_string()),
                release_notes: Some(update.release_notes().to_string()),
                downloaded_bytes: 0,
                total_bytes: update.installer_size,
                verifying: false,
                error_message: None,
            },
            Self::Downloading {
                update,
                downloaded,
                total,
                verifying,
            } => UpdateStateSnapshot {
                status: UpdateStatusKind::Downloading,
                current_version,
                available_version: Some(update.version().to_string()),
                release_url: Some(update.release_url().to_string()),
                release_notes: Some(update.release_notes().to_string()),
                downloaded_bytes: *downloaded,
                total_bytes: *total,
                verifying: *verifying,
                error_message: None,
            },
            Self::Verified { update, .. } => UpdateStateSnapshot {
                status: UpdateStatusKind::Verified,
                current_version,
                available_version: Some(update.version().to_string()),
                release_url: Some(update.release_url().to_string()),
                release_notes: Some(update.release_notes().to_string()),
                downloaded_bytes: update.installer_size,
                total_bytes: update.installer_size,
                verifying: false,
                error_message: None,
            },
            Self::Installing(update) => UpdateStateSnapshot {
                status: UpdateStatusKind::Installing,
                current_version,
                available_version: Some(update.version().to_string()),
                release_url: Some(update.release_url().to_string()),
                release_notes: Some(update.release_notes().to_string()),
                downloaded_bytes: update.installer_size,
                total_bytes: update.installer_size,
                verifying: false,
                error_message: None,
            },
            Self::Error {
                message,
                last_available,
            } => UpdateStateSnapshot {
                status: UpdateStatusKind::Error,
                current_version,
                available_version: last_available.as_ref().map(|u| u.version().to_string()),
                release_url: last_available.as_ref().map(|u| u.release_url().to_string()),
                release_notes: last_available.as_ref().map(|u| u.release_notes().to_string()),
                downloaded_bytes: 0,
                total_bytes: last_available.as_ref().map_or(0, |u| u.installer_size),
                verifying: false,
                error_message: Some(message.clone()),
            },
        }
    }
}

enum UpdateCommand {
    Check(Arc<AtomicBool>),
    Download(VerifiedUpdate, Arc<AtomicBool>),
    Shutdown,
}

/// Manages background signed update checks, downloads, integrity verification, and install handoff.
pub struct AppUpdater {
    lifecycle: Mutex<UpdateLifecycle>,
    commands: Sender<UpdateCommand>,
    active_cancel: Mutex<Option<Arc<AtomicBool>>>,
    shutdown_flag: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl AppUpdater {
    pub fn start(app: AppHandle) -> Arc<Self> {
        let (tx, rx) = mpsc::channel();
        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let updater = Arc::new(Self {
            lifecycle: Mutex::new(UpdateLifecycle::Ready),
            commands: tx,
            active_cancel: Mutex::new(None),
            shutdown_flag: Arc::clone(&shutdown_flag),
            worker: Mutex::new(None),
        });

        let worker_updater = Arc::clone(&updater);
        let worker_app = app;
        let handle = std::thread::Builder::new()
            .name("uncord-updater".to_string())
            .spawn(move || {
                run_worker(&rx, &worker_updater, &worker_app, &shutdown_flag);
            })
            .ok();

        if let Ok(mut guard) = updater.worker.lock() {
            *guard = handle;
        }
        updater
    }

    pub fn snapshot(&self) -> UpdateStateSnapshot {
        self.lifecycle
            .lock()
            .map(|g| g.snapshot())
            .unwrap_or_else(|e| e.into_inner().snapshot())
    }

    pub fn check_for_updates(&self, app: &AppHandle) -> UpdateStateSnapshot {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut lc = self
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if lc.is_busy() {
                return lc.snapshot();
            }
            *lc = UpdateLifecycle::Checking;
        }
        if let Ok(mut guard) = self.active_cancel.lock() {
            *guard = Some(Arc::clone(&cancel));
        }
        let snap = self.snapshot();
        emit_state_if_visible(app, &snap);
        if self.commands.send(UpdateCommand::Check(cancel)).is_err() {
            self.set_error(app, "Update worker unavailable", None);
        }
        self.snapshot()
    }

    pub fn cancel_active(&self, app: &AppHandle) -> UpdateStateSnapshot {
        if let Ok(guard) = self.active_cancel.lock() {
            if let Some(flag) = guard.as_ref() {
                flag.store(true, Ordering::Release);
            }
        }
        let snap = {
            let mut lc = self
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match &*lc {
                UpdateLifecycle::Checking => {
                    *lc = UpdateLifecycle::Ready;
                }
                UpdateLifecycle::Downloading { update, .. } => {
                    *lc = UpdateLifecycle::Available(update.clone());
                }
                _ => {}
            }
            lc.snapshot()
        };
        emit_state_if_visible(app, &snap);
        snap
    }

    pub fn download_update(&self, app: &AppHandle) -> Result<UpdateStateSnapshot, String> {
        let update = {
            let mut lc = self
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let available = match &*lc {
                UpdateLifecycle::Available(u) => u.clone(),
                UpdateLifecycle::Error {
                    last_available: Some(u),
                    ..
                } => u.clone(),
                UpdateLifecycle::Verified { .. } => return Ok(lc.snapshot()),
                UpdateLifecycle::Downloading { .. } => return Ok(lc.snapshot()),
                _ => return Err("No verified update is available to download".to_string()),
            };
            *lc = UpdateLifecycle::Downloading {
                update: available.clone(),
                downloaded: 0,
                total: available.installer_size,
                verifying: false,
            };
            available
        };

        let cancel = Arc::new(AtomicBool::new(false));
        if let Ok(mut guard) = self.active_cancel.lock() {
            *guard = Some(Arc::clone(&cancel));
        }
        let snap = self.snapshot();
        emit_state_if_visible(app, &snap);
        if self
            .commands
            .send(UpdateCommand::Download(update, cancel))
            .is_err()
        {
            self.set_error(app, "Update worker unavailable", None);
            return Err("Update worker unavailable".to_string());
        }
        Ok(self.snapshot())
    }

    pub fn install_update(&self, app: &AppHandle) -> Result<(), String> {
        let (path, update) = {
            let mut lc = self
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let (p, u) = match &*lc {
                UpdateLifecycle::Verified { path, update } => (path.clone(), update.clone()),
                _ => return Err("No verified installer is ready to launch".to_string()),
            };
            *lc = UpdateLifecycle::Installing(u.clone());
            (p, u)
        };
        emit_state_if_visible(app, &self.snapshot());

        if let Err(err) = download::launch_verified_installer(&path, &update) {
            self.set_error(app, &err.to_string(), Some(update));
            return Err(err.to_string());
        }
        Ok(())
    }

    pub fn open_release_page(&self) -> Result<(), String> {
        let lc = self
            .lifecycle
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let update = match &*lc {
            UpdateLifecycle::Available(u)
            | UpdateLifecycle::Downloading { update: u, .. }
            | UpdateLifecycle::Verified { update: u, .. }
            | UpdateLifecycle::Installing(u)
            | UpdateLifecycle::Error {
                last_available: Some(u),
                ..
            } => u.clone(),
            _ => return Err("No update release URL is available".to_string()),
        };
        download::open_validated_release_page(update.release_url(), update.version())
            .map_err(|e| e.to_string())
    }

    pub fn shutdown(&self) {
        self.shutdown_flag.store(true, Ordering::Release);
        if let Ok(guard) = self.active_cancel.lock() {
            if let Some(flag) = guard.as_ref() {
                flag.store(true, Ordering::Release);
            }
        }
        let _ = self.commands.send(UpdateCommand::Shutdown);
        if let Ok(mut guard) = self.worker.lock() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
    }

    fn set_error(&self, app: &AppHandle, message: &str, last_available: Option<VerifiedUpdate>) {
        let snap = {
            let mut lc = self
                .lifecycle
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *lc = UpdateLifecycle::Error {
                message: message.to_string(),
                last_available,
            };
            lc.snapshot()
        };
        emit_state_if_visible(app, &snap);
    }
}

fn emit_state_if_visible(app: &AppHandle, snap: &UpdateStateSnapshot) {
    if app.get_webview_window(crate::MAIN_WINDOW_LABEL).is_some() {
        let _ = app.emit(EVENT_UPDATE_STATE, snap);
    }
}

fn sleep_with_cancellation(
    duration: Duration,
    worker_cancelled: &AtomicBool,
    check_cancelled: &AtomicBool,
) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < duration {
        if worker_cancelled.load(Ordering::Acquire) || check_cancelled.load(Ordering::Acquire) {
            return false;
        }
        let remaining = duration.saturating_sub(start.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(25)));
    }
    !worker_cancelled.load(Ordering::Acquire) && !check_cancelled.load(Ordering::Acquire)
}

fn check_with_retry<F>(
    mut check: F,
    check_cancelled: &AtomicBool,
    worker_cancelled: &AtomicBool,
    backoff_schedule: &[Duration],
) -> Result<Option<VerifiedUpdate>, UpdateFailure>
where
    F: FnMut() -> Result<Option<VerifiedUpdate>, UpdateFailure>,
{
    let mut attempt = 0;
    loop {
        if worker_cancelled.load(Ordering::Acquire) || check_cancelled.load(Ordering::Acquire) {
            return Err(UpdateFailure::Cancelled);
        }
        match check() {
            Ok(result) => return Ok(result),
            Err(error) => {
                if worker_cancelled.load(Ordering::Acquire)
                    || check_cancelled.load(Ordering::Acquire)
                {
                    return Err(UpdateFailure::Cancelled);
                }
                if error.is_network_error() && attempt < backoff_schedule.len() {
                    let backoff = backoff_schedule[attempt];
                    attempt += 1;
                    if !sleep_with_cancellation(backoff, worker_cancelled, check_cancelled) {
                        return Err(UpdateFailure::Cancelled);
                    }
                } else {
                    return Err(error);
                }
            }
        }
    }
}

fn run_worker(
    commands: &Receiver<UpdateCommand>,
    updater: &AppUpdater,
    app: &AppHandle,
    shutdown_flag: &AtomicBool,
) {
    let agent = transport::new_agent();
    while let Ok(command) = commands.recv() {
        if shutdown_flag.load(Ordering::Acquire) {
            break;
        }
        match command {
            UpdateCommand::Shutdown => break,
            UpdateCommand::Check(check_cancelled) => {
                let res = check_with_retry(
                    || manifest::check_for_update(&agent, &check_cancelled),
                    &check_cancelled,
                    shutdown_flag,
                    &CHECK_BACKOFF_SCHEDULE,
                );
                let snap = {
                    let mut lc = updater
                        .lifecycle
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    match res {
                        Ok(Some(update)) => *lc = UpdateLifecycle::Available(update),
                        Ok(None) => *lc = UpdateLifecycle::UpToDate,
                        Err(UpdateFailure::Cancelled) => *lc = UpdateLifecycle::Ready,
                        Err(err) => {
                            *lc = UpdateLifecycle::Error {
                                message: err.to_string(),
                                last_available: None,
                            };
                        }
                    }
                    lc.snapshot()
                };
                emit_state_if_visible(app, &snap);
            }
            UpdateCommand::Download(update, dl_cancelled) => {
                let res = download::download_update(
                    &agent,
                    &update,
                    &dl_cancelled,
                    |downloaded, total, verifying| {
                        let snap = {
                            let mut lc = updater
                                .lifecycle
                                .lock()
                                .unwrap_or_else(|e| e.into_inner());
                            if let UpdateLifecycle::Downloading {
                                downloaded: d,
                                total: t,
                                verifying: v,
                                ..
                            } = &mut *lc
                            {
                                *d = downloaded;
                                *t = total;
                                *v = verifying;
                            }
                            lc.snapshot()
                        };
                        emit_state_if_visible(app, &snap);
                    },
                );
                let snap = {
                    let mut lc = updater
                        .lifecycle
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    match res {
                        Ok(path) => *lc = UpdateLifecycle::Verified { path, update },
                        Err(UpdateFailure::Cancelled) => *lc = UpdateLifecycle::Available(update),
                        Err(err) => {
                            *lc = UpdateLifecycle::Error {
                                message: err.to_string(),
                                last_available: Some(update),
                            };
                        }
                    }
                    lc.snapshot()
                };
                emit_state_if_visible(app, &snap);
            }
        }
    }
}
