LOG_COLORS = {
    "info": "#d1d5db",
    "success": "#22c55e",
    "warning": "#facc15",
    "error": "#f87171",
    "process": "#93c5fd",
}


APP_STYLES = """
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
