use crate::events::{log, Event, Level};
use crate::tools::{self, Tools};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc::Sender, Arc, Mutex};
use std::thread;
use url::Url;

const TITLE_MARKER: &str = "__YTMP3_TITLE__:";
const OUTPUT_MARKER: &str = "__YTMP3_OUTPUT__:";
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mp3,
    Mp4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlKind {
    Single,
    VideoInPlaylist,
    Playlist,
}

#[derive(Clone, Debug)]
pub struct DownloadRequest {
    pub url: String,
    pub destination: PathBuf,
    pub format: Format,
    pub height: Option<u32>,
    pub no_playlist: bool,
}

pub fn classify_url(input: &str) -> UrlKind {
    let Ok(url) = Url::parse(input) else {
        return UrlKind::Single;
    };
    let mut has_playlist = url.path().trim_matches('/') == "playlist";
    let mut has_video = false;
    for (key, value) in url.query_pairs() {
        if key == "list" && !value.is_empty() {
            has_playlist = true;
        }
        if key == "v" && !value.is_empty() {
            has_video = true;
        }
    }
    let path = url.path().trim_matches('/');
    if url
        .host_str()
        .is_some_and(|host| host.to_ascii_lowercase().contains("youtu.be"))
        && !path.is_empty()
    {
        has_video = true;
    }
    if ["shorts/", "embed/", "live/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        has_video = true;
    }
    match (has_playlist, has_video) {
        (true, true) => UrlKind::VideoInPlaylist,
        (true, false) => UrlKind::Playlist,
        _ => UrlKind::Single,
    }
}

fn path_argument(prefix: &str, path: &Path) -> OsString {
    let mut result = OsString::from(prefix);
    result.push(path.as_os_str());
    result
}

pub fn arguments(
    request: &DownloadRequest,
    scratch: &Path,
    cache_dir: &Path,
    ffmpeg_dir: Option<&Path>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--ignore-config",
        "--newline",
        "--no-quiet",
        "--no-overwrites",
        "--no-continue",
        "--concurrent-fragments",
        "4",
        "--print",
        "before_dl:__YTMP3_TITLE__:%(title)j",
        "--print",
        "after_move:__YTMP3_OUTPUT__:%(filepath)j",
        "--user-agent",
        USER_AGENT,
        "--referer",
        "https://www.youtube.com/",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    args.push("--cache-dir".into());
    args.push(cache_dir.as_os_str().to_owned());
    args.push("-P".into());
    args.push(path_argument("home:", &scratch.join("complete")));
    args.push("-P".into());
    args.push(path_argument("temp:", &scratch.join("incomplete")));
    args.extend(["-o", "%(title)s.%(ext)s"].into_iter().map(OsString::from));
    match request.format {
        Format::Mp3 => args.extend(
            ["-x", "--audio-format", "mp3"]
                .into_iter()
                .map(OsString::from),
        ),
        Format::Mp4 => {
            let filter = request
                .height
                .map(|height| format!("[height<={height}]"))
                .unwrap_or_default();
            args.extend([
                OsString::from("-f"),
                OsString::from(format!(
                    "bv*{filter}+ba[ext=m4a]/bv*{filter}+ba/b{filter}/bv*{filter}"
                )),
                OsString::from("--merge-output-format"),
                OsString::from("mp4"),
                OsString::from("--remux-video"),
                OsString::from("mp4"),
            ]);
        }
    }
    if let Some(dir) = ffmpeg_dir {
        args.push("--ffmpeg-location".into());
        args.push(dir.as_os_str().to_owned());
    }
    if request.no_playlist {
        args.push("--no-playlist".into());
    }
    args.push(request.url.clone().into());
    args
}

fn is_progress(line: &str) -> bool {
    let Some(rest) = line.strip_prefix("[download]") else {
        return false;
    };
    let rest = rest.trim_start();
    let Some((percent, suffix)) = rest.split_once('%') else {
        return false;
    };
    if !suffix.is_empty() && !suffix.starts_with(char::is_whitespace) {
        return false;
    }
    percent
        .parse::<f64>()
        .is_ok_and(|value| (0.0..=100.0).contains(&value))
}

fn display_line(line: &str) -> Option<(String, Level)> {
    if let Some(raw) = line.strip_prefix(TITLE_MARKER) {
        let title = serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.to_owned());
        let title = title.replace(['\r', '\n'], " ");
        let title = title.trim();
        return (!title.is_empty()).then(|| (format!("Downloading: {title}"), Level::Info));
    }
    if is_progress(line) {
        return Some((
            format!("Downloading {}", line["[download]".len()..].trim()),
            Level::Progress,
        ));
    }
    if line.starts_with("WARNING:") {
        return Some((line.to_owned(), Level::Warning));
    }
    if let Some(detail) = line.strip_prefix("ERROR:") {
        return Some((format!("Download failed: {}", detail.trim()), Level::Error));
    }
    if line.starts_with("[Merger] Merging") {
        return Some(("Combining video and audio...".into(), Level::Info));
    }
    if line.starts_with("[ExtractAudio] Destination:") {
        return Some(("Converting audio to MP3...".into(), Level::Info));
    }
    if line.starts_with("[VideoRemuxer] Remuxing") {
        return Some(("Preparing MP4...".into(), Level::Info));
    }
    if line.starts_with("[download]") && line.contains("has already been downloaded") {
        return Some(("Already downloaded.".into(), Level::Info));
    }
    None
}

#[derive(Default)]
struct OutputState {
    last_line: String,
    reported_error: bool,
    publish_error: bool,
}

fn publish_output(
    raw: &str,
    scratch: &Path,
    destination: &Path,
    sender: &Sender<Event>,
) -> Result<(), String> {
    let path: String = serde_json::from_str(raw)
        .map_err(|error| format!("Could not read the completed download path: {error}"))?;
    let source = fs::canonicalize(&path)
        .map_err(|error| format!("Could not find the completed download: {error}"))?;
    let root = fs::canonicalize(scratch)
        .map_err(|error| format!("Could not find the temporary download folder: {error}"))?;
    if !source.starts_with(root) || !source.is_file() {
        return Err("The downloader reported an invalid output path.".into());
    }
    let filename = source
        .file_name()
        .ok_or("The completed download has no filename.")?;
    let target = destination.join(filename);
    if fs::hard_link(&source, &target).is_ok() {
        return Ok(());
    }

    let mut input = File::open(&source)
        .map_err(|error| format!("Could not read completed download: {error}"))?;
    let mut output = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            log(
                sender,
                format!("Already exists; kept: {}", filename.to_string_lossy()),
                Level::Info,
            );
            return Ok(());
        }
        Err(error) => {
            return Err(format!(
                "Could not save {}: {error}",
                filename.to_string_lossy()
            ))
        }
    };
    if let Err(error) = std::io::copy(&mut input, &mut output).and_then(|_| output.flush()) {
        drop(output);
        let _ = fs::remove_file(&target);
        return Err(format!(
            "Could not save {}: {error}",
            filename.to_string_lossy()
        ));
    }
    Ok(())
}

fn read_output<R: Read>(
    reader: R,
    sender: &Sender<Event>,
    state: &Arc<Mutex<OutputState>>,
    publish_to: Option<(&Path, &Path)>,
) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                log(
                    sender,
                    format!("Could not read downloader output: {error}"),
                    Level::Warning,
                );
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes);
        for part in text.split(['\r', '\n']) {
            let line = part.trim();
            if line.is_empty() {
                continue;
            }
            if let (Some(raw), Some((scratch, destination))) =
                (line.strip_prefix(OUTPUT_MARKER), publish_to)
            {
                if let Err(error) = publish_output(raw, scratch, destination, sender) {
                    log(sender, error, Level::Error);
                    if let Ok(mut output) = state.lock() {
                        output.publish_error = true;
                    }
                }
                continue;
            }
            if let Ok(mut output) = state.lock() {
                output.last_line = line.to_owned();
                output.reported_error |= line.starts_with("ERROR:");
            }
            if let Some((message, level)) = display_line(line) {
                log(sender, message, level);
            }
        }
    }
}

#[cfg(windows)]
pub fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
pub fn hide_console(_command: &mut Command) {}

fn prepare_command(
    request: &DownloadRequest,
    scratch: &Path,
    data_dir: &Path,
    tools: &Tools,
) -> Command {
    let mut command = Command::new(&tools.yt_dlp);
    command.args(arguments(
        request,
        scratch,
        &data_dir.join("yt-dlp-cache"),
        tools.ffmpeg_dir.as_deref(),
    ));
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.env("DENO_DIR", data_dir.join("deno-cache"));
    let mut paths = vec![tools.private_dir.clone()];
    if let Some(previous) = crate::config::previous_data_dir(data_dir) {
        let tools = previous.join("tools");
        if tools.is_dir() {
            paths.push(tools);
        }
    }
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    if let Ok(path) = std::env::join_paths(paths) {
        command.env("PATH", path);
    }
    hide_console(&mut command);
    command
}

pub fn run(
    request: DownloadRequest,
    data_dir: &Path,
    sender: &Sender<Event>,
) -> Result<bool, String> {
    if request.url.trim().is_empty() {
        return Err("YouTube URL required.".into());
    }
    if request.destination.as_os_str().is_empty() {
        return Err("Choose a download folder in Settings.".into());
    }
    fs::create_dir_all(&request.destination)
        .map_err(|error| format!("Could not open download folder: {error}"))?;
    let tools = tools::resolve(data_dir, sender)?;
    run_with_tools(request, data_dir, sender, &tools)
}

fn run_with_tools(
    request: DownloadRequest,
    data_dir: &Path,
    sender: &Sender<Event>,
    tools: &Tools,
) -> Result<bool, String> {
    let scratch = tempfile::Builder::new()
        .prefix(".ytmp3-")
        .tempdir_in(&request.destination)
        .map_err(|error| format!("Could not create temporary folder: {error}"))?;
    log(sender, "Preparing download...", Level::Info);
    let mut child = prepare_command(&request, scratch.path(), data_dir, tools)
        .spawn()
        .map_err(|error| format!("Could not start yt-dlp: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Could not read downloader output.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Could not read downloader errors.")?;
    let state = Arc::new(Mutex::new(OutputState::default()));
    let error_state = Arc::clone(&state);
    let error_sender = sender.clone();
    let error_thread =
        thread::spawn(move || read_output(stderr, &error_sender, &error_state, None));
    read_output(
        stdout,
        sender,
        &state,
        Some((scratch.path(), &request.destination)),
    );
    let status = child
        .wait()
        .map_err(|error| format!("Could not wait for downloader: {error}"))?;
    error_thread
        .join()
        .map_err(|_| "Could not read downloader errors.".to_owned())?;
    let output = state
        .lock()
        .map_err(|_| "Could not read downloader result.".to_owned())?;
    if output.publish_error {
        Ok(false)
    } else if status.success() {
        log(sender, "Download complete.", Level::Success);
        Ok(true)
    } else if output.reported_error {
        Ok(false)
    } else if output.last_line.is_empty() {
        Err("Download failed.".into())
    } else {
        Err(format!("Download failed: {}", output.last_line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[derive(Clone)]
    struct MediaTestTools {
        yt_dlp: PathBuf,
        ffmpeg: PathBuf,
        ffprobe: PathBuf,
    }

    fn find_media_binary(name: &str) -> Option<PathBuf> {
        tools::find_on_path(&format!("{name}.exe")).or_else(|| tools::find_on_path(name))
    }

    fn media_test_tools() -> Option<MediaTestTools> {
        let yt_dlp = find_media_binary("yt-dlp");
        let ffmpeg = find_media_binary("ffmpeg");
        let ffprobe = find_media_binary("ffprobe");
        let mut missing = Vec::new();
        if yt_dlp.is_none() {
            missing.push("yt-dlp");
        }
        if ffmpeg.is_none() {
            missing.push("FFmpeg");
        }
        if ffprobe.is_none() {
            missing.push("FFprobe");
        }
        if !missing.is_empty() {
            let message = format!("missing {} on PATH", missing.join(", "));
            if std::env::var("YTDLP_REQUIRE_MEDIA_TOOLS").as_deref() == Ok("1") {
                panic!("Media parity tests require {message}.");
            }
            eprintln!("Skipping media parity test: {message}.");
            return None;
        }

        Some(MediaTestTools {
            yt_dlp: yt_dlp.unwrap(),
            ffmpeg: ffmpeg.unwrap(),
            ffprobe: ffprobe.unwrap(),
        })
    }

    fn media_test_tools_with_h264() -> Option<MediaTestTools> {
        let binaries = media_test_tools()?;
        let mut command = Command::new(&binaries.ffmpeg);
        command.args(["-hide_banner", "-encoders"]);
        hide_console(&mut command);
        let encoders = command
            .output()
            .ok()
            .filter(|output| output.status.success());
        let has_h264 = encoders
            .as_ref()
            .is_some_and(|output| String::from_utf8_lossy(&output.stdout).contains("libx264"));
        if !has_h264 {
            let message = "FFmpeg does not expose the libx264 encoder";
            if std::env::var("YTDLP_REQUIRE_MEDIA_TOOLS").as_deref() == Ok("1") {
                panic!("Media parity tests require {message}.");
            }
            eprintln!("Skipping media parity test: {message}.");
            return None;
        }
        Some(binaries)
    }

    #[derive(Default)]
    struct FragmentStats {
        active: AtomicUsize,
        peak: AtomicUsize,
    }

    struct LocalMediaServer {
        address: std::net::SocketAddr,
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
        fragments: Arc<FragmentStats>,
    }

    impl LocalMediaServer {
        fn start(root: &Path, fragment_delay: Duration) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let root = root.to_owned();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_signal = Arc::clone(&stop);
            let fragments = Arc::new(FragmentStats::default());
            let stats = Arc::clone(&fragments);
            let worker = thread::spawn(move || {
                while !stop_signal.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let root = root.clone();
                            let stats = Arc::clone(&stats);
                            thread::spawn(move || {
                                serve_media_request(stream, &root, &stats, fragment_delay);
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                address,
                stop,
                worker: Some(worker),
                fragments,
            }
        }

        fn url(&self, path: &str) -> String {
            format!("http://{}/{path}", self.address)
        }
    }

    impl Drop for LocalMediaServer {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn serve_media_request(
        mut stream: TcpStream,
        root: &Path,
        stats: &FragmentStats,
        fragment_delay: Duration,
    ) {
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            let Ok(count) = stream.read(&mut buffer) else {
                return;
            };
            if count == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
            if request.len() > 32 * 1024 {
                return;
            }
        }

        let request = String::from_utf8_lossy(&request);
        let mut fields = request
            .lines()
            .next()
            .unwrap_or_default()
            .split_whitespace();
        let method = fields.next().unwrap_or_default();
        let request_path = fields.next().unwrap_or_default();
        let path = request_path.split('?').next().unwrap_or_default();
        let is_fragment = path.starts_with("/fragment-");
        if is_fragment {
            let active = stats.active.fetch_add(1, Ordering::Relaxed) + 1;
            stats.peak.fetch_max(active, Ordering::Relaxed);
            thread::sleep(fragment_delay);
        }

        let relative = Path::new(path.trim_start_matches('/'));
        let safe_path = relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)));
        let file = if safe_path {
            root.join(relative)
        } else {
            root.join("__invalid_request_path__")
        };
        let body = fs::read(&file).ok();
        let (status, content_type, body) = match body {
            Some(body) if file.extension().is_some_and(|ext| ext == "m3u8") => {
                ("200 OK", "application/vnd.apple.mpegurl", body)
            }
            Some(body) => ("200 OK", "video/mp2t", body),
            None => ("404 Not Found", "text/plain", b"not found".to_vec()),
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        if method != "HEAD" {
            let _ = stream.write_all(&body);
        }
        let _ = stream.flush();
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let _ = stream.read(&mut buffer);
        if is_fragment {
            stats.active.fetch_sub(1, Ordering::Relaxed);
        }
    }

    fn create_video_segment(ffmpeg: &Path, output: &Path, size: &str) {
        let mut command = Command::new(ffmpeg);
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ]);
        command.arg(format!("color=c=blue:s={size}:r=1"));
        command.args([
            "-t",
            "1",
            "-an",
            "-c:v",
            "libx264",
            "-threads",
            "2",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "mpegts",
        ]);
        command.arg(output);
        hide_console(&mut command);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "FFmpeg could not create the {size} fixture: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn create_audio_segment(ffmpeg: &Path, output: &Path) {
        let mut command = Command::new(ffmpeg);
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=44100",
            "-t",
            "1",
            "-vn",
            "-c:a",
            "aac",
            "-f",
            "mpegts",
        ]);
        command.arg(output);
        hide_console(&mut command);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "FFmpeg could not create the audio fixture: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    fn create_hls_fixture(root: &Path, binaries: &MediaTestTools) {
        fs::create_dir_all(root).unwrap();
        create_video_segment(&binaries.ffmpeg, &root.join("low.ts"), "160x90");
        create_video_segment(&binaries.ffmpeg, &root.join("uhd.ts"), "3840x2160");
        create_audio_segment(&binaries.ffmpeg, &root.join("audio.ts"));

        for name in ["low", "uhd", "audio"] {
            fs::write(
                root.join(format!("{name}.m3u8")),
                format!(
                    "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\n{name}.ts\n#EXT-X-ENDLIST\n"
                ),
            )
            .unwrap();
        }
        fs::write(
            root.join("master.m3u8"),
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"audio\",NAME=\"English\",DEFAULT=YES,AUTOSELECT=YES,URI=\"audio.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=160x90,AUDIO=\"audio\",CODECS=\"avc1.42c00b,mp4a.40.2\"\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=3840x2160,AUDIO=\"audio\",CODECS=\"avc1.42c033,mp4a.40.2\"\nuhd.m3u8\n",
        )
        .unwrap();
        fs::write(
            root.join("silent.m3u8"),
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=3840x2160,CODECS=\"avc1.42c033\"\nuhd.m3u8\n",
        )
        .unwrap();

        let mut fragments = String::from(
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n",
        );
        for index in 0..8 {
            let name = format!("fragment-{index}.ts");
            fs::copy(root.join("low.ts"), root.join(&name)).unwrap();
            fragments.push_str(&format!("#EXTINF:1.0,\n{name}\n"));
        }
        fragments.push_str("#EXT-X-ENDLIST\n");
        fs::write(root.join("fragments.m3u8"), fragments).unwrap();
        fs::write(
            root.join("parallel.m3u8"),
            "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=160x90,CODECS=\"avc1.42c00b\"\nfragments.m3u8\n",
        )
        .unwrap();
    }

    fn run_local_download(
        root: &Path,
        binaries: &MediaTestTools,
        url: String,
        name: &str,
        format: Format,
        height: Option<u32>,
    ) -> (Result<bool, String>, PathBuf, Vec<Event>) {
        let destination = root.join(name);
        fs::create_dir_all(&destination).unwrap();
        let data_dir = root.join("prototype-data");
        let tools = Tools {
            yt_dlp: binaries.yt_dlp.clone(),
            ffmpeg_dir: None,
            private_dir: root.join("tools"),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let request = DownloadRequest {
            url,
            destination: destination.clone(),
            format,
            height,
            no_playlist: true,
        };
        let result = run_with_tools(request, &data_dir, &sender, &tools);
        (result, destination, receiver.try_iter().collect())
    }

    fn output_file(destination: &Path, extension: &str) -> PathBuf {
        let files: Vec<_> = fs::read_dir(destination)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|value| value == extension))
            .collect();
        assert_eq!(
            files.len(),
            1,
            "expected one .{extension} output in {destination:?}"
        );
        files.into_iter().next().unwrap()
    }

    fn assert_media_streams(
        ffprobe: &Path,
        path: &Path,
        expected_height: Option<u32>,
        expect_audio: bool,
    ) {
        let mut command = Command::new(ffprobe);
        command
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,height",
                "-of",
                "json",
            ])
            .arg(path);
        hide_console(&mut command);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "FFprobe could not inspect {path:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let streams = value["streams"].as_array().unwrap();
        let video = streams
            .iter()
            .find(|stream| stream["codec_type"] == "video");
        let audio = streams.iter().any(|stream| stream["codec_type"] == "audio");
        assert!(video.is_some(), "expected a video stream in {path:?}");
        if let Some(expected_height) = expected_height {
            assert_eq!(
                video.unwrap()["height"].as_u64(),
                Some(u64::from(expected_height))
            );
        }
        assert_eq!(audio, expect_audio, "unexpected audio streams in {path:?}");
    }

    fn assert_media_download(
        root: &Path,
        binaries: &MediaTestTools,
        url: String,
        name: &str,
        requested_height: Option<u32>,
        expected_height: Option<u32>,
        expect_audio: bool,
    ) {
        let (result, destination, events) =
            run_local_download(root, binaries, url, name, Format::Mp4, requested_height);
        assert!(
            matches!(result, Ok(true)),
            "download failed: {result:?}; events: {events:?}"
        );
        let output = output_file(&destination, "mp4");
        assert_media_streams(&binaries.ffprobe, &output, expected_height, expect_audio);
    }

    fn request(format: Format) -> DownloadRequest {
        DownloadRequest {
            url: "https://youtube.com/watch?v=abc&list=xyz".into(),
            destination: PathBuf::from("output folder"),
            format,
            height: Some(2160),
            no_playlist: true,
        }
    }

    #[test]
    fn recognizes_playlist_and_video_links() {
        assert_eq!(
            classify_url("https://www.youtube.com/watch?v=abc"),
            UrlKind::Single
        );
        assert_eq!(
            classify_url("https://youtu.be/abc?list=xyz"),
            UrlKind::VideoInPlaylist
        );
        assert_eq!(
            classify_url("https://youtube.com/shorts/abc?list=xyz"),
            UrlKind::VideoInPlaylist
        );
        assert_eq!(
            classify_url("https://youtube.com/playlist?list=xyz"),
            UrlKind::Playlist
        );
    }

    #[test]
    fn audio_and_video_options_keep_existing_files_safe() {
        for format in [Format::Mp3, Format::Mp4] {
            let args = arguments(
                &request(format),
                Path::new("private scratch"),
                Path::new("private cache"),
                None,
            );
            let args: Vec<_> = args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect();
            assert!(args.contains(&"--no-overwrites".into()));
            assert!(args.contains(&"--no-continue".into()));
            assert!(args.contains(&"--ignore-config".into()));
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--cache-dir", "private cache"]));
            assert!(args.windows(2).any(|pair| pair[0] == "-P"
                && pair[1]
                    == path_argument("home:", &Path::new("private scratch").join("complete"))
                        .to_string_lossy()));
            assert!(args
                .windows(2)
                .any(|pair| pair == ["--concurrent-fragments", "4"]));
            assert!(args.windows(2).any(|pair| pair[0] == "-P"
                && pair[1]
                    == path_argument("temp:", &Path::new("private scratch").join("incomplete"))
                        .to_string_lossy()));
            assert!(args.contains(&"--no-playlist".into()));
            if format == Format::Mp3 {
                assert!(args
                    .windows(2)
                    .any(|pair| pair == ["--audio-format", "mp3"]));
            } else {
                assert!(args.iter().any(|arg| arg.contains("height<=2160")));
                assert!(args
                    .windows(2)
                    .any(|pair| pair == ["--merge-output-format", "mp4"]));
            }
        }
    }

    #[test]
    fn concise_output_preserves_progress_and_errors() {
        assert_eq!(display_line("[youtube] Downloading webpage"), None);
        assert_eq!(
            display_line("__YTMP3_TITLE__:\"Example\\nTitle\""),
            Some(("Downloading: Example Title".into(), Level::Info))
        );
        assert_eq!(
            display_line("[download] 12.4% of 10MiB").unwrap().1,
            Level::Progress
        );
        assert_eq!(display_line("[download] 706% of 10MiB"), None);
        assert_eq!(
            display_line("ERROR: unavailable"),
            Some(("Download failed: unavailable".into(), Level::Error))
        );
    }

    #[test]
    fn completed_output_must_be_inside_the_temporary_folder() {
        let root = tempfile::tempdir().unwrap();
        let scratch = root.path().join("scratch");
        let destination = root.path().join("destination");
        fs::create_dir(&scratch).unwrap();
        fs::create_dir(&destination).unwrap();
        let outside = root.path().join("existing.mp4");
        fs::write(&outside, b"untouched").unwrap();
        let (sender, _) = std::sync::mpsc::channel();
        let raw = serde_json::to_string(&outside.to_string_lossy()).unwrap();
        assert!(publish_output(&raw, &scratch, &destination, &sender).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"untouched");
        assert!(!destination.join("existing.mp4").exists());
    }

    #[test]
    fn local_hls_selection_matches_python_media_behavior() {
        let Some(binaries) = media_test_tools_with_h264() else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        create_hls_fixture(&source, &binaries);
        let server = LocalMediaServer::start(&source, Duration::ZERO);
        let root = temp.path().join("downloads");

        assert_media_download(
            &root,
            &binaries,
            server.url("master.m3u8"),
            "four-k",
            Some(2160),
            Some(2160),
            true,
        );
        assert_media_download(
            &root,
            &binaries,
            server.url("master.m3u8"),
            "720p-cap-fallback",
            Some(720),
            Some(90),
            true,
        );
        assert_media_download(
            &root,
            &binaries,
            server.url("master.m3u8"),
            "best-available",
            None,
            Some(2160),
            true,
        );
        assert_media_download(
            &root,
            &binaries,
            server.url("silent.m3u8"),
            "silent-video",
            Some(2160),
            Some(2160),
            false,
        );

        let (result, destination, events) = run_local_download(
            &root,
            &binaries,
            server.url("master.m3u8"),
            "missing-resolution",
            Format::Mp4,
            Some(72),
        );
        assert!(
            !matches!(result, Ok(true)),
            "a missing resolution unexpectedly downloaded successfully; events: {events:?}"
        );
        assert!(
            fs::read_dir(destination).unwrap().next().is_none(),
            "a missing resolution left output files behind"
        );
    }

    #[test]
    fn local_hls_downloads_fragments_concurrently_with_a_four_fragment_limit() {
        let Some(binaries) = media_test_tools_with_h264() else {
            return;
        };
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        create_hls_fixture(&source, &binaries);
        let server = LocalMediaServer::start(&source, Duration::from_millis(200));
        let (result, destination, events) = run_local_download(
            temp.path(),
            &binaries,
            server.url("parallel.m3u8"),
            "parallel-download",
            Format::Mp4,
            Some(720),
        );
        assert!(
            matches!(result, Ok(true)),
            "download failed: {result:?}; events: {events:?}"
        );
        assert_media_streams(
            &binaries.ffprobe,
            &output_file(&destination, "mp4"),
            Some(90),
            false,
        );
        let peak = server.fragments.peak.load(Ordering::Relaxed);
        assert!(peak > 1, "HLS fragment requests were not concurrent");
        assert!(
            peak <= 4,
            "yt-dlp requested {peak} HLS fragments concurrently"
        );
    }

    #[test]
    #[ignore = "manual network smoke test; set YTDLP_LIVE_TEST_URL before running with --ignored"]
    fn live_youtube_video_downloads_mp4_at_or_below_720p() {
        let url = std::env::var("YTDLP_LIVE_TEST_URL")
            .expect("set YTDLP_LIVE_TEST_URL to a public YouTube video URL");
        let binaries = media_test_tools().expect("yt-dlp, FFmpeg, and FFprobe must be on PATH");
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("downloads");
        fs::create_dir_all(&destination).unwrap();
        let tools = Tools {
            yt_dlp: binaries.yt_dlp.clone(),
            ffmpeg_dir: None,
            private_dir: temp.path().join("tools"),
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        let request = DownloadRequest {
            url,
            destination: destination.clone(),
            format: Format::Mp4,
            height: Some(720),
            no_playlist: true,
        };
        let result = run_with_tools(
            request,
            &temp.path().join("prototype-data"),
            &sender,
            &tools,
        );
        let events: Vec<_> = receiver.try_iter().collect();
        assert!(
            matches!(result, Ok(true)),
            "download failed: {result:?}; events: {events:?}"
        );
        let output = output_file(&destination, "mp4");
        let mut command = Command::new(&binaries.ffprobe);
        command
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,height",
                "-of",
                "json",
            ])
            .arg(&output);
        hide_console(&mut command);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "FFprobe could not inspect {output:?}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let streams = value["streams"].as_array().unwrap();
        let video = streams
            .iter()
            .find(|stream| stream["codec_type"] == "video")
            .expect("live download has no video stream");
        assert!(
            video["height"].as_u64().is_some_and(|height| height <= 720),
            "live output exceeds 720p: {video:?}"
        );
        assert!(
            streams.iter().any(|stream| stream["codec_type"] == "audio"),
            "live output has no audio stream"
        );
    }

    #[test]
    #[ignore = "manual network smoke test; set YTDLP_LIVE_TEST_URL before running with --ignored"]
    fn live_youtube_audio_downloads_as_mp3() {
        let url = std::env::var("YTDLP_LIVE_TEST_URL")
            .expect("set YTDLP_LIVE_TEST_URL to a public YouTube video URL");
        let binaries = media_test_tools().expect("yt-dlp, FFmpeg, and FFprobe must be on PATH");
        let temp = tempfile::tempdir().unwrap();
        let (result, destination, events) =
            run_local_download(temp.path(), &binaries, url, "audio", Format::Mp3, None);
        assert!(
            matches!(result, Ok(true)),
            "download failed: {result:?}; events: {events:?}"
        );
        let output = output_file(&destination, "mp3");
        let mut probe = Command::new(&binaries.ffprobe);
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "json",
            ])
            .arg(&output);
        hide_console(&mut probe);
        let result = probe.output().unwrap();
        assert!(result.status.success(), "FFprobe could not inspect the MP3");
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let streams = value["streams"].as_array().unwrap();
        assert!(streams.iter().any(|stream| stream["codec_type"] == "audio"));
        assert!(!streams.iter().any(|stream| stream["codec_type"] == "video"));
    }

    #[cfg(windows)]
    #[test]
    fn local_media_download_preserves_existing_output() {
        let Some(binaries) = media_test_tools_with_h264() else {
            return;
        };
        let ytdlp = binaries.yt_dlp;
        let ffmpeg = binaries.ffmpeg;
        let ffprobe = binaries.ffprobe;

        let temp = tempfile::tempdir().unwrap();
        let segment_path = temp.path().join("segment.ts");
        let mut create = Command::new(ffmpeg);
        create
            .args([
                "-v",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=160x90:r=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=44100",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-f",
                "mpegts",
            ])
            .arg(&segment_path);
        hide_console(&mut create);
        assert!(create.status().unwrap().success());
        let segment = Arc::new(fs::read(&segment_path).unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_signal = Arc::clone(&stopped);
        let server = thread::spawn(move || {
            while !stop_signal.load(Ordering::Relaxed) {
                let (mut stream, _) = match listener.accept() {
                    Ok(connection) => connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }
                    Err(_) => break,
                };
                let data = Arc::clone(&segment);
                thread::spawn(move || {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                    let mut buffer = [0_u8; 4096];
                    let mut request_bytes = Vec::new();
                    loop {
                        let Ok(count) = stream.read(&mut buffer) else {
                            return;
                        };
                        if count == 0 {
                            return;
                        }
                        request_bytes.extend_from_slice(&buffer[..count]);
                        if request_bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                            break;
                        }
                        if request_bytes.len() > 32 * 1024 {
                            return;
                        }
                    }
                    let request = String::from_utf8_lossy(&request_bytes);
                    let mut parts = request
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .split_whitespace();
                    let method = parts.next().unwrap_or_default();
                    let path = parts.next().unwrap_or_default();
                    let (status, content_type, body): (&str, &str, &[u8]) = match path {
                        "/master.m3u8" => ("200 OK", "application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=160x90,CODECS=\"avc1.42c00b,mp4a.40.2\"\nvideo.m3u8\n"),
                        "/video.m3u8" => ("200 OK", "application/vnd.apple.mpegurl", b"#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\nsegment.ts\n#EXT-X-ENDLIST\n"),
                        "/segment.ts" => ("200 OK", "video/mp2t", data.as_slice()),
                        _ => ("404 Not Found", "text/plain", b"not found"),
                    };
                    let header = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = stream.write_all(header.as_bytes());
                    if method != "HEAD" {
                        let _ = stream.write_all(body);
                    }
                    let _ = stream.flush();
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    let _ = stream.read(&mut buffer);
                });
            }
        });

        let tools = Tools {
            yt_dlp: ytdlp,
            ffmpeg_dir: None,
            private_dir: temp.path().join("tools"),
        };
        let data = temp.path().join("prototype-data");
        let existing = temp.path().join("existing");
        fs::create_dir_all(&existing).unwrap();
        let protected = existing.join("master.mp4");
        fs::write(&protected, b"do not overwrite").unwrap();
        let protected_partial = existing.join("master.mp4.part");
        fs::write(&protected_partial, b"do not modify partial").unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut job = DownloadRequest {
            url: format!("http://{address}/master.m3u8"),
            destination: existing,
            format: Format::Mp4,
            height: Some(720),
            no_playlist: true,
        };
        let result = run_with_tools(job.clone(), &data, &sender, &tools);
        assert!(
            result.unwrap(),
            "events: {:?}",
            receiver.try_iter().collect::<Vec<_>>()
        );
        assert_eq!(fs::read(&protected).unwrap(), b"do not overwrite");
        assert_eq!(
            fs::read(&protected_partial).unwrap(),
            b"do not modify partial"
        );

        let protected_audio = job.destination.join("master.mp3");
        job.format = Format::Mp3;
        job.height = None;
        let result = run_with_tools(job.clone(), &data, &sender, &tools);
        assert!(
            matches!(result, Ok(true)),
            "result: {result:?}; events: {:?}",
            receiver.try_iter().collect::<Vec<_>>()
        );
        assert!(protected_audio.is_file());
        assert_eq!(fs::read(&protected).unwrap(), b"do not overwrite");
        fs::write(&protected_audio, b"do not overwrite audio").unwrap();
        let result = run_with_tools(job.clone(), &data, &sender, &tools);
        assert!(
            matches!(result, Ok(true)),
            "result: {result:?}; events: {:?}",
            receiver.try_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            fs::read(&protected_audio).unwrap(),
            b"do not overwrite audio"
        );

        job.destination = temp.path().join("fresh");
        job.format = Format::Mp4;
        job.height = Some(720);
        fs::create_dir_all(&job.destination).unwrap();
        let result = run_with_tools(job.clone(), &data, &sender, &tools);
        assert!(
            matches!(result, Ok(true)),
            "result: {result:?}; events: {:?}",
            receiver.try_iter().collect::<Vec<_>>()
        );
        let video = job.destination.join("master.mp4");
        assert!(video.is_file());
        let mut probe = Command::new(&ffprobe);
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "json",
            ])
            .arg(&video);
        hide_console(&mut probe);
        let output = probe.output().unwrap();
        assert!(output.status.success());
        let streams: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let streams = streams["streams"].as_array().unwrap();
        assert!(streams.iter().any(|stream| stream["codec_type"] == "video"));
        assert!(streams.iter().any(|stream| stream["codec_type"] == "audio"));

        job.destination = temp.path().join("audio");
        job.format = Format::Mp3;
        job.height = None;
        fs::create_dir_all(&job.destination).unwrap();
        let result = run_with_tools(job.clone(), &data, &sender, &tools);
        assert!(
            matches!(result, Ok(true)),
            "result: {result:?}; events: {:?}",
            receiver.try_iter().collect::<Vec<_>>()
        );
        let audio = job.destination.join("master.mp3");
        assert!(audio.is_file());
        let mut probe = Command::new(&ffprobe);
        probe
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type",
                "-of",
                "json",
            ])
            .arg(&audio);
        hide_console(&mut probe);
        let output = probe.output().unwrap();
        assert!(output.status.success());
        let streams: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let streams = streams["streams"].as_array().unwrap();
        assert!(streams.iter().any(|stream| stream["codec_type"] == "audio"));
        assert!(!streams.iter().any(|stream| stream["codec_type"] == "video"));
        stopped.store(true, Ordering::Relaxed);
        server.join().unwrap();
    }
}
