use crate::events::{log, Event, Level};
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;
use url::Url;

const API_URL: &str = "https://api.github.com/repos/jeremygold02/youtube-to-mp3/releases/latest";
const RELEASES_URL: &str = "https://github.com/jeremygold02/youtube-to-mp3/releases";
const USER_AGENT: &str = "YouTube-Downloader-Rust/1.0";
const HELPER_FILE: &str = "update-helper.exe";
const PAYLOAD_FILE: &str = "update-payload.exe";
const TICKET_FILE: &str = "ticket.json";
const APPLY_ARG: &str = "--apply-rust-update";
const CLEANUP_ARG: &str = "--cleanup-rust-update";
const MAX_UPDATE_SIZE: u64 = 150 * 1024 * 1024;

pub const RUST_ASSET_NAME: &str = "YouTube.to.MP3.Rust.exe";
pub const CURRENT_VERSION: &str = match option_env!("YTDLP_APP_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub fn current_version() -> &'static str {
    CURRENT_VERSION
}

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    pub update_available: bool,
    pub asset_url: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Serialize, Deserialize)]
struct InstallTicket {
    target: PathBuf,
    parent_pid: u32,
}

fn version_parts(version: &str) -> Result<Vec<u64>, String> {
    let value = version.trim().trim_start_matches(['v', 'V']);
    let parts = value
        .split('.')
        .map(|part| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(format!("Invalid release version: {version}"));
            }
            part.parse::<u64>()
                .map_err(|_| format!("Invalid release version: {version}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if parts.is_empty() || parts.len() > 4 {
        return Err(format!("Invalid release version: {version}"));
    }
    Ok(parts)
}

fn is_newer(latest: &str, current: &str) -> Result<bool, String> {
    let mut latest = version_parts(latest)?;
    let mut current = version_parts(current)?;
    let length = latest.len().max(current.len());
    latest.resize(length, 0);
    current.resize(length, 0);
    Ok(latest > current)
}

fn trusted_release_url(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url
                .path()
                .starts_with("/jeremygold02/youtube-to-mp3/releases/tag/")
    })
}

fn trusted_asset_url(value: &str, version: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    let Ok(mut expected) = Url::parse("https://github.com/") else {
        return false;
    };
    let Ok(mut segments) = expected.path_segments_mut() else {
        return false;
    };
    segments.clear().extend([
        "jeremygold02",
        "youtube-to-mp3",
        "releases",
        "download",
        version,
        RUST_ASSET_NAME,
    ]);
    drop(segments);
    url == expected && url.username().is_empty() && url.password().is_none()
}

fn release_info(release: Release, current: &str) -> Result<UpdateInfo, String> {
    let latest = release.tag_name.trim().to_owned();
    let update_available = is_newer(&latest, current)?;
    let asset_url = release
        .assets
        .into_iter()
        .find(|asset| {
            asset.name == RUST_ASSET_NAME && trusted_asset_url(&asset.browser_download_url, &latest)
        })
        .map(|asset| asset.browser_download_url);
    Ok(UpdateInfo {
        current_version: current.to_owned(),
        latest_version: latest,
        release_url: if trusted_release_url(&release.html_url) {
            release.html_url
        } else {
            RELEASES_URL.into()
        },
        update_available,
        asset_url,
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .build()
}

pub fn check_for_update() -> Result<UpdateInfo, String> {
    let response = match agent()
        .get(API_URL)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => {
            return Ok(UpdateInfo {
                current_version: CURRENT_VERSION.into(),
                latest_version: String::new(),
                release_url: RELEASES_URL.into(),
                update_available: false,
                asset_url: None,
            })
        }
        Err(error) => return Err(format!("Could not check releases: {error}")),
    };
    let release: Release = serde_json::from_reader(response.into_reader())
        .map_err(|error| format!("Could not read release information: {error}"))?;
    release_info(release, CURRENT_VERSION)
}

fn valid_redirect(value: &str) -> bool {
    Url::parse(value).ok().is_some_and(|url| {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.host_str().is_some_and(|host| {
                host == "github.com" || host.ends_with(".githubusercontent.com")
            })
    })
}

fn validate_executable(path: &Path) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|error| format!("Could not read downloaded update: {error}"))?;
    let len = file
        .metadata()
        .map_err(|error| format!("Could not inspect downloaded update: {error}"))?
        .len();
    if !(4096..=MAX_UPDATE_SIZE).contains(&len) {
        return Err("The downloaded update has an invalid size.".into());
    }
    let mut header = [0_u8; 64];
    file.read_exact(&mut header)
        .map_err(|_| "The downloaded update is incomplete.")?;
    if &header[..2] != b"MZ" {
        return Err("The downloaded update is not a Windows executable.".into());
    }
    let offset = u32::from_le_bytes(header[60..64].try_into().unwrap()) as u64;
    if offset < 64 || offset > len.saturating_sub(24) {
        return Err("The downloaded update has an invalid executable header.".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| format!("Could not inspect downloaded update: {error}"))?;
    let mut pe = [0_u8; 6];
    file.read_exact(&mut pe)
        .map_err(|_| "The downloaded update is incomplete.")?;
    let machine = u16::from_le_bytes([pe[4], pe[5]]);
    let expected = if cfg!(target_arch = "aarch64") {
        0xaa64
    } else if cfg!(target_arch = "x86") {
        0x014c
    } else {
        0x8664
    };
    if &pe[..4] != b"PE\0\0" || machine != expected {
        return Err("The downloaded update is not a compatible Windows executable.".into());
    }
    Ok(())
}

fn download_update(url: &str, destination: &Path, sender: &Sender<Event>) -> Result<(), String> {
    log(sender, "Downloading app update...", Level::Info);
    let response = agent()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Could not download app update: {error}"))?;
    if !valid_redirect(response.get_url()) {
        return Err("The update download was redirected to an untrusted site.".into());
    }
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    if total.is_some_and(|size| size > MAX_UPDATE_SIZE) {
        return Err("The downloaded update is too large.".into());
    }
    let mut reader = response.into_reader();
    let mut file =
        File::create(destination).map_err(|error| format!("Could not save app update: {error}"))?;
    let mut buffer = [0_u8; 128 * 1024];
    let mut downloaded = 0_u64;
    let mut next_progress = 10;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("Could not download app update: {error}"))?;
        if count == 0 {
            break;
        }
        downloaded += count as u64;
        if downloaded > MAX_UPDATE_SIZE {
            return Err("The downloaded update is too large.".into());
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("Could not save app update: {error}"))?;
        if let Some(size) = total.filter(|value| *value > 0) {
            let percent = (downloaded.saturating_mul(100) / size).min(100);
            if percent >= next_progress {
                log(
                    sender,
                    format!("Downloading app update: {percent}%"),
                    Level::Progress,
                );
                next_progress = (percent / 10 + 1) * 10;
            }
        }
    }
    file.flush()
        .map_err(|error| format!("Could not save app update: {error}"))?;
    drop(file);
    if total.is_some_and(|expected| downloaded != expected) {
        return Err("The downloaded update is incomplete.".into());
    }
    validate_executable(destination)
}

#[cfg(windows)]
pub fn install_update(
    info: &UpdateInfo,
    data_dir: &Path,
    sender: &Sender<Event>,
) -> Result<(), String> {
    let fresh = check_for_update()?;
    if !fresh.update_available
        || fresh.latest_version != info.latest_version
        || fresh.asset_url != info.asset_url
    {
        return Err("The available update changed. Please check again.".into());
    }
    let asset = fresh
        .asset_url
        .as_deref()
        .ok_or("The latest release does not contain the Rust executable.")?;
    if !trusted_asset_url(asset, &fresh.latest_version) {
        return Err("The release contains an untrusted update URL.".into());
    }
    let target = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("Could not locate the running application: {error}"))?;
    let root = data_dir.join("updates");
    fs::create_dir_all(&root)
        .map_err(|error| format!("Could not create update folder: {error}"))?;
    let stage = tempfile::Builder::new()
        .prefix("update-")
        .tempdir_in(&root)
        .map_err(|error| format!("Could not create update folder: {error}"))?;
    download_update(asset, &stage.path().join(PAYLOAD_FILE), sender)?;
    fs::copy(&target, stage.path().join(HELPER_FILE))
        .map_err(|error| format!("Could not prepare update helper: {error}"))?;
    let ticket = InstallTicket {
        target,
        parent_pid: std::process::id(),
    };
    let ticket_path = stage.path().join(TICKET_FILE);
    let ticket_file =
        File::create(&ticket_path).map_err(|error| format!("Could not prepare update: {error}"))?;
    serde_json::to_writer(ticket_file, &ticket)
        .map_err(|error| format!("Could not prepare update: {error}"))?;
    let mut helper = Command::new(stage.path().join(HELPER_FILE));
    helper.arg(APPLY_ARG);
    crate::download::hide_console(&mut helper);
    helper
        .spawn()
        .map_err(|error| format!("Could not start update helper: {error}"))?;
    let _ = stage.keep();
    Ok(())
}

#[cfg(not(windows))]
pub fn install_update(
    _info: &UpdateInfo,
    _data_dir: &Path,
    _sender: &Sender<Event>,
) -> Result<(), String> {
    Err("App updates are only supported on Windows.".into())
}

fn files_match(first: &Path, second: &Path) -> Result<bool, String> {
    let mut left =
        File::open(first).map_err(|error| format!("Could not read the current app: {error}"))?;
    let mut right =
        File::open(second).map_err(|error| format!("Could not read the update helper: {error}"))?;
    if left.metadata().map_err(|error| error.to_string())?.len()
        != right.metadata().map_err(|error| error.to_string())?.len()
    {
        return Ok(false);
    }
    let mut a = [0_u8; 64 * 1024];
    let mut b = [0_u8; 64 * 1024];
    loop {
        let a_len = left.read(&mut a).map_err(|error| error.to_string())?;
        let b_len = right.read(&mut b).map_err(|error| error.to_string())?;
        if a_len != b_len || a[..a_len] != b[..b_len] {
            return Ok(false);
        }
        if a_len == 0 {
            return Ok(true);
        }
    }
}

fn preserve(mut path: tempfile::TempPath) -> PathBuf {
    let location = path.to_path_buf();
    path.disable_cleanup(true);
    let _ = path.keep();
    location
}

fn rollback(backup: tempfile::TempPath, target: &Path, reason: &str) -> String {
    match backup.persist_noclobber(target) {
        Ok(()) => format!("{reason} The previous app was restored."),
        Err(error) => {
            let location = preserve(error.path);
            format!(
                "{reason} Could not restore the previous app: {}. Its backup is at {}.",
                error.error,
                location.display()
            )
        }
    }
}

fn replace_and_launch(
    target: &Path,
    payload: &Path,
    helper: &Path,
    launch: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    if fs::canonicalize(target).ok() == fs::canonicalize(helper).ok()
        || !files_match(target, helper)?
    {
        return Err("The update target is not the original running Rust application.".into());
    }
    let parent = target.parent().ok_or("Could not locate the app folder.")?;
    let mut replacement = tempfile::Builder::new()
        .prefix(".ytmp3-new-")
        .tempfile_in(parent)
        .map_err(|error| format!("Could not prepare replacement: {error}"))?;
    let mut input =
        File::open(payload).map_err(|error| format!("Could not open update: {error}"))?;
    std::io::copy(&mut input, &mut replacement)
        .map_err(|error| format!("Could not prepare replacement: {error}"))?;
    replacement
        .flush()
        .map_err(|error| format!("Could not prepare replacement: {error}"))?;
    replacement
        .as_file()
        .sync_all()
        .map_err(|error| format!("Could not prepare replacement: {error}"))?;
    let backup = tempfile::Builder::new()
        .prefix(".ytmp3-old-")
        .tempfile_in(parent)
        .map_err(|error| format!("Could not prepare backup: {error}"))?
        .into_temp_path();
    fs::rename(target, &backup)
        .map_err(|error| format!("Could not replace the running application: {error}"))?;
    match files_match(&backup, helper) {
        Ok(true) => {}
        Ok(false) => {
            return Err(rollback(
                backup,
                target,
                "The app changed while the update was starting.",
            ))
        }
        Err(error) => {
            return Err(rollback(
                backup,
                target,
                &format!("Could not verify the previous app: {error}."),
            ))
        }
    }
    if let Err(error) = replacement.persist_noclobber(target) {
        return Err(rollback(
            backup,
            target,
            &format!("Could not install the update: {}.", error.error),
        ));
    }
    if let Err(error) = launch(target) {
        let reason = format!("Could not restart the updated app: {error}.");
        if !matches!(files_match(target, payload), Ok(true)) {
            let location = preserve(backup);
            return Err(format!(
                "{reason} The replacement changed; the previous app is at {}.",
                location.display()
            ));
        }
        let displaced = match tempfile::Builder::new()
            .prefix(".ytmp3-failed-")
            .tempfile_in(parent)
        {
            Ok(file) => file.into_temp_path(),
            Err(error) => {
                let location = preserve(backup);
                return Err(format!(
                    "{reason} Could not move the replacement: {error}. The previous app is at {}.",
                    location.display()
                ));
            }
        };
        if let Err(error) = fs::rename(target, &displaced) {
            let location = preserve(backup);
            return Err(format!(
                "{reason} Could not move the replacement: {error}. The previous app is at {}.",
                location.display()
            ));
        }
        if !matches!(files_match(&displaced, payload), Ok(true)) {
            let displaced_result = displaced.persist_noclobber(target);
            let backup_location = preserve(backup);
            return match displaced_result {
                Ok(()) => Err(format!(
                    "{reason} The replacement changed; the previous app is at {}.",
                    backup_location.display()
                )),
                Err(error) => {
                    let displaced_location = preserve(error.path);
                    Err(format!(
                        "{reason} The changed replacement is at {}; the previous app is at {}.",
                        displaced_location.display(),
                        backup_location.display()
                    ))
                }
            };
        }
        let failed_location = preserve(displaced);
        return Err(format!(
            "{} The failed update is at {}.",
            rollback(backup, target, &reason),
            failed_location.display()
        ));
    }
    drop(backup);
    Ok(())
}

#[cfg(windows)]
fn wait_for_process(pid: u32, timeout_ms: u32) -> Result<(), String> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let handle = unsafe { OpenProcess(0x0010_0000, 0, pid) };
    if handle.is_null() {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(87) {
            Ok(())
        } else {
            Err(format!("Could not wait for the app to close: {error}"))
        };
    }
    let status = unsafe { WaitForSingleObject(handle, timeout_ms) };
    unsafe { CloseHandle(handle) };
    match status {
        0 => Ok(()),
        0x102 => Err("The app did not close in time for the update.".into()),
        _ => Err(format!(
            "Could not wait for the app to close: {}",
            std::io::Error::last_os_error()
        )),
    }
}

#[cfg(windows)]
fn valid_stage(stage: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(crate::config::data_dir()?.join("updates"))
        .map_err(|error| format!("Could not locate update folder: {error}"))?;
    let stage =
        fs::canonicalize(stage).map_err(|error| format!("Could not locate update: {error}"))?;
    if stage.parent() != Some(root.as_path())
        || !stage
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("update-"))
    {
        return Err("Invalid update folder.".into());
    }
    Ok(stage)
}

#[cfg(windows)]
fn apply_update() -> Result<(), String> {
    let helper = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|error| format!("Could not find update helper: {error}"))?;
    if helper.file_name() != Some(OsStr::new(HELPER_FILE)) {
        return Err("Invalid update helper.".into());
    }
    let stage = valid_stage(helper.parent().ok_or("Invalid update helper path.")?)?;
    let ticket_file = File::open(stage.join(TICKET_FILE))
        .map_err(|error| format!("Could not read update request: {error}"))?;
    let ticket: InstallTicket = serde_json::from_reader(ticket_file)
        .map_err(|error| format!("Invalid update request: {error}"))?;
    if !ticket.target.is_absolute() {
        return Err("Invalid app update target.".into());
    }
    let payload = stage.join(PAYLOAD_FILE);
    validate_executable(&payload)?;
    wait_for_process(ticket.parent_pid, 60_000)?;
    replace_and_launch(&ticket.target, &payload, &helper, |target| {
        Command::new(target)
            .arg(CLEANUP_ARG)
            .arg(&stage)
            .arg(std::process::id().to_string())
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    })
}

#[cfg(windows)]
fn report_helper_error(error: &str) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(window: isize, text: *const u16, title: *const u16, kind: u32) -> i32;
    }
    let text: Vec<u16> = OsStr::new(error).encode_wide().chain(Some(0)).collect();
    let title: Vec<u16> = OsStr::new("YouTube Downloader update failed")
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MessageBoxW(0, text.as_ptr(), title.as_ptr(), 0x10);
    }
}

#[cfg(windows)]
pub fn handle_update_mode() -> bool {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref() {
        Some(arg) if arg == OsStr::new(APPLY_ARG) => {
            if let Err(error) = apply_update() {
                report_helper_error(&error);
            }
            true
        }
        Some(arg) if arg == OsStr::new(CLEANUP_ARG) => {
            let stage = args.next().map(PathBuf::from);
            let pid = args
                .next()
                .and_then(|value| value.to_str().and_then(|value| value.parse::<u32>().ok()));
            if let (Some(stage), Some(pid)) = (stage, pid) {
                std::thread::spawn(move || {
                    if wait_for_process(pid, 60_000).is_ok() {
                        if let Ok(stage) = valid_stage(&stage) {
                            for name in [HELPER_FILE, PAYLOAD_FILE, TICKET_FILE] {
                                let _ = fs::remove_file(stage.join(name));
                            }
                            let _ = fs::remove_dir(stage);
                        }
                    }
                });
            }
            false
        }
        _ => false,
    }
}

#[cfg(not(windows))]
pub fn handle_update_mode() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: serde_json::Value) -> Release {
        serde_json::from_value(serde_json::json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/jeremygold02/youtube-to-mp3/releases/tag/{tag}"),
            "assets": assets,
        })).unwrap()
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(is_newer("v0.1.10", "0.1.9").unwrap());
        assert!(!is_newer("0.1.0", "0.1").unwrap());
        assert!(!is_newer("0.1.4", "0.1.5").unwrap());
        assert!(is_newer("invalid", "0.1.5").is_err());
    }

    #[test]
    fn accepts_only_named_rust_asset_from_the_release() {
        let python = serde_json::json!([{ "name": "YouTube to MP3.exe", "browser_download_url": "https://github.com/jeremygold02/youtube-to-mp3/releases/download/0.1.6/YouTube%20to%20MP3.exe" }]);
        let info = release_info(release("0.1.6", python), "0.1.5").unwrap();
        assert!(info.update_available);
        assert!(info.asset_url.is_none());
        let rust = serde_json::json!([{ "name": RUST_ASSET_NAME, "browser_download_url": "https://github.com/jeremygold02/youtube-to-mp3/releases/download/0.1.6/YouTube.to.MP3.Rust.exe" }]);
        assert!(release_info(release("0.1.6", rust), "0.1.5")
            .unwrap()
            .asset_url
            .is_some());
        let wrong_host = serde_json::json!([{ "name": RUST_ASSET_NAME, "browser_download_url": "https://example.com/YouTube.to.MP3.Rust.exe" }]);
        assert!(release_info(release("0.1.6", wrong_host), "0.1.5")
            .unwrap()
            .asset_url
            .is_none());
        assert!(!trusted_asset_url("https://github.com/jeremygold02/youtube-to-mp3/releases/download/0.1.5/YouTube.to.MP3.Rust.exe", "0.1.6"));
        assert!(!valid_redirect(
            "https://githubusercontent.com.evil.example/file"
        ));
    }

    #[test]
    fn rejects_invalid_pe_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("invalid.exe");
        fs::write(&file, vec![0_u8; 4096]).unwrap();
        assert!(validate_executable(&file).is_err());
        assert!(validate_executable(&std::env::current_exe().unwrap()).is_ok());
    }

    #[test]
    fn replacement_checks_target_identity_and_rolls_back_failed_launch() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app.exe");
        let helper = dir.path().join("helper.exe");
        let payload = dir.path().join("new.exe");
        fs::write(&target, b"original app").unwrap();
        fs::write(&helper, b"different app").unwrap();
        fs::write(&payload, b"new app").unwrap();
        assert!(replace_and_launch(&target, &payload, &helper, |_| Ok(())).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original app");
        fs::write(&helper, b"original app").unwrap();
        assert!(
            replace_and_launch(&target, &payload, &helper, |_| Err("launch failed".into()))
                .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"original app");
        replace_and_launch(&target, &payload, &helper, |_| Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new app");
        assert_eq!(fs::read(&helper).unwrap(), b"original app");
    }

    #[test]
    fn rollback_preserves_a_concurrently_created_target_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app.exe");
        let mut backup = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        backup.write_all(b"original app").unwrap();
        let backup = backup.into_temp_path();
        let backup_path = backup.to_path_buf();
        fs::write(&target, b"new occupant").unwrap();
        let message = rollback(backup, &target, "Update failed.");
        assert!(message.contains("backup"));
        assert_eq!(fs::read(&target).unwrap(), b"new occupant");
        assert_eq!(fs::read(&backup_path).unwrap(), b"original app");
    }

    #[test]
    fn failed_launch_never_removes_a_changed_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app.exe");
        let helper = dir.path().join("helper.exe");
        let payload = dir.path().join("new.exe");
        fs::write(&target, b"original app").unwrap();
        fs::write(&helper, b"original app").unwrap();
        fs::write(&payload, b"new app").unwrap();
        let result = replace_and_launch(&target, &payload, &helper, |path| {
            fs::write(path, b"changed by another process").unwrap();
            Err("launch failed".into())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&target).unwrap(), b"changed by another process");
        let backups: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ytmp3-old-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(fs::read(backups[0].path()).unwrap(), b"original app");
    }
}
