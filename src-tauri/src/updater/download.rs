use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use semver::Version;
use sha2::{Digest, Sha256};
use ureq::Agent;

use super::manifest::{expected_file_version, validate_installer_url, validate_release_url, VerifiedUpdate};
use super::transport::{check_cancelled, get_with_redirects, RequestTarget};
use super::UpdateFailure;

#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

const INSTALLER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const UI_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(80);
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

static VERIFIED_INSTALLER_LOCK: Mutex<Option<(PathBuf, File)>> = Mutex::new(None);

fn retain_verified_installer_lock(path: &Path, file: File) {
    let mut guard = match VERIFIED_INSTALLER_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *guard = Some((path.to_path_buf(), file));
}

fn release_verified_installer_lock(path: &Path) {
    let mut guard = match VERIFIED_INSTALLER_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard
        .as_ref()
        .is_some_and(|(locked_path, _)| locked_path == path)
    {
        *guard = None;
    }
}

#[cfg(windows)]
fn update_directory() -> Result<PathBuf, UpdateFailure> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or(UpdateFailure::Rejected("LOCALAPPDATA is unavailable"))?;
    Ok(PathBuf::from(local_app_data).join("uncord").join("Updates"))
}

#[cfg(not(windows))]
fn update_directory() -> Result<PathBuf, UpdateFailure> {
    if let Some(cache_home) = std::env::var_os("XDG_CACHE_HOME") {
        Ok(PathBuf::from(cache_home).join("uncord").join("updates"))
    } else if let Some(home) = std::env::var_os("HOME") {
        Ok(PathBuf::from(home).join(".cache").join("uncord").join("updates"))
    } else {
        Ok(std::env::temp_dir().join("uncord-updates"))
    }
}

fn sha256_hex_digest(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

pub(super) fn download_update<F>(
    agent: &Agent,
    update: &VerifiedUpdate,
    cancelled: &AtomicBool,
    mut on_progress: F,
) -> Result<PathBuf, UpdateFailure>
where
    F: FnMut(u64, u64, bool),
{
    on_progress(0, update.installer_size, false);
    download_update_inner(agent, update, cancelled, &mut on_progress)
}

fn download_update_inner<F>(
    agent: &Agent,
    update: &VerifiedUpdate,
    cancelled: &AtomicBool,
    on_progress: &mut F,
) -> Result<PathBuf, UpdateFailure>
where
    F: FnMut(u64, u64, bool),
{
    validate_installer_url(&update.installer_url, update.version())?;
    let directory = update_directory()?;
    fs::create_dir_all(&directory).map_err(|source| UpdateFailure::Io {
        operation: "Could not create update directory",
        source,
    })?;
    let final_path = directory.join(format!("uncord_{}_x64-setup.exe", update.version()));
    let (partial_path, partial_file) = create_unique_partial(&directory, update.version())?;
    let result = download_to_partial(agent, update, partial_file, cancelled, on_progress).and_then(
        |()| {
            check_cancelled(cancelled)?;
            on_progress(update.installer_size, update.installer_size, true);
            verify_download(&partial_path, update)?;
            release_verified_installer_lock(&partial_path);
            release_verified_installer_lock(&final_path);
            check_cancelled(cancelled)?;
            if final_path.is_file() {
                fs::remove_file(&final_path).map_err(|source| UpdateFailure::Io {
                    operation: "Could not replace previous update",
                    source,
                })?;
            }
            fs::rename(&partial_path, &final_path).map_err(|source| UpdateFailure::Io {
                operation: "Could not finalize update",
                source,
            })?;
            verify_download(&final_path, update)?;
            Ok(final_path.clone())
        },
    );
    finish_download(result, &partial_path)
}

fn finish_download(
    result: Result<PathBuf, UpdateFailure>,
    partial_path: &Path,
) -> Result<PathBuf, UpdateFailure> {
    if result.is_err() {
        release_verified_installer_lock(partial_path);
        let _ = fs::remove_file(partial_path);
    }
    result
}

fn create_unique_partial(
    directory: &Path,
    version: &str,
) -> Result<(PathBuf, File), UpdateFailure> {
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for attempt in 0..16_u32 {
        let candidate = directory.join(format!("uncord_{version}_x64-setup.{pid}.{nanos}.{attempt}.part"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => return Ok((candidate, file)),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(source) => {
                return Err(UpdateFailure::Io {
                    operation: "Could not create temporary update file",
                    source,
                });
            }
        }
    }
    Err(UpdateFailure::Rejected(
        "Could not allocate unique temporary update file",
    ))
}

fn download_to_partial<F>(
    agent: &Agent,
    update: &VerifiedUpdate,
    mut partial_file: File,
    cancelled: &AtomicBool,
    on_progress: &mut F,
) -> Result<(), UpdateFailure>
where
    F: FnMut(u64, u64, bool),
{
    let (mut response, _) = get_with_redirects(
        agent,
        &update.installer_url,
        RequestTarget::Installer(update.version()),
        INSTALLER_TIMEOUT,
        cancelled,
    )?;

    let mut reader = response.body_mut().as_reader();
    let mut buffer = [0_u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut hasher = Sha256::new();
    let mut last_emit = Instant::now();

    loop {
        check_cancelled(cancelled)?;
        let read = reader
            .read(&mut buffer)
            .map_err(|source| UpdateFailure::Io {
                operation: "Could not read update installer stream",
                source,
            })?;
        if read == 0 {
            break;
        }
        downloaded = downloaded
            .checked_add(read as u64)
            .ok_or(UpdateFailure::Rejected("Update installer size overflow"))?;
        if downloaded > update.installer_size {
            return Err(UpdateFailure::Rejected(
                "Downloaded installer exceeds manifest size",
            ));
        }
        hasher.update(&buffer[..read]);
        partial_file
            .write_all(&buffer[..read])
            .map_err(|source| UpdateFailure::Io {
                operation: "Could not write temporary update file",
                source,
            })?;
        if last_emit.elapsed() >= UI_PROGRESS_EMIT_INTERVAL {
            on_progress(downloaded, update.installer_size, false);
            last_emit = Instant::now();
        }
    }

    if downloaded != update.installer_size {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer size does not match manifest",
        ));
    }
    if sha256_hex_digest(hasher.finalize().as_ref()) != update.installer_sha256 {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer SHA-256 does not match manifest",
        ));
    }
    partial_file.sync_all().map_err(|source| UpdateFailure::Io {
        operation: "Could not flush downloaded update file",
        source,
    })?;
    Ok(())
}

pub(super) fn verify_download(path: &Path, update: &VerifiedUpdate) -> Result<(), UpdateFailure> {
    let locked_file = verify_file_integrity(path, update.installer_size, &update.installer_sha256)?;
    #[cfg(windows)]
    {
        let actual_version = executable_file_version(path)?;
        let expected_version = Version::parse(update.version())
            .map_err(|source| UpdateFailure::Version {
                operation: "Invalid release version",
                source,
            })
            .and_then(|version| expected_file_version(&version))?;
        if actual_version != expected_version {
            return Err(UpdateFailure::Rejected(
                "Downloaded installer version does not match update metadata",
            ));
        }
    }
    retain_verified_installer_lock(path, locked_file);
    Ok(())
}

fn verify_file_integrity(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<File, UpdateFailure> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.share_mode(FILE_SHARE_READ);
    let mut file = options.open(path).map_err(|source| UpdateFailure::Io {
        operation: "Could not reopen downloaded update",
        source,
    })?;
    let metadata = file.metadata().map_err(|source| UpdateFailure::Io {
        operation: "Could not inspect downloaded update",
        source,
    })?;
    #[cfg(windows)]
    if !metadata.is_file() || (metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer must be a regular non-reparse file",
        ));
    }
    #[cfg(not(windows))]
    if !metadata.is_file() {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer must be a regular file",
        ));
    }
    if metadata.len() != expected_size {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer size changed after download",
        ));
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|source| UpdateFailure::Io {
            operation: "Could not rehash downloaded update",
            source,
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if sha256_hex_digest(hasher.finalize().as_ref()) != expected_sha256 {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer changed after verification",
        ));
    }
    Ok(file)
}

pub(super) fn launch_verified_installer(
    path: &Path,
    update: &VerifiedUpdate,
) -> Result<(), UpdateFailure> {
    verify_download(path, update)?;
    let _locked = lock_executable_for_launch(path)?;
    Command::new(path)
        .spawn()
        .map(|_| ())
        .map_err(|source| UpdateFailure::Io {
            operation: "Could not start installer",
            source,
        })
}

fn lock_executable_for_launch(path: &Path) -> Result<File, UpdateFailure> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    options.share_mode(FILE_SHARE_READ);
    let file = options.open(path).map_err(|source| UpdateFailure::Io {
        operation: "Could not lock installer for launch",
        source,
    })?;
    let metadata = file.metadata().map_err(|source| UpdateFailure::Io {
        operation: "Could not inspect locked installer",
        source,
    })?;
    #[cfg(windows)]
    if !metadata.is_file() || (metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(UpdateFailure::Rejected(
            "Installer must be a regular non-reparse file",
        ));
    }
    #[cfg(not(windows))]
    if !metadata.is_file() {
        return Err(UpdateFailure::Rejected("Installer must be a regular file"));
    }
    Ok(file)
}

pub(super) fn open_validated_release_page(url: &str, version: &str) -> Result<(), UpdateFailure> {
    validate_release_url(url, version)?;
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide_url: Vec<u16> = OsStr::new(url).encode_wide().chain(Some(0)).collect();
        let wide_verb: Vec<u16> = OsStr::new("open").encode_wide().chain(Some(0)).collect();
        // SAFETY: `wide_verb` and `wide_url` are valid NUL-terminated UTF-16 buffers alive for this call.
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(wide_verb.as_ptr()),
                PCWSTR(wide_url.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        if (result.0 as usize) <= 32 {
            return Err(UpdateFailure::Rejected("Could not open browser for release URL"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = url;
        Err(UpdateFailure::Rejected("Opening release page is Windows-only"))
    }
}

#[cfg(windows)]
fn executable_file_version(path: &Path) -> Result<[u16; 4], UpdateFailure> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
    };

    let wide_path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut ignored = 0_u32;
    // SAFETY: `wide_path` is a live NUL-terminated UTF-16 path and `ignored` is a valid mutable u32.
    let byte_len = unsafe { GetFileVersionInfoSizeW(PCWSTR(wide_path.as_ptr()), Some(&mut ignored)) };
    if byte_len == 0 {
        return Err(UpdateFailure::Rejected(
            "Could not inspect downloaded installer version metadata",
        ));
    }

    let mut buffer = vec![0_u8; byte_len as usize];
    // SAFETY: `buffer` has length `byte_len` and stays alive for the call.
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(wide_path.as_ptr()),
            Some(0),
            byte_len,
            buffer.as_mut_ptr().cast::<c_void>(),
        )
    }
    .map_err(|_| UpdateFailure::Rejected("Could not read downloaded installer version metadata"))?;

    let root_sub_block: Vec<u16> = ['\\' as u16, 0].into();
    let mut info_ptr: *mut c_void = std::ptr::null_mut();
    let mut info_len = 0_u32;
    // SAFETY: `buffer` contains valid version info and `info_ptr`/`info_len` are valid out pointers.
    let ok = unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast::<c_void>(),
            PCWSTR(root_sub_block.as_ptr()),
            &mut info_ptr,
            &mut info_len,
        )
    };
    if !ok.as_bool() || info_ptr.is_null() || (info_len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer has no valid PE version header",
        ));
    }

    // SAFETY: `VerQueryValueW` validated `info_len >= size_of::<VS_FIXEDFILEINFO>()`.
    let fixed = unsafe { std::ptr::read_unaligned(info_ptr.cast::<VS_FIXEDFILEINFO>()) };
    if fixed.dwSignature != 0xFEEF_04BD {
        return Err(UpdateFailure::Rejected(
            "Downloaded installer PE version signature is invalid",
        ));
    }

    Ok(fixed_file_version(fixed.dwFileVersionMS, fixed.dwFileVersionLS))
}

#[cfg(any(windows, test))]
fn fixed_file_version(ms: u32, ls: u32) -> [u16; 4] {
    let ms = ms.to_be_bytes();
    let ls = ls.to_be_bytes();
    [
        u16::from_be_bytes([ms[0], ms[1]]),
        u16::from_be_bytes([ms[2], ms[3]]),
        u16::from_be_bytes([ls[0], ls[1]]),
        u16::from_be_bytes([ls[2], ls[3]]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_windows_version_words_are_decoded_in_order() {
        assert_eq!(fixed_file_version(0x0001_0002, 0x0003_0000), [1, 2, 3, 0]);
    }

    #[test]
    fn verify_download_checks_sha256_and_size() {
        let dir = std::env::temp_dir().join(format!(
            "uncord-dl-verify-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("payload.bin");
        let payload = b"uncord-verified-payload";
        fs::write(&path, payload).expect("write payload");

        let mut hasher = Sha256::new();
        hasher.update(payload);
        let hash = sha256_hex_digest(hasher.finalize().as_ref());

        // `verify_file_integrity` validates exact bytes and SHA-256 without needing a full PE file.
        assert!(verify_file_integrity(&path, payload.len() as u64, &hash).is_ok());
        assert!(verify_file_integrity(&path, payload.len() as u64 + 1, &hash).is_err());
        assert!(verify_file_integrity(&path, payload.len() as u64, &"0".repeat(64)).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn executable_launch_lock_denies_concurrent_writes() {
        let dir = std::env::temp_dir().join(format!(
            "uncord-exe-lock-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("setup.exe");
        fs::write(&path, b"MZ-verified-payload").expect("write setup");

        let lock = lock_executable_for_launch(&path).expect("read-share lock succeeds");
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        drop(lock);
        let _ = fs::remove_dir_all(&dir);
    }
}
