use crate::config;
use crate::events::{log, Event, Level};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const FFMPEG_BASE: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest";
const USER_AGENT: &str = "YouTube-to-MP3-Rust/1.0";

#[derive(Clone, Debug)]
pub struct Tools {
    pub yt_dlp: PathBuf,
    pub ffmpeg_dir: Option<PathBuf>,
    pub private_dir: PathBuf,
}

pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os("PATH")?;
    std::env::split_paths(&value)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn ffmpeg_pair(dir: &Path) -> bool {
    dir.join("ffmpeg.exe").is_file() && dir.join("ffprobe.exe").is_file()
}

pub fn resolve(data_dir: &Path, sender: &Sender<Event>) -> Result<Tools, String> {
    let private_dir = data_dir.join("tools");
    let previous_dir = config::previous_data_dir(data_dir).map(|dir| dir.join("tools"));
    let original_dir = config::original_download_dir();
    let private_ytdlp = private_dir.join("yt-dlp.exe");
    let previous_ytdlp = previous_dir.as_ref().map(|dir| dir.join("yt-dlp.exe"));
    let original_ytdlp = original_dir.as_ref().map(|dir| dir.join("yt-dlp.exe"));
    let yt_dlp = if private_ytdlp.is_file() {
        private_ytdlp
    } else if let Some(path) = previous_ytdlp.filter(|path| path.is_file()) {
        path
    } else if let Some(path) = original_ytdlp.filter(|path| path.is_file()) {
        path
    } else if let Some(path) = find_on_path("yt-dlp.exe") {
        path
    } else {
        install_ytdlp(&private_dir, sender)?
    };

    let has_deno = private_dir.join("deno.exe").is_file()
        || previous_dir
            .as_ref()
            .is_some_and(|dir| dir.join("deno.exe").is_file())
        || yt_dlp
            .parent()
            .is_some_and(|dir| dir.join("deno.exe").is_file())
        || find_on_path("deno.exe").is_some();
    if !has_deno {
        install_deno(&private_dir, sender)?;
    }

    let ffmpeg_dir = if ffmpeg_pair(&private_dir) {
        Some(private_dir.clone())
    } else if let Some(dir) = previous_dir.filter(|dir| ffmpeg_pair(dir)) {
        Some(dir)
    } else if let Some(dir) = original_dir.filter(|dir| ffmpeg_pair(dir)) {
        Some(dir)
    } else if find_on_path("ffmpeg.exe").is_some() && find_on_path("ffprobe.exe").is_some() {
        None
    } else {
        install_ffmpeg(&private_dir, sender)?;
        Some(private_dir.clone())
    };

    Ok(Tools {
        yt_dlp,
        ffmpeg_dir,
        private_dir,
    })
}

fn download(
    url: &str,
    destination: &Path,
    label: &str,
    sender: &Sender<Event>,
) -> Result<(), String> {
    log(sender, format!("Downloading {label}..."), Level::Info);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(300))
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|error| format!("Could not download {label}: {error}"))?;
    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut output =
        File::create(destination).map_err(|error| format!("Could not save {label}: {error}"))?;
    let mut buffer = [0_u8; 128 * 1024];
    let mut downloaded = 0_u64;
    let mut next_progress = 10;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("Could not download {label}: {error}"))?;
        if count == 0 {
            break;
        }
        output
            .write_all(&buffer[..count])
            .map_err(|error| format!("Could not save {label}: {error}"))?;
        downloaded += count as u64;
        if let Some(total) = total.filter(|value| *value > 0) {
            let percent = (downloaded.saturating_mul(100) / total).min(100);
            if percent >= next_progress {
                log(
                    sender,
                    format!("Downloading {label}: {percent}%"),
                    Level::Progress,
                );
                next_progress = (percent / 10 + 1) * 10;
            }
        }
    }
    output
        .flush()
        .map_err(|error| format!("Could not save {label}: {error}"))?;
    Ok(())
}

fn install_file(source: &Path, destination: &Path, label: &str) -> Result<(), String> {
    fs::rename(source, destination).map_err(|error| format!("Could not install {label}: {error}"))
}

fn extract_named(
    archive_path: &Path,
    temporary_dir: &Path,
    filename: &str,
) -> Result<PathBuf, String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("Could not open downloaded archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("Invalid downloaded archive: {error}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Invalid downloaded archive: {error}"))?;
        let matches = entry
            .name()
            .rsplit('/')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(filename));
        if !entry.is_dir() && matches {
            let output_path = temporary_dir.join(filename);
            let mut output = File::create(&output_path)
                .map_err(|error| format!("Could not extract {filename}: {error}"))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("Could not extract {filename}: {error}"))?;
            return Ok(output_path);
        }
    }
    Err(format!(
        "The downloaded archive did not contain {filename}."
    ))
}

fn tool_staging(dir: &Path) -> Result<tempfile::TempDir, String> {
    fs::create_dir_all(dir).map_err(|error| format!("Could not create tools folder: {error}"))?;
    tempfile::Builder::new()
        .prefix(".tool-")
        .tempdir_in(dir)
        .map_err(|error| format!("Could not create a temporary tools folder: {error}"))
}

fn install_ytdlp(dir: &Path, sender: &Sender<Event>) -> Result<PathBuf, String> {
    let temporary = tool_staging(dir)?;
    let downloaded = temporary.path().join("yt-dlp.exe");
    download(YTDLP_URL, &downloaded, "yt-dlp", sender)?;
    let destination = dir.join("yt-dlp.exe");
    install_file(&downloaded, &destination, "yt-dlp")?;
    log(sender, "yt-dlp is ready.", Level::Success);
    Ok(destination)
}

fn install_ffmpeg(dir: &Path, sender: &Sender<Event>) -> Result<(), String> {
    let platform = match std::env::consts::ARCH {
        "aarch64" => "winarm64",
        "x86" => "win32",
        _ => "win64",
    };
    let url = format!("{FFMPEG_BASE}/ffmpeg-master-latest-{platform}-gpl.zip");
    let temporary = tool_staging(dir)?;
    let archive = temporary.path().join("ffmpeg.zip");
    download(&url, &archive, "FFmpeg", sender)?;
    let ffmpeg = extract_named(&archive, temporary.path(), "ffmpeg.exe")?;
    let ffprobe = extract_named(&archive, temporary.path(), "ffprobe.exe")?;
    install_file(&ffmpeg, &dir.join("ffmpeg.exe"), "FFmpeg")?;
    install_file(&ffprobe, &dir.join("ffprobe.exe"), "FFprobe")?;
    log(sender, "FFmpeg is ready.", Level::Success);
    Ok(())
}

fn install_deno(dir: &Path, sender: &Sender<Event>) -> Result<(), String> {
    let arch = if std::env::consts::ARCH == "aarch64" {
        "aarch64"
    } else {
        "x86_64"
    };
    let url = format!(
        "https://github.com/denoland/deno/releases/latest/download/deno-{arch}-pc-windows-msvc.zip"
    );
    let temporary = tool_staging(dir)?;
    let archive = temporary.path().join("deno.zip");
    download(&url, &archive, "Deno", sender)?;
    let deno = extract_named(&archive, temporary.path(), "deno.exe")?;
    install_file(&deno, &dir.join("deno.exe"), "Deno")?;
    log(sender, "Deno is ready.", Level::Success);
    Ok(())
}

pub fn update_tools(data_dir: &Path, sender: &Sender<Event>) -> Result<(), String> {
    let dir = data_dir.join("tools");
    log(sender, "Updating download tools...", Level::Info);
    let mut errors = Vec::new();
    if let Err(error) = install_ytdlp(&dir, sender) {
        errors.push(error);
    }
    if let Err(error) = install_deno(&dir, sender) {
        errors.push(error);
    }
    let system_ffmpeg =
        find_on_path("ffmpeg.exe").is_some() && find_on_path("ffprobe.exe").is_some();
    let original_ffmpeg = config::original_download_dir().is_some_and(|path| ffmpeg_pair(&path));
    let previous_ffmpeg =
        config::previous_data_dir(data_dir).is_some_and(|path| ffmpeg_pair(&path.join("tools")));
    if !ffmpeg_pair(&dir) && !system_ffmpeg && !original_ffmpeg && !previous_ffmpeg {
        if let Err(error) = install_ffmpeg(&dir, sender) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        log(sender, "Download tools are ready.", Level::Success);
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_only_writes_the_requested_basename() {
        use zip::write::SimpleFileOptions;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("archive.zip");
        let file = File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        writer
            .start_file("../../nested/ffmpeg.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"test executable").unwrap();
        writer.finish().unwrap();
        let extracted = extract_named(&path, temp.path(), "ffmpeg.exe").unwrap();
        assert_eq!(extracted, temp.path().join("ffmpeg.exe"));
        assert_eq!(fs::read(extracted).unwrap(), b"test executable");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "manual clean-install network test; downloads tools into a temporary directory"]
    fn live_clean_install_bootstraps_tools() {
        const CHILD: &str = "YTDLP_CLEAN_INSTALL_TEST_DIR";
        if let Some(dir) = std::env::var_os(CHILD) {
            let data = PathBuf::from(dir);
            let (sender, receiver) = std::sync::mpsc::channel();
            let result = resolve(&data, &sender);
            let events: Vec<_> = receiver.try_iter().collect();
            let tools = result.unwrap_or_else(|error| {
                panic!("clean install failed: {error}; events: {events:?}")
            });
            let ffmpeg = tools
                .ffmpeg_dir
                .as_ref()
                .expect("FFmpeg should be installed privately");
            for (path, argument) in [
                (tools.yt_dlp, "--version"),
                (tools.private_dir.join("deno.exe"), "--version"),
                (ffmpeg.join("ffmpeg.exe"), "-version"),
                (ffmpeg.join("ffprobe.exe"), "-version"),
            ] {
                let mut command = std::process::Command::new(&path);
                command.arg(argument);
                crate::download::hide_console(&mut command);
                let result = command
                    .output()
                    .unwrap_or_else(|error| panic!("could not run {path:?}: {error}"));
                assert!(result.status.success(), "{path:?} did not run successfully");
            }
        } else {
            let temp = tempfile::tempdir().unwrap();
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "tools::tests::live_clean_install_bootstraps_tools",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD, temp.path().join("data"))
                .env("USERPROFILE", temp.path().join("profile"))
                .env("LOCALAPPDATA", temp.path().join("local"))
                .env("DENO_DIR", temp.path().join("deno-cache"))
                .env("PATH", "");
            crate::download::hide_console(&mut command);
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "clean install failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}
