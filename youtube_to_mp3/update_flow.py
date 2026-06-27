import threading
import webbrowser

from youtube_to_mp3.qt_compat import (
    ACCEPT_ROLE,
    ACTION_ROLE,
    REJECT_ROLE,
    WARNING_ICON,
    QMessageBox,
    exec_dialog,
)
from youtube_to_mp3.updates import check_for_update, install_update
from youtube_to_mp3.version import APP_NAME, APP_VERSION


def start_update_check(log, signals):
    log(f"Checking for updates (v{APP_VERSION})...", "info")
    threading.Thread(target=run_update_check, args=(signals,), daemon=True).start()


def run_update_check(signals):
    try:
        update = check_for_update()
    except Exception as error:
        signals.update_check_failed.emit(str(error))
        return

    signals.update_check_complete.emit(update)


def handle_update_check_failed(log, message):
    log(f"Update check failed: {message}", "warning")


def handle_update_check_complete(parent, update, log, start_install):
    message = update.get("message") or "Update check complete."
    if not update.get("update_available"):
        log(message, "info")
        return

    log(message, "warning")
    latest_version = update.get("latest_version") or "the latest version"
    release_url = update.get("release_url") or update.get("repo_url") or ""

    message_box = QMessageBox(parent)
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
            start_install()
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


def start_app_update_install(busy, log, start_background_task, install_callback):
    if busy:
        log("Finish the current task before installing an app update.", "warning")
        return

    start_background_task(install_callback)


def install_app_update(log):
    log("Downloading app update...", "warning")
    result = install_update()
    log(result.get("message", "Update downloaded."), "success")
