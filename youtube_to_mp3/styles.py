LOG_COLORS = {
    "info": "#d1d5db",
    "success": "#22c55e",
    "warning": "#facc15",
    "error": "#f87171",
    "process": "#93c5fd",
    "progress": "#93c5fd",
}


APP_STYLES = """
QMainWindow, QWidget {
    background: #081321;
    color: #edf5ff;
    font-family: "Segoe UI", Arial, sans-serif;
    font-size: 13px;
}
QLabel {
    background: transparent;
    border: none;
}
QLabel#appTitle {
    color: #f4f8ff;
    font-size: 22px;
    font-weight: 700;
}
QLabel#appSubtitle {
    color: #718aa5;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 1px;
}
QLabel#sectionLabel {
    color: #9cb0c6;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.6px;
}
QLabel#helperText {
    color: #7890a9;
    font-size: 11px;
}
QLabel#activityTitle, QLabel#dialogTitle {
    color: #f2f7ff;
    font-size: 15px;
    font-weight: 700;
}
QLineEdit {
    background: #101f32;
    border: 1px solid #30445e;
    border-radius: 7px;
    color: #edf5ff;
    padding: 0 12px;
    selection-background-color: #08798a;
}
QLineEdit:focus {
    border-color: #17c6d7;
}
QLineEdit:read-only {
    color: #c6d5e5;
}
QComboBox {
    background: #14243a;
    border: 1px solid #354b66;
    border-radius: 7px;
    color: #f0f6ff;
    padding: 0 12px;
}
QComboBox:hover, QComboBox:focus {
    border-color: #17c6d7;
}
QComboBox:disabled {
    color: #71869c;
    background: #101c2b;
    border-color: #26384d;
}
QComboBox::drop-down {
    border: none;
    width: 26px;
}
QComboBox::down-arrow {
    image: none;
}
QComboBox QAbstractItemView {
    background: #14243a;
    border: 1px solid #354b66;
    color: #edf5ff;
    selection-background-color: #0a6573;
    outline: none;
}
QPushButton {
    background: #1a2c43;
    border: 1px solid #354b66;
    border-radius: 7px;
    color: #edf5ff;
    font-weight: 600;
    padding: 9px 15px;
}
QPushButton:hover {
    background: #243a54;
    border-color: #4b6685;
}
QPushButton:pressed {
    background: #10283d;
}
QPushButton:disabled {
    color: #6a7f94;
    background: #152234;
    border-color: #26384d;
}
QPushButton#formatButton:checked {
    background: #0a4655;
    border: 1px solid #17c6d7;
    color: #f0feff;
}
QPushButton#formatButton:checked:disabled {
    background: #12313d;
    border-color: #29616c;
    color: #829da6;
}
QPushButton#primaryButton {
    background: #09b7cb;
    border: 1px solid #1ad3e1;
    color: #051820;
    font-size: 15px;
    font-weight: 700;
}
QPushButton#primaryButton:hover {
    background: #20c9d9;
}
QPushButton#primaryButton:pressed {
    background: #0797aa;
}
QPushButton#primaryButton:disabled {
    background: #194c57;
    border-color: #28616a;
    color: #87a6ab;
}
QPushButton#quietButton {
    padding: 5px 12px;
    font-size: 12px;
}
QFrame#activityPanel {
    background: #102035;
    border: 1px solid #2b405a;
    border-radius: 9px;
}
QTextEdit#logView {
    background: #091625;
    border: 1px solid #293f59;
    border-radius: 7px;
    color: #d5e2ef;
    padding: 9px;
    selection-background-color: #08798a;
}
"""
