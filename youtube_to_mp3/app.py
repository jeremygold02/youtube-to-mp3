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
    QButtonGroup,
    QColor,
    QComboBox,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QIcon,
    QLabel,
    QLineEdit,
    QMainWindow,
    QObject,
    QPainter,
    QPen,
    QPixmap,
    QPointF,
    QPushButton,
    QRectF,
    QSize,
    QTextCursor,
    QTextEdit,
    Qt,
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


def make_icon(name, color="#e6f2ff"):
    pixmap = QPixmap(20, 20)
    pixmap.fill(Qt.GlobalColor.transparent)
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing)
    pen = QPen(QColor(color), 1.6)
    pen.setCapStyle(Qt.PenCapStyle.RoundCap)
    pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
    painter.setPen(pen)

    def line(x1, y1, x2, y2):
        painter.drawLine(QPointF(x1, y1), QPointF(x2, y2))

    if name == "audio":
        line(8, 5, 8, 14)
        line(8, 5, 15, 3)
        line(15, 3, 15, 12)
        painter.drawEllipse(QRectF(4, 13, 4, 3))
        painter.drawEllipse(QRectF(11, 11, 4, 3))
    elif name == "video":
        painter.drawRoundedRect(QRectF(2, 5, 11, 10), 2, 2)
        line(13, 8, 18, 5)
        line(18, 5, 18, 15)
        line(18, 15, 13, 12)
    elif name == "download":
        line(10, 2, 10, 13)
        line(6, 9, 10, 13)
        line(10, 13, 14, 9)
        line(3, 14, 3, 17)
        line(3, 17, 17, 17)
        line(17, 17, 17, 14)
    elif name == "folder":
        line(2, 5, 7, 5)
        line(7, 5, 9, 7)
        line(9, 7, 18, 7)
        line(18, 7, 18, 16)
        line(18, 16, 2, 16)
        line(2, 16, 2, 5)
    elif name == "settings":
        painter.drawEllipse(QRectF(6, 6, 8, 8))
        for x1, y1, x2, y2 in (
            (10, 2, 10, 4), (10, 16, 10, 18), (2, 10, 4, 10),
            (16, 10, 18, 10), (4, 4, 5.5, 5.5), (14.5, 14.5, 16, 16),
            (16, 4, 14.5, 5.5), (5.5, 14.5, 4, 16),
        ):
            line(x1, y1, x2, y2)
    elif name == "clear":
        line(4, 5, 16, 5)
        line(8, 3, 12, 3)
        line(6, 7, 7, 17)
        line(7, 17, 13, 17)
        line(13, 17, 14, 7)
        line(10, 8, 10, 14)
    painter.end()
    return QIcon(pixmap)


class QualityComboBox(QComboBox):
    def paintEvent(self, event):
        super().paintEvent(event)
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing)
        pen = QPen(QColor("#b9cadb" if self.isEnabled() else "#63788d"), 1.7)
        pen.setCapStyle(Qt.PenCapStyle.RoundCap)
        pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
        painter.setPen(pen)
        x = self.width() - 18
        y = self.height() / 2 - 2
        painter.drawLine(QPointF(x - 4, y), QPointF(x, y + 4))
        painter.drawLine(QPointF(x, y + 4), QPointF(x + 4, y))


class YoutubeToMP3Window(QMainWindow):
    def __init__(self):
        super().__init__()
        self.config = load_or_create_config()
        self.download_path = self.config.get("download_path", "")
        self.worker_thread = None
        self.busy = False
        self.last_log_was_progress = False
        self.download_manager = DownloadManager(self.log_from_worker)

        self.signals = WorkerSignals()
        self.signals.log.connect(self.append_log)
        self.signals.busy_changed.connect(self.set_busy)
        self.signals.update_check_complete.connect(self.handle_update_check_complete)
        self.signals.update_check_failed.connect(self.handle_update_check_failed)

        self.setWindowTitle(APP_NAME)
        self.setWindowIcon(create_app_icon())
        self.resize(760, 620)
        self.setMinimumSize(640, 540)
        self.build_ui()
        self.apply_styles()
        QTimer.singleShot(700, self.check_for_updates_on_launch)

    def build_ui(self):
        root = QWidget()
        root.setObjectName("root")
        layout = QVBoxLayout(root)
        layout.setContentsMargins(26, 24, 26, 24)
        layout.setSpacing(16)

        header = QHBoxLayout()
        heading = QVBoxLayout()
        heading.setSpacing(2)
        title = QLabel(APP_NAME)
        title.setObjectName("appTitle")
        subtitle = QLabel("AUDIO & VIDEO DOWNLOADER")
        subtitle.setObjectName("appSubtitle")
        heading.addWidget(title)
        heading.addWidget(subtitle)
        header.addLayout(heading, 1)

        self.settings_button = QPushButton(" Settings")
        self.settings_button.setObjectName("secondaryButton")
        self.settings_button.setIcon(make_icon("settings"))
        self.settings_button.setFixedHeight(38)
        self.settings_button.clicked.connect(self.open_settings)
        header.addWidget(self.settings_button)
        layout.addLayout(header)

        source_column = QVBoxLayout()
        source_column.setSpacing(8)
        source_label = QLabel("VIDEO LINK")
        source_label.setObjectName("sectionLabel")
        source_column.addWidget(source_label)

        self.url_input = QLineEdit()
        self.url_input.setAccessibleName("Video link")
        self.url_input.setPlaceholderText("Paste a YouTube link here")
        self.url_input.setFixedHeight(44)
        self.url_input.setClearButtonEnabled(True)
        self.url_input.returnPressed.connect(self.start_download)
        source_column.addWidget(self.url_input)
        layout.addLayout(source_column)

        options_row = QHBoxLayout()
        options_row.setSpacing(16)
        format_column = QVBoxLayout()
        format_column.setSpacing(8)
        format_label = QLabel("OUTPUT FORMAT")
        format_label.setObjectName("sectionLabel")
        format_column.addWidget(format_label)
        format_row = QHBoxLayout()
        format_row.setSpacing(8)
        self.format_group = QButtonGroup(self)
        self.mp3_button = QPushButton(" MP3 Audio")
        self.mp4_button = QPushButton(" MP4 Video")
        self.mp3_button.setIcon(make_icon("audio"))
        self.mp4_button.setIcon(make_icon("video"))
        for button in (self.mp3_button, self.mp4_button):
            button.setObjectName("formatButton")
            button.setCheckable(True)
            button.setFixedHeight(44)
            button.setIconSize(QSize(18, 18))
            self.format_group.addButton(button)
            format_row.addWidget(button, 1)
        self.mp3_button.setChecked(True)
        format_column.addLayout(format_row)
        options_row.addLayout(format_column, 3)

        quality_column = QVBoxLayout()
        quality_column.setSpacing(8)
        quality_label = QLabel("VIDEO QUALITY")
        quality_label.setObjectName("sectionLabel")
        quality_column.addWidget(quality_label)
        self.quality_input = QualityComboBox()
        self.quality_input.setObjectName("qualityInput")
        self.quality_input.setAccessibleName("Video quality")
        self.quality_input.setFixedHeight(44)
        for label, height in (
            ("Best available", None),
            ("Up to 2160p (4K)", 2160),
            ("Up to 1440p (QHD)", 1440),
            ("Up to 1080p (Full HD)", 1080),
            ("Up to 720p (HD)", 720),
            ("Up to 480p", 480),
            ("Up to 360p", 360),
        ):
            self.quality_input.addItem(label, height)
        self.quality_input.setCurrentIndex(1)
        quality_column.addWidget(self.quality_input)
        options_row.addLayout(quality_column, 2)
        layout.addLayout(options_row)

        self.mp4_button.toggled.connect(self.update_format_controls)
        self.update_format_controls()

        self.download_button = QPushButton(" Download MP3")
        self.download_button.setObjectName("primaryButton")
        self.download_button.setIcon(make_icon("download", "#051820"))
        self.download_button.setIconSize(QSize(19, 19))
        self.download_button.setFixedHeight(48)
        self.download_button.clicked.connect(self.start_download)
        layout.addWidget(self.download_button)

        path_column = QVBoxLayout()
        path_column.setSpacing(8)
        path_label = QLabel("SAVE TO")
        path_label.setObjectName("sectionLabel")
        path_column.addWidget(path_label)

        self.path_input = QLineEdit()
        self.path_input.setAccessibleName("Download folder")
        self.path_input.setPlaceholderText("No download folder set")
        self.path_input.setReadOnly(True)
        self.path_input.setFixedHeight(44)
        self.update_download_path_display()

        self.open_folder_button = QPushButton(" Open Folder")
        self.open_folder_button.setObjectName("secondaryButton")
        self.open_folder_button.setIcon(make_icon("folder"))
        self.open_folder_button.setFixedHeight(44)
        self.open_folder_button.clicked.connect(self.open_download_folder)

        path_row = QHBoxLayout()
        path_row.setSpacing(10)
        path_row.addWidget(self.path_input, 1)
        path_row.addWidget(self.open_folder_button)
        path_column.addLayout(path_row)
        layout.addLayout(path_column)

        activity_panel = QFrame()
        activity_panel.setObjectName("activityPanel")
        activity_layout = QVBoxLayout(activity_panel)
        activity_layout.setContentsMargins(14, 12, 14, 14)
        activity_layout.setSpacing(10)
        activity_header = QHBoxLayout()
        activity_title = QLabel("Activity log")
        activity_title.setObjectName("activityTitle")
        activity_header.addWidget(activity_title, 1)
        self.clear_log_button = QPushButton(" Clear")
        self.clear_log_button.setObjectName("quietButton")
        self.clear_log_button.setIcon(make_icon("clear"))
        self.clear_log_button.clicked.connect(self.clear_log)
        activity_header.addWidget(self.clear_log_button)
        activity_layout.addLayout(activity_header)

        self.log_view = QTextEdit()
        self.log_view.setReadOnly(True)
        self.log_view.setObjectName("logView")
        self.log_view.setAccessibleName("Activity log")
        self.log_view.setPlaceholderText("Your download activity will appear here.")
        activity_layout.addWidget(self.log_view, 1)
        layout.addWidget(activity_panel, 1)

        self.setCentralWidget(root)

    def apply_styles(self):
        self.setStyleSheet(APP_STYLES)

    def update_format_controls(self):
        is_video = self.mp4_button.isChecked()
        self.quality_input.setEnabled(is_video and not self.busy)
        if hasattr(self, "download_button"):
            self.download_button.setText(" Download MP4" if is_video else " Download MP3")

    def append_log(self, message, level="info"):
        color = LOG_COLORS.get(level, LOG_COLORS["info"])
        timestamp = datetime.now().strftime("%H:%M:%S")
        safe_message = html.escape(message).replace("\n", "<br>")
        formatted_message = (
            f'<span style="color: #64748b;">[{timestamp}]</span> '
            f'<span style="color: {color};">{safe_message}</span>'
        )
        if level == "progress" and self.last_log_was_progress:
            cursor = self.log_view.textCursor()
            cursor.movePosition(TEXT_CURSOR_END)
            cursor.movePosition(
                QTextCursor.MoveOperation.StartOfBlock, QTextCursor.MoveMode.KeepAnchor
            )
            cursor.removeSelectedText()
            cursor.insertHtml(formatted_message)
            self.log_view.setTextCursor(cursor)
        else:
            self.log_view.append(formatted_message)
        self.last_log_was_progress = level == "progress"
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
            self.mp3_button,
            self.mp4_button,
        ):
            button.setDisabled(busy)
        self.update_format_controls()

    def log_from_worker(self, message, level="info"):
        self.signals.log.emit(message, level)

    def check_for_updates_on_launch(self):
        start_update_check(self.append_log, self.signals, silent_if_current=True)

    def check_for_updates(self):
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
        self.path_input.setToolTip(self.download_path)

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
            self.check_for_updates,
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
            "mp4" if self.mp4_button.isChecked() else "mp3",
            self.quality_input.currentData() if self.mp4_button.isChecked() else None,
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
