from youtube_to_mp3.qt_compat import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QPushButton,
    QVBoxLayout,
    create_app_icon,
    exec_dialog,
)


def show_settings_dialog(
    parent,
    download_path,
    choose_download_folder,
    start_tools_update,
    check_for_updates,
):
    dialog = QDialog(parent)
    dialog.setWindowTitle("Settings")
    dialog.setWindowIcon(create_app_icon())
    dialog.setModal(True)
    dialog.resize(610, 270)

    layout = QVBoxLayout(dialog)
    layout.setContentsMargins(24, 22, 24, 22)
    layout.setSpacing(14)

    heading = QLabel("Settings")
    heading.setObjectName("dialogTitle")
    layout.addWidget(heading)
    description = QLabel("Manage your download folder and updates.")
    description.setObjectName("helperText")
    layout.addWidget(description)

    folder_label = QLabel("DOWNLOAD FOLDER")
    folder_label.setObjectName("sectionLabel")
    layout.addWidget(folder_label)

    folder_row = QHBoxLayout()
    folder_input = QLineEdit()
    folder_input.setPlaceholderText("No download folder set")
    folder_input.setReadOnly(True)
    folder_input.setText(download_path)
    folder_input.setFixedHeight(42)
    set_folder_button = QPushButton("Set Download Folder")
    set_folder_button.setFixedHeight(42)
    folder_row.addWidget(folder_input, 1)
    folder_row.addWidget(set_folder_button)
    layout.addLayout(folder_row)

    updates_label = QLabel("UPDATES")
    updates_label.setObjectName("sectionLabel")
    layout.addWidget(updates_label)

    actions_row = QHBoxLayout()
    check_updates_button = QPushButton("Check for Updates")
    update_tools_button = QPushButton("Update yt-dlp, Deno & FFmpeg")
    actions_row.addWidget(check_updates_button)
    actions_row.addWidget(update_tools_button)
    actions_row.addStretch(1)
    layout.addLayout(actions_row)

    close_row = QHBoxLayout()
    close_row.addStretch(1)
    close_button = QPushButton("Close")
    close_row.addWidget(close_button)
    layout.addLayout(close_row)

    def set_folder():
        selected_path = choose_download_folder()
        if selected_path:
            folder_input.setText(selected_path)

    def update_tools():
        start_tools_update()
        dialog.accept()

    def check_updates():
        check_for_updates()
        dialog.accept()

    set_folder_button.clicked.connect(set_folder)
    check_updates_button.clicked.connect(check_updates)
    update_tools_button.clicked.connect(update_tools)
    close_button.clicked.connect(dialog.reject)
    return exec_dialog(dialog)
