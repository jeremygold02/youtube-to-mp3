import html
import os
import subprocess
import sys
import threading
from datetime import datetime

if __package__ in {None, ""}:
    sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from youtube_to_mp3.config import load_or_create_config, save_config
from youtube_to_mp3.downloaders import DownloadManager
from youtube_to_mp3.playlist_dialogs import confirm_playlist_choice
from youtube_to_mp3.qt_compat import (
    TEXT_CURSOR_END,
    QApplication,
    QFileDialog,
    QHBoxLayout,
    QLineEdit,
    QMainWindow,
    QObject,
    QPushButton,
    QTextEdit,
    QTimer,
    QVBoxLayout,
    QWidget,
    create_app_icon,
    pyqtSignal,
)
from youtube_to_mp3.runtime import configure_windows_app_id, hidden_subprocess_kwargs
from youtube_to_mp3.settings_dialog import show_settings_dialog
from youtube_to_mp3.styles import APP_STYLES, LOG_COLORS
from youtube_to_mp3.update_flow import (
    handle_update_check_complete,
    handle_update_check_failed,
    install_app_update,
    start_app_update_install,
    start_update_check,
)
from youtube_to_mp3.version import APP_NAME


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
        self.download_manager = DownloadManager(self.log_from_worker)

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
        self.path_input.setPlaceholderText("No download folder set")
        self.path_input.setReadOnly(True)
        self.update_download_path_display()

        self.open_folder_button = QPushButton("Open Folder")
        self.open_folder_button.clicked.connect(self.open_download_folder)

        self.settings_button = QPushButton("Settings")
        self.settings_button.clicked.connect(self.open_settings)

        path_row = QHBoxLayout()
        path_row.addWidget(self.path_input, 1)
        path_row.addWidget(self.open_folder_button)
        path_row.addWidget(self.settings_button)
        layout.addLayout(path_row)

        actions_row = QHBoxLayout()
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
        self.setStyleSheet(APP_STYLES)

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
            self.open_folder_button,
            self.settings_button,
        ):
            button.setDisabled(busy)

    def log_from_worker(self, message, level="info"):
        self.signals.log.emit(message, level)

    def check_for_updates_on_launch(self):
        start_update_check(self.append_log, self.signals)

    def handle_update_check_failed(self, message):
        handle_update_check_failed(self.append_log, message)

    def handle_update_check_complete(self, update):
        handle_update_check_complete(
            self,
            update,
            self.append_log,
            self.start_app_update_install,
        )

    def start_app_update_install(self):
        start_app_update_install(
            self.busy,
            self.append_log,
            self.start_background_task,
            self.install_app_update,
        )

    def install_app_update(self):
        install_app_update(self.log_from_worker)

    def save_download_path(self, path):
        self.download_path = path
        self.config["download_path"] = path
        save_config(self.config)
        self.update_download_path_display()
        self.append_log(f"Download path updated: {path}", "success")
        return path

    def update_download_path_display(self):
        self.path_input.setText(self.download_path)

    def current_download_path(self):
        path = self.download_path.strip()
        if not path:
            self.append_log("Error: No download path set.", "error")
            return None

        return path

    def choose_download_folder(self):
        start_dir = self.download_path or os.path.expanduser("~")
        selected_path = QFileDialog.getExistingDirectory(
            self,
            "Set Download Folder",
            start_dir,
        )
        if selected_path:
            self.save_download_path(selected_path)
        return selected_path

    def open_settings(self):
        show_settings_dialog(
            self,
            self.download_path,
            self.choose_download_folder,
            self.start_tools_update,
            self.check_for_updates_on_launch,
        )

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

        no_playlist = confirm_playlist_choice(self, url, self.append_log)
        if no_playlist is None:
            return

        self.start_background_task(
            self.download_manager.ensure_ytdlp_and_download,
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

        self.start_background_task(self.download_manager.force_update_tools, path)

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
