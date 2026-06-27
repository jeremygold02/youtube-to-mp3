import os
import re
import subprocess

import requests

from youtube_to_mp3.runtime import hidden_subprocess_kwargs


YTDLP_URL = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
BROWSER_USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) "
    "Gecko/20100101 Firefox/128.0"
)
YOUTUBE_REFERER = "https://www.youtube.com/"
GITHUB_REFERER = "https://github.com/yt-dlp/yt-dlp"


def extract_percent(text):
    match = re.search(r"(\d{1,3}(?:\.\d+)?)%", text)
    if not match:
        return None
    try:
        return float(match.group(1))
    except ValueError:
        return None


class DownloadManager:
    def __init__(self, log_callback):
        self.log = log_callback

    def download_yt_dlp(self, path):
        self.log("Downloading yt-dlp...", "success")
        response = requests.get(
            YTDLP_URL,
            headers={
                "User-Agent": BROWSER_USER_AGENT,
                "Referer": GITHUB_REFERER,
            },
            stream=True,
            timeout=120,
        )
        response.raise_for_status()

        total_bytes = int(response.headers.get("content-length") or 0)
        downloaded_bytes = 0
        next_progress = 10

        with open(path, "wb") as file:
            for chunk in response.iter_content(chunk_size=8192):
                if not chunk:
                    continue
                file.write(chunk)
                downloaded_bytes += len(chunk)
                if total_bytes:
                    percent = int(downloaded_bytes * 100 / total_bytes)
                    if percent >= next_progress:
                        self.log(f"yt-dlp download... {percent}%", "process")
                        next_progress += 10

        self.log("yt-dlp downloaded successfully.", "success")

    def ensure_ytdlp_and_download(self, url, target_path, no_playlist):
        self.log("Checking yt-dlp...", "warning")
        os.makedirs(target_path, exist_ok=True)

        ytdlp_path = os.path.join(target_path, "yt-dlp.exe")
        if not os.path.exists(ytdlp_path):
            self.download_yt_dlp(ytdlp_path)

        self.download_audio(url, target_path, no_playlist)

    def force_update_yt(self, target_path):
        os.makedirs(target_path, exist_ok=True)
        self.download_yt_dlp(os.path.join(target_path, "yt-dlp.exe"))
        self.log("yt-dlp updated successfully.", "success")

    def force_update_deno(self):
        try:
            self.log("Downloading Deno...", "success")
            process = subprocess.run(
                [
                    "powershell.exe",
                    "-NoProfile",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-Command",
                    "irm https://deno.land/install.ps1 | iex",
                ],
                capture_output=True,
                text=True,
                **hidden_subprocess_kwargs(),
            )
        except FileNotFoundError:
            self.log("PowerShell not found on system.", "error")
            return False
        except Exception as error:
            self.log(f"Error updating Deno: {error}", "error")
            return False

        output = (process.stdout or "").strip()
        error_output = (process.stderr or "").strip()

        if process.returncode == 0:
            if output:
                self.log(output, "process")
            self.log("Deno updated successfully.", "success")
            return True

        message = error_output or output or "Unknown error"
        self.log(f"Deno update failed: {message}", "error")
        return False

    def force_update_tools(self, target_path):
        self.log("Updating yt-dlp and Deno...", "info")

        ytdlp_updated = True
        try:
            self.force_update_yt(target_path)
        except Exception as error:
            ytdlp_updated = False
            self.log(f"yt-dlp update failed: {error}", "error")

        deno_updated = self.force_update_deno()
        if ytdlp_updated and deno_updated:
            self.log("yt-dlp and Deno updated successfully.", "success")
        elif ytdlp_updated or deno_updated:
            self.log("Update finished with one failure.", "warning")
        else:
            self.log("yt-dlp and Deno updates failed.", "error")

    def download_audio(self, url, target_path, no_playlist):
        ytdlp_path = os.path.join(target_path, "yt-dlp.exe")
        cmd = [
            ytdlp_path,
            "--newline",
            "-x",
            "--audio-format",
            "mp3",
            "--user-agent",
            BROWSER_USER_AGENT,
            "--referer",
            YOUTUBE_REFERER,
            "-o",
            os.path.join(target_path, "%(title)s.%(ext)s"),
        ]
        if no_playlist:
            cmd.append("--no-playlist")
        cmd.append(url)

        self.log("Downloading audio...", "warning")
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            encoding="utf-8",
            errors="replace",
            **hidden_subprocess_kwargs(),
        )

        last_output = ""
        if process.stdout is not None:
            for line in process.stdout:
                for part in re.split(r"[\r\n]+", line):
                    clean_line = part.strip()
                    if not clean_line:
                        continue
                    last_output = clean_line
                    self.log(clean_line, "process")
                    percent = extract_percent(clean_line)
                    if percent is not None:
                        self.log(f"Downloading... {percent:.0f}%", "warning")

        process.wait()
        if process.returncode == 0:
            self.log("Download complete.", "success")
        elif last_output:
            self.log(f"Download failed: {last_output}", "error")
        else:
            self.log("Download failed.", "error")
