import os
import unittest
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from youtube_to_mp3 import playlist_dialogs, settings_dialog
from youtube_to_mp3.qt_compat import QApplication, QLineEdit, QPushButton
from youtube_to_mp3.url_utils import classify_youtube_url


class DialogTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.qt_app = QApplication.instance() or QApplication([])

    def test_youtube_url_classification(self):
        cases = (
            ("https://www.youtube.com/watch?v=abc", "single"),
            ("https://youtu.be/abc", "single"),
            ("https://www.youtube.com/playlist?list=xyz", "playlist"),
            ("https://www.youtube.com/watch?v=abc&list=xyz", "video_in_playlist"),
            ("https://youtu.be/abc?list=xyz", "video_in_playlist"),
            ("https://www.youtube.com/shorts/abc?list=xyz", "video_in_playlist"),
            ("https://www.youtube.com/embed/abc?list=xyz", "video_in_playlist"),
            ("https://www.youtube.com/live/abc?list=xyz", "video_in_playlist"),
        )
        for url, expected in cases:
            with self.subTest(url=url):
                self.assertEqual(classify_youtube_url(url), expected)

    def test_single_video_does_not_prompt_for_playlist(self):
        with patch.object(playlist_dialogs, "exec_dialog") as execute:
            result = playlist_dialogs.confirm_playlist_choice(
                None, "https://www.youtube.com/watch?v=abc", Mock()
            )
        self.assertFalse(result)
        execute.assert_not_called()

    def test_playlist_prompts_return_selected_choice(self):
        cases = (
            ("https://www.youtube.com/watch?v=abc&list=xyz", "Download This Video", True),
            ("https://www.youtube.com/watch?v=abc&list=xyz", "Download Entire Playlist", False),
            ("https://www.youtube.com/watch?v=abc&list=xyz", "Cancel", None),
            ("https://www.youtube.com/playlist?list=xyz", "Download Playlist", False),
            ("https://www.youtube.com/playlist?list=xyz", "Cancel", None),
        )
        for url, label, expected in cases:
            with self.subTest(button=label):
                def click_button(dialog):
                    next(button for button in dialog.buttons() if button.text() == label).click()
                with patch.object(playlist_dialogs, "exec_dialog", side_effect=click_button):
                    self.assertIs(
                        playlist_dialogs.confirm_playlist_choice(None, url, Mock()), expected
                    )

    def test_settings_folder_selection_updates_the_read_only_field(self):
        choose_folder = Mock(return_value="new folder")

        def inspect_dialog(dialog):
            field = dialog.findChild(QLineEdit)
            self.assertTrue(field.isReadOnly())
            self.assertEqual(field.text(), "old folder")
            next(
                button for button in dialog.findChildren(QPushButton)
                if button.text() == "Set Download Folder"
            ).click()
            self.assertEqual(field.text(), "new folder")
            return 0

        with patch.object(settings_dialog, "exec_dialog", side_effect=inspect_dialog):
            settings_dialog.show_settings_dialog(None, "old folder", choose_folder, Mock(), Mock())
        choose_folder.assert_called_once_with()

    def test_settings_update_buttons_invoke_the_correct_action(self):
        for label, expected in (
            ("Check for Updates", "app"),
            ("Update yt-dlp, Deno & FFmpeg", "tools"),
        ):
            with self.subTest(action=expected):
                update_tools, update_app = Mock(), Mock()

                def click_button(dialog):
                    next(
                        button for button in dialog.findChildren(QPushButton)
                        if button.text() == label
                    ).click()
                    return dialog.result()

                with patch.object(settings_dialog, "exec_dialog", side_effect=click_button):
                    result = settings_dialog.show_settings_dialog(
                        None, "folder", Mock(), update_tools, update_app
                    )
                self.assertEqual(result, 1)
                (update_app if expected == "app" else update_tools).assert_called_once_with()
                (update_tools if expected == "app" else update_app).assert_not_called()


if __name__ == "__main__":
    unittest.main()
