import os

from youtube_to_mp3.runtime import ICON_PATH


try:
    from PyQt6.QtCore import QObject, QTimer, pyqtSignal
    from PyQt6.QtGui import QIcon, QTextCursor
    from PyQt6.QtWidgets import (
        QApplication,
        QDialog,
        QFileDialog,
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
        QDialog,
        QFileDialog,
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


if PYQT_VERSION == 6:
    TEXT_CURSOR_END = QTextCursor.MoveOperation.End
    WARNING_ICON = QMessageBox.Icon.Warning
    ACCEPT_ROLE = QMessageBox.ButtonRole.AcceptRole
    ACTION_ROLE = QMessageBox.ButtonRole.ActionRole
    REJECT_ROLE = QMessageBox.ButtonRole.RejectRole
    PASSWORD_ECHO_MODE = QLineEdit.EchoMode.Password
else:
    TEXT_CURSOR_END = QTextCursor.End
    WARNING_ICON = QMessageBox.Warning
    ACCEPT_ROLE = QMessageBox.AcceptRole
    ACTION_ROLE = QMessageBox.ActionRole
    REJECT_ROLE = QMessageBox.RejectRole
    PASSWORD_ECHO_MODE = QLineEdit.Password


def exec_dialog(dialog):
    if hasattr(dialog, "exec"):
        return dialog.exec()
    return dialog.exec_()


def create_app_icon():
    if os.path.exists(ICON_PATH):
        return QIcon(ICON_PATH)
    return QIcon()
