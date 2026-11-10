# YouTube to MP3

A Windows downloader for MP3 audio and MP4 video, written in Rust. Choose a video quality up to 4K or the best available quality, download a single video or playlist, and follow progress in the activity log. yt-dlp downloads up to four fragments concurrently.

The app uses yt-dlp, FFmpeg, FFprobe, and Deno as separate tools. It reuses available copies or downloads missing tools into its own local application data directory; Python is not required to run the app.

## Settings and existing installations

The app stores its settings and managed tools under `%LOCALAPPDATA%\YouTube to MP3`. On first run it copies the previous Rust prototype's chosen download folder, or the Python app's chosen folder if no prototype setting exists. It does not change the old settings files. Existing media files are preserved; a duplicate may be downloaded before the existing filename is detected.

The app checks for updates at startup and offers to install a compatible Rust update. The release includes two identical portable executables: GitHub publishes the legacy-compatible asset as `YouTube.to.MP3.exe`, and the Rust updater uses `YouTube.to.MP3.Rust.exe`. The first name is recognized by the installed Python updater. The installer retains the existing app's identity and install location.

## Build and test

On Windows, install Rust and the MSVC build tools, then run:

```powershell
cargo build --locked --release --manifest-path rust-app\Cargo.toml --target-dir rust-app\target
cargo test --locked --manifest-path rust-app\Cargo.toml
```

The executable is `rust-app\target\release\youtube-to-mp3.exe`. Local media tests use yt-dlp, FFmpeg and FFprobe from `PATH`; set `YTDLP_REQUIRE_MEDIA_TOOLS=1` to fail rather than skip if they are unavailable. The release workflow installs these test tools and runs the legacy-upgrade compatibility check before publishing.

To create versioned portable release assets locally, see [release packaging](rust-app/packaging/README.md).
