import os

from youtube_to_mp3.runtime import ICON_PATH


from PyQt6.QtCore import QObject, QPointF, QRectF, QSize, Qt, QTimer, pyqtSignal
from PyQt6.QtGui import QColor, QIcon, QPainter, QPen, QPixmap, QTextCursor
from PyQt6.QtWidgets import (
    QApplication,
    QButtonGroup,
    QComboBox,
    QDialog,
    QFileDialog,
    QFrame,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QMainWindow,
    QMessageBox,
    QPushButton,
    QTextEdit,
    QVBoxLayout,
    QWidget,
)


TEXT_CURSOR_END = QTextCursor.MoveOperation.End
WARNING_ICON = QMessageBox.Icon.Warning
ACCEPT_ROLE = QMessageBox.ButtonRole.AcceptRole
ACTION_ROLE = QMessageBox.ButtonRole.ActionRole
REJECT_ROLE = QMessageBox.ButtonRole.RejectRole
PASSWORD_ECHO_MODE = QLineEdit.EchoMode.Password


def exec_dialog(dialog):
    return dialog.exec()


def create_app_icon():
    if os.path.exists(ICON_PATH):
        return QIcon(ICON_PATH)
    return QIcon()
