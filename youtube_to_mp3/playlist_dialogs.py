from youtube_to_mp3.qt_compat import (
    ACCEPT_ROLE,
    ACTION_ROLE,
    REJECT_ROLE,
    WARNING_ICON,
    QMessageBox,
    exec_dialog,
)
from youtube_to_mp3.url_utils import classify_youtube_url


def confirm_playlist_choice(parent, url, log):
    url_kind = classify_youtube_url(url)
    if url_kind == "single":
        return False

    if url_kind == "video_in_playlist":
        message_box = QMessageBox(parent)
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
            log("Playlist URL detected. Downloading this video only.", "warning")
            return True
        if clicked_button == playlist_button:
            log("Playlist URL detected. Downloading the entire playlist.", "warning")
            return False

        log("Download canceled.", "info")
        return None

    message_box = QMessageBox(parent)
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
        log("Playlist URL detected. Downloading the entire playlist.", "warning")
        return False

    log("Download canceled.", "info")
    return None
