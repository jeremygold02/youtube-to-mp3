import html
import json
import os
import re
import subprocess
import sys
import threading
import webbrowser
from datetime import datetime
from urllib.parse import parse_qs, urlparse

import requests

if __package__ in {None, ""}:
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from youtube_to_mp3.updates import check_for_update, install_update
from youtube_to_mp3.version import APP_NAME, APP_VERSION

try:
    from PyQt6.QtCore import QObject, QTimer, pyqtSignal
    from PyQt6.QtGui import QIcon, QTextCursor
    from PyQt6.QtWidgets import (
        QApplication,
        QHBoxLayout,
        QLineEdit,
        QMainWindow,
        QMessageBox,
        QPushButton,
        QTextEdit,
        QVBoxLayout,
        QWidget,
    )

    PYQT_VERSION = 6
except ImportError:
    from PyQt5.QtCore import QObject, QTimer, pyqtSignal
    from PyQt5.QtGui import QIcon, QTextCursor
    from PyQt5.QtWidgets import (
        QApplication,
        QHBoxLayout,
        QLineEdit,
        QMainWindow,
        QMessageBox,
        QPushButton,
        QTextEdit,
        QVBoxLayout,
        QWidget,
    )

    PYQT_VERSION = 5


CONFIG_PATH = os.path.join(os.path.expanduser("~"), "Documents", "ytmp3-config.json")
YTDLP_URL = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe"
APP_USER_MODEL_ID = "YoutubeToMP3.PyQt"
PROJECT_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_PATH = os.path.join(
    getattr(sys, "_MEIPASS", PROJECT_ROOT),
    "icon.ico",
)


if PYQT_VERSION == 6:
    TEXT_CURSOR_END = QTextCursor.MoveOperation.End
    WARNING_ICON = QMessageBox.Icon.Warning
    ACCEPT_ROLE = QMessageBox.ButtonRole.AcceptRole
    ACTION_ROLE = QMessageBox.ButtonRole.ActionRole
    REJECT_ROLE = QMessageBox.ButtonRole.RejectRole
else:
    TEXT_CURSOR_END = QTextCursor.End
    WARNING_ICON = QMessageBox.Warning
    ACCEPT_ROLE = QMessageBox.AcceptRole
    ACTION_ROLE = QMessageBox.ActionRole
    REJECT_ROLE = QMessageBox.RejectRole


LOG_COLORS = {
    "info": "#d1d5db",
    "success": "#22c55e",
    "warning": "#facc15",
    "error": "#f87171",
    "process": "#93c5fd",
}


def load_or_create_config():
    os.makedirs(os.path.dirname(CONFIG_PATH), exist_ok=True)
    if os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as file:
                return json.load(file)
        except json.JSONDecodeError:
            pass
    return {}


def save_config(config):
    with open(CONFIG_PATH, "w", encoding="utf-8") as file:
        json.dump(config, file, indent=4)


def extract_percent(text):
    match = re.search(r"(\d{1,3}(?:\.\d+)?)%", text)
    if not match:
        return None
    try:
        return float(match.group(1))
    except ValueError:
        return None


def exec_dialog(dialog):
    if hasattr(dialog, "exec"):
        return dialog.exec()
    return dialog.exec_()


def create_app_icon():
    if os.path.exists(ICON_PATH):
        return QIcon(ICON_PATH)
    return QIcon()


def configure_windows_app_id():
    if os.name != "nt":
        return

    try:
        import ctypes

        ctypes.windll.shell32.SetCurrentProcessExplicitAppUserModelID(APP_USER_MODEL_ID)
    except Exception:
        pass


def hidden_subprocess_kwargs():
    if os.name != "nt":
        return {}

    startupinfo = subprocess.STARTUPINFO()
    startupinfo.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startupinfo.wShowWindow = subprocess.SW_HIDE
    return {
        "creationflags": subprocess.CREATE_NO_WINDOW,
        "startupinfo": startupinfo,
    }


def classify_youtube_url(url):
    parsed = urlparse(url)
    query = parse_qs(parsed.query)
    host = parsed.netloc.lower()
    path = parsed.path.strip("/")

    has_playlist = bool(query.get("list")) or path == "playlist"
    has_video = bool(query.get("v"))

    if "youtu.be" in host and path:
        has_video = True
    if path.startswith(("shorts/", "embed/", "live/")):
        has_video = True

    if has_playlist and has_video:
        return "video_in_playlist"
    if has_playlist:
        return "playlist"
    return "single"


class WorkerSignals(QObject):
    log = pyqtSignal(str, str)
    busy_changed = pyqtSignal(bool)
    update_check_complete = pyqtSignal(dict)
    update_check_failed = pyqtSignal(str)


class YoutubeToMP3Window(QMainWindow):
    def __init__(self):
        super().__init__()
        self.config = load_or_create_config()
        self.download_path = self.config.get("download_path", "")
        self.worker_thread = None
        self.busy = False

        self.signals = WorkerSignals()
        self.signals.log.connect(self.append_log)
        self.signals.busy_changed.connect(self.set_busy)
        self.signals.update_check_complete.connect(self.handle_update_check_complete)
        self.signals.update_check_failed.connect(self.handle_update_check_failed)

        self.setWindowTitle(APP_NAME)
        self.setWindowIcon(create_app_icon())
        self.resize(620, 460)
        self.setMinimumSize(520, 380)
        self.build_ui()
        self.apply_styles()
        self.append_log("Ready.", "info")
        QTimer.singleShot(700, self.check_for_updates_on_launch)

    def build_ui(self):
        root = QWidget()
        layout = QVBoxLayout(root)
        layout.setContentsMargins(22, 22, 22, 22)
        layout.setSpacing(12)

        self.url_input = QLineEdit()
        self.url_input.setPlaceholderText("https://www.youtube.com/watch?v=...")
        self.url_input.returnPressed.connect(self.start_download)

        self.download_button = QPushButton("Download MP3")
        self.download_button.clicked.connect(self.start_download)

        url_row = QHBoxLayout()
        url_row.addWidget(self.url_input, 1)
        url_row.addWidget(self.download_button)
        layout.addLayout(url_row)

        self.path_input = QLineEdit()
        self.path_input.setPlaceholderText(r"C:\Music\Downloads")
        self.path_input.setText(self.download_path)

        self.save_path_button = QPushButton("Set Download Folder")
        self.save_path_button.clicked.connect(self.save_download_path_from_input)

        self.open_folder_button = QPushButton("Open Folder")
        self.open_folder_button.clicked.connect(self.open_download_folder)

        path_row = QHBoxLayout()
        path_row.addWidget(self.path_input, 1)
        path_row.addWidget(self.open_folder_button)
        layout.addLayout(path_row)

        actions_row = QHBoxLayout()
        actions_row.addWidget(self.save_path_button)

        self.update_tools_button = QPushButton("Update yt-dlp && Deno")
        self.update_tools_button.clicked.connect(self.start_tools_update)
        actions_row.addWidget(self.update_tools_button)

        self.clear_log_button = QPushButton("Clear Log")
        self.clear_log_button.clicked.connect(self.clear_log)
        actions_row.addWidget(self.clear_log_button)
        layout.addLayout(actions_row)

        self.log_view = QTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.setObjectName("logView")
        layout.addWidget(self.log_view, 1)

        self.setCentralWidget(root)

    def apply_styles(self):
        self.setStyleSheet(
            """
            QMainWindow, QWidget {
                background: #0b1220;
                color: #f9fafb;
                font-family: "Segoe UI", Arial, sans-serif;
                font-size: 14px;
            }
            QLineEdit {
                background: #0f172a;
                border: 1px solid #334155;
                border-radius: 6px;
                color: #f9fafb;
                padding: 9px 10px;
            }
            QLineEdit:focus {
                border-color: #22d3ee;
            }
            QPushButton {
                background: #374151;
                border: 1px solid #4b5563;
                border-radius: 6px;
                color: #f9fafb;
                font-weight: 600;
                padding: 9px 12px;
            }
            QPushButton:hover {
                background: #475569;
            }
            QPushButton:pressed {
                background: #1f2937;
            }
            QPushButton:disabled {
                color: #6b7280;
                background: #1f2937;
                border-color: #374151;
            }
            QTextEdit#logView {
                background: #050a14;
                border: 1px solid #334155;
                border-radius: 6px;
                color: #d1d5db;
                padding: 8px;
                selection-background-color: #155e75;
            }
            """
        )

    def append_log(self, message, level="info"):
        color = LOG_COLORS.get(level, LOG_COLORS["info"])
        timestamp = datetime.now().strftime("%H:%M:%S")
        safe_message = html.escape(message).replace("\n", "<br>")
        self.log_view.append(
            f'<span style="color: #64748b;">[{timestamp}]</span> '
            f'<span style="color: {color};">{safe_message}</span>'
        )
        self.log_view.moveCursor(TEXT_CURSOR_END)

    def clear_log(self):
        self.log_view.clear()
        self.append_log("Log cleared.", "info")

    def set_busy(self, busy):
        self.busy = busy
        for button in (
            self.download_button,
            self.save_path_button,
            self.open_folder_button,
            self.update_tools_button,
        ):
            button.setDisabled(busy)

    def log_from_worker(self, message, level="info"):
        self.signals.log.emit(message, level)

    def check_for_updates_on_launch(self):
        self.append_log(f"Checking for updates (v{APP_VERSION})...", "info")
        threading.Thread(target=self.run_update_check, daemon=True).start()

    def run_update_check(self):
        try:
            update = check_for_update()
        except Exception as error:
            self.signals.update_check_failed.emit(str(error))
            return

        self.signals.update_check_complete.emit(update)

    def handle_update_check_failed(self, message):
        self.append_log(f"Update check failed: {message}", "warning")

    def handle_update_check_complete(self, update):
        message = update.get("message") or "Update check complete."
        if not update.get("update_available"):
            self.append_log(message, "info")
            return

        self.append_log(message, "warning")
        latest_version = update.get("latest_version") or "the latest version"
        release_url = update.get("release_url") or update.get("repo_url") or ""

        message_box = QMessageBox(self)
        message_box.setWindowTitle("Update Available")
        message_box.setIcon(WARNING_ICON)
        message_box.setText(f"{APP_NAME} {latest_version} is available.")

        if update.get("can_install"):
            message_box.setInformativeText(
                "Install it now? The app will close and restart after the update is installed."
            )
            install_button = message_box.addButton("Install Update", ACCEPT_ROLE)
            release_button = message_box.addButton("Open Release Page", ACTION_ROLE)
            later_button = message_box.addButton("Later", REJECT_ROLE)
            message_box.setDefaultButton(install_button)
            message_box.setEscapeButton(later_button)
            exec_dialog(message_box)

            clicked_button = message_box.clickedButton()
            if clicked_button == install_button:
                self.start_app_update_install()
            elif clicked_button == release_button and release_url:
                webbrowser.open(release_url)
            return

        message_box.setInformativeText(
            "Automatic install is only available in the packaged Windows app. "
            "Open the release page to download it manually."
        )
        release_button = message_box.addButton("Open Release Page", ACCEPT_ROLE)
        later_button = message_box.addButton("Later", REJECT_ROLE)
        message_box.setDefaultButton(release_button)
        message_box.setEscapeButton(later_button)
        exec_dialog(message_box)

        if message_box.clickedButton() == release_button and release_url:
            webbrowser.open(release_url)

    def start_app_update_install(self):
        if self.busy:
            self.append_log("Finish the current task before installing an app update.", "warning")
            return

        self.start_background_task(self.install_app_update)

    def install_app_update(self):
        self.log_from_worker("Downloading app update...", "warning")
        result = install_update()
        self.log_from_worker(result.get("message", "Update downloaded."), "success")

    def save_download_path_from_input(self):
        path = self.path_input.text().strip()
        if not path:
            self.append_log("Download path cannot be empty.", "error")
            return None

        self.download_path = path
        self.config["download_path"] = path
        save_config(self.config)
        self.append_log(f"Download path updated: {path}", "success")
        return path

    def current_download_path(self):
        path = self.path_input.text().strip()
        if not path:
            self.append_log("Error: No download path set.", "error")
            return None

        if path != self.download_path:
            self.download_path = path
            self.config["download_path"] = path
            save_config(self.config)
            self.append_log(f"Download path updated: {path}", "success")
        return path

    def open_download_folder(self):
        path = self.current_download_path()
        if not path:
            return

        try:
            os.makedirs(path, exist_ok=True)
            if os.name == "nt":
                os.startfile(path)  # type: ignore[attr-defined]
            elif sys.platform == "darwin":
                subprocess.Popen(["open", path], **hidden_subprocess_kwargs())
            else:
                subprocess.Popen(["xdg-open", path], **hidden_subprocess_kwargs())
            self.append_log(f"Opened download folder: {path}", "info")
        except Exception as error:
            self.append_log(f"Failed to open folder: {error}", "error")

    def confirm_playlist_choice(self, url):
        url_kind = classify_youtube_url(url)
        if url_kind == "single":
            return False

        if url_kind == "video_in_playlist":
            message_box = QMessageBox(self)
            message_box.setWindowTitle("Playlist URL Detected")
            message_box.setIcon(WARNING_ICON)
            message_box.setText("This video URL is part of a playlist.")
            message_box.setInformativeText(
                "Choose whether to download only this video or the entire playlist."
            )
            video_button = message_box.addButton("Download This Video", ACCEPT_ROLE)
            playlist_button = message_box.addButton("Download Entire Playlist", ACTION_ROLE)
            cancel_button = message_box.addButton("Cancel", REJECT_ROLE)
            message_box.setDefaultButton(video_button)
            message_box.setEscapeButton(cancel_button)
            exec_dialog(message_box)

            clicked_button = message_box.clickedButton()
            if clicked_button == video_button:
                self.append_log("Playlist URL detected. Downloading this video only.", "warning")
                return True
            if clicked_button == playlist_button:
                self.append_log("Playlist URL detected. Downloading the entire playlist.", "warning")
                return False

            self.append_log("Download canceled.", "info")
            return None

        message_box = QMessageBox(self)
        message_box.setWindowTitle("Playlist URL Detected")
        message_box.setIcon(WARNING_ICON)
        message_box.setText("This URL is a playlist.")
        message_box.setInformativeText("Do you want to download the entire playlist?")
        download_button = message_box.addButton("Download Playlist", ACCEPT_ROLE)
        cancel_button = message_box.addButton("Cancel", REJECT_ROLE)
        message_box.setDefaultButton(download_button)
        message_box.setEscapeButton(cancel_button)
        exec_dialog(message_box)

        if message_box.clickedButton() == download_button:
            self.append_log("Playlist URL detected. Downloading the entire playlist.", "warning")
            return False

        self.append_log("Download canceled.", "info")
        return None

    def start_download(self):
        if self.busy:
            self.append_log("Another task is already running.", "warning")
            return

        url = self.url_input.text().strip()
        if not url:
            self.append_log("Error: YouTube URL required.", "error")
            return

        path = self.current_download_path()
        if not path:
            return

        no_playlist = self.confirm_playlist_choice(url)
        if no_playlist is None:
            return

        self.start_background_task(
            self.ensure_ytdlp_and_download,
            url,
            path,
            no_playlist,
        )

    def start_tools_update(self):
        if self.busy:
            self.append_log("Another task is already running.", "warning")
            return

        path = self.current_download_path()
        if not path:
            return

        self.start_background_task(self.force_update_tools, path)

    def start_background_task(self, target, *args):
        self.set_busy(True)

        def runner():
            try:
                target(*args)
            except Exception as error:
                self.log_from_worker(f"Error: {error}", "error")
            finally:
                self.signals.busy_changed.emit(False)

        self.worker_thread = threading.Thread(target=runner, daemon=True)
        self.worker_thread.start()

    def download_yt_dlp(self, path):
        self.log_from_worker("Downloading yt-dlp...", "success")
        response = requests.get(YTDLP_URL, stream=True, timeout=120)
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
                        self.log_from_worker(f"yt-dlp download... {percent}%", "process")
                        next_progress += 10

        self.log_from_worker("yt-dlp downloaded successfully.", "success")

    def ensure_ytdlp_and_download(self, url, target_path, no_playlist):
        self.log_from_worker("Checking yt-dlp...", "warning")
        os.makedirs(target_path, exist_ok=True)

        ytdlp_path = os.path.join(target_path, "yt-dlp.exe")
        if not os.path.exists(ytdlp_path):
            self.download_yt_dlp(ytdlp_path)

        self.download_audio(url, target_path, no_playlist)

    def force_update_yt(self, target_path):
        os.makedirs(target_path, exist_ok=True)
        self.download_yt_dlp(os.path.join(target_path, "yt-dlp.exe"))
        self.log_from_worker("yt-dlp updated successfully.", "success")

    def force_update_deno(self):
        try:
            self.log_from_worker("Downloading Deno...", "success")
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
            self.log_from_worker("PowerShell not found on system.", "error")
            return False
        except Exception as error:
            self.log_from_worker(f"Error updating Deno: {error}", "error")
            return False

        output = (process.stdout or "").strip()
        error_output = (process.stderr or "").strip()

        if process.returncode == 0:
            if output:
                self.log_from_worker(output, "process")
            self.log_from_worker("Deno updated successfully.", "success")
            return True

        message = error_output or output or "Unknown error"
        self.log_from_worker(f"Deno update failed: {message}", "error")
        return False

    def force_update_tools(self, target_path):
        self.log_from_worker("Updating yt-dlp and Deno...", "info")

        ytdlp_updated = True
        try:
            self.force_update_yt(target_path)
        except Exception as error:
            ytdlp_updated = False
            self.log_from_worker(f"yt-dlp update failed: {error}", "error")

        deno_updated = self.force_update_deno()
        if ytdlp_updated and deno_updated:
            self.log_from_worker("yt-dlp and Deno updated successfully.", "success")
        elif ytdlp_updated or deno_updated:
            self.log_from_worker("Update finished with one failure.", "warning")
        else:
            self.log_from_worker("yt-dlp and Deno updates failed.", "error")

    def download_audio(self, url, target_path, no_playlist):
        ytdlp_path = os.path.join(target_path, "yt-dlp.exe")
        cmd = [
            ytdlp_path,
            "--newline",
            "-x",
            "--audio-format",
            "mp3",
            "-o",
            os.path.join(target_path, "%(title)s.%(ext)s"),
        ]
        if no_playlist:
            cmd.append("--no-playlist")
        cmd.append(url)

        self.log_from_worker("Downloading audio...", "warning")
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
                    self.log_from_worker(clean_line, "process")
                    percent = extract_percent(clean_line)
                    if percent is not None:
                        self.log_from_worker(f"Downloading... {percent:.0f}%", "warning")

        process.wait()
        if process.returncode == 0:
            self.log_from_worker("Download complete.", "success")
        elif last_output:
            self.log_from_worker(f"Download failed: {last_output}", "error")
        else:
            self.log_from_worker("Download failed.", "error")


def main():
    configure_windows_app_id()
    app = QApplication(sys.argv)
    app.setWindowIcon(create_app_icon())
    app.setStyle("Fusion")
    window = YoutubeToMP3Window()
    window.show()
    if hasattr(app, "exec"):
        return app.exec()
    return app.exec_()


if __name__ == "__main__":
    sys.exit(main())
