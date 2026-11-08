import io
import os
import tempfile
import unittest
from unittest.mock import Mock, patch

os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")

from youtube_to_mp3 import app as app_module
from youtube_to_mp3.downloaders import DownloadManager, extract_percent


class DownloadCommandTests(unittest.TestCase):
    def setUp(self):
        self.messages = []
        self.manager = DownloadManager(
            lambda message, level="info": self.messages.append((message, level))
        )

    def run_download(self, output_format, height=None, no_playlist=False, returncode=0):
        process = Mock()
        process.stdout = io.StringIO("[download] 25.0%\nfinished\n")
        process.returncode = returncode
        with patch("youtube_to_mp3.downloaders.local_ffmpeg_directory", return_value=None):
            with patch("youtube_to_mp3.downloaders.subprocess.Popen", return_value=process) as popen:
                self.manager.download_media(
                    "https://example.com/video", "a folder", no_playlist, output_format, height
                )
        self.assertTrue(process.stdout.closed)
        return popen.call_args.args[0]

    def test_mp3_retains_audio_extraction_and_playlist_choice(self):
        command = self.run_download("mp3", no_playlist=True)
        self.assertIn("-x", command)
        self.assertEqual(command[command.index("--audio-format") + 1], "mp3")
        self.assertIn("--no-playlist", command)
        self.assertNotIn("--merge-output-format", command)
        self.assertEqual(command[-1], "https://example.com/video")
        self.assertIn(("Download complete.", "success"), self.messages)

    def test_progress_ignores_urls_filenames_and_out_of_range_values(self):
        cases = (
            ("[download]   0.0% of 1MiB", 0.0),
            ("[download]  25.7% of 1MiB", 25.7),
            ("[download] 100%", 100.0),
            ("[download] 706% of 1MiB", None),
            ("[download] 1000% of 1MiB", None),
            ("[download] Destination: 50% complete.mp4", None),
            ("Error opening URL: example.test/?value=1706%2C", None),
        )
        for line, expected in cases:
            with self.subTest(line=line):
                self.assertEqual(extract_percent(line), expected)

    def test_download_progress_is_reported_once_per_update(self):
        process = Mock(
            stdout=io.StringIO(
                "[download] Destination: example.mp4\n"
                "[download]  7.3% of 10MiB\n"
                "[download]  7.9% of 10MiB\n"
                "[Merger] Merging formats\n"
            ),
            returncode=0,
        )
        with patch("youtube_to_mp3.downloaders.local_ffmpeg_directory", return_value=None):
            with patch("youtube_to_mp3.downloaders.subprocess.Popen", return_value=process):
                self.manager.download_media("url", "output", False, "mp4", 2160)
        progress = [(message, level) for message, level in self.messages if level == "progress"]
        self.assertEqual(
            progress,
            [("[download]  7.3% of 10MiB", "progress"), ("[download]  7.9% of 10MiB", "progress")],
        )
        self.assertIn(("[Merger] Merging formats", "process"), self.messages)
        self.assertFalse(any(message.startswith("Downloading... ") for message, _ in self.messages))

    def test_mp4_applies_maximum_height_and_remuxes(self):
        command = self.run_download("mp4", height=720)
        self.assertEqual(
            command[command.index("-f") + 1],
            "bv*[height<=720]+ba[ext=m4a]/"
            "bv*[height<=720]+ba/b[height<=720]/bv*[height<=720]",
        )
        self.assertEqual(command[command.index("--merge-output-format") + 1], "mp4")
        self.assertEqual(command[command.index("--remux-video") + 1], "mp4")
        self.assertNotIn("--no-playlist", command)
        self.assertNotIn("-x", command)

    def test_mp4_best_available_has_no_height_filter(self):
        command = self.run_download("mp4")
        self.assertEqual(command[command.index("-f") + 1], "bv*+ba[ext=m4a]/bv*+ba/b/bv*")

    def test_download_failure_is_reported(self):
        self.run_download("mp4", returncode=1)
        self.assertIn(("Download failed: finished", "error"), self.messages)
        self.assertNotIn(("Download complete.", "success"), self.messages)

    def test_download_failure_without_process_output_is_reported(self):
        process = Mock(stdout=None, returncode=1)
        with patch("youtube_to_mp3.downloaders.local_ffmpeg_directory", return_value=None):
            with patch("youtube_to_mp3.downloaders.subprocess.Popen", return_value=process):
                self.manager.download_media("url", "output", False, "mp4", 2160)
        self.assertIn(("Download failed.", "error"), self.messages)

    def test_invalid_format_or_height_fails_before_creating_output_folder(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            target = os.path.join(temp_dir, "downloads")
            for output_format, height in (
                ("other", None), ("mp4", 0), ("mp4", -1),
                ("mp4", "720"), ("mp4", True), ("mp4", 720.5),
            ):
                with self.subTest(output_format=output_format, height=height):
                    with self.assertRaises(ValueError):
                        self.manager.ensure_ytdlp_and_download(
                            "https://example.com/video", target, False, output_format, height
                        )
                    self.assertFalse(os.path.exists(target))

    def test_app_local_ffmpeg_is_passed_to_ytdlp(self):
        process = Mock(stdout=io.StringIO(""), returncode=0)
        with patch("youtube_to_mp3.downloaders.local_ffmpeg_directory", return_value="tools folder"):
            with patch("youtube_to_mp3.downloaders.subprocess.Popen", return_value=process) as popen:
                self.manager.download_media("https://example.com", "output", True, "mp4", 2160)
        command = popen.call_args.args[0]
        self.assertEqual(command[command.index("--ffmpeg-location") + 1], "tools folder")

    def test_setup_downloads_missing_tool_and_preserves_existing_tool(self):
        with tempfile.TemporaryDirectory() as target:
            with patch.object(self.manager, "download_yt_dlp") as download_tool:
                with patch.object(self.manager, "ensure_ffmpeg") as ensure_ffmpeg:
                    with patch.object(self.manager, "download_media") as download_media:
                        self.manager.ensure_ytdlp_and_download("url", target, True, "mp4", 2160)
                        download_tool.assert_called_once_with(os.path.join(target, "yt-dlp.exe"))
                        ensure_ffmpeg.assert_called_once_with(target)
                        download_media.assert_called_once_with("url", target, True, "mp4", 2160)
                        download_tool.reset_mock()
                        with open(os.path.join(target, "yt-dlp.exe"), "wb"):
                            pass
                        self.manager.ensure_ytdlp_and_download("url", target, False)
                        download_tool.assert_not_called()

    def test_ffmpeg_setup_error_does_not_start_download(self):
        with tempfile.TemporaryDirectory() as target:
            with patch.object(self.manager, "download_yt_dlp"):
                with patch.object(self.manager, "ensure_ffmpeg", side_effect=RuntimeError("unavailable")):
                    with patch.object(self.manager, "download_media") as download_media:
                        with self.assertRaisesRegex(RuntimeError, "unavailable"):
                            self.manager.ensure_ytdlp_and_download("url", target, False, "mp4", 2160)
                        download_media.assert_not_called()


class DownloadWindowTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.qt_app = app_module.QApplication.instance() or app_module.QApplication([])

    def setUp(self):
        config = patch(
            "youtube_to_mp3.app.load_or_create_config",
            return_value={"download_path": "a folder"},
        )
        timer = patch("youtube_to_mp3.app.QTimer")
        config.start()
        timer.start()
        self.addCleanup(config.stop)
        self.addCleanup(timer.stop)
        self.window = app_module.YoutubeToMP3Window()
        self.addCleanup(self.window.close)
        self.window.start_background_task = Mock()
        self.window.url_input.setText("https://example.com/video")

    def test_format_choice_controls_quality_and_passes_selection_to_worker(self):
        self.assertFalse(self.window.quality_input.isEnabled())
        self.window.mp4_button.click()
        self.assertTrue(self.window.quality_input.isEnabled())
        self.assertEqual(self.window.quality_input.currentData(), 2160)
        self.window.quality_input.setCurrentIndex(4)
        with patch("youtube_to_mp3.app.confirm_playlist_choice", return_value=True):
            self.window.start_download()
        self.window.start_background_task.assert_called_once_with(
            self.window.download_manager.ensure_ytdlp_and_download,
            "https://example.com/video",
            "a folder",
            True,
            "mp4",
            720,
        )
        self.window.set_busy(True)
        self.assertFalse(self.window.quality_input.isEnabled())
        self.window.set_busy(False)
        self.assertTrue(self.window.quality_input.isEnabled())
        self.window.mp3_button.click()
        self.assertFalse(self.window.quality_input.isEnabled())

    def test_canceling_playlist_choice_does_not_start_worker(self):
        with patch("youtube_to_mp3.app.confirm_playlist_choice", return_value=None):
            self.window.start_download()
        self.window.start_background_task.assert_not_called()

    def test_every_quality_option_reaches_the_worker(self):
        self.window.mp4_button.click()
        with patch("youtube_to_mp3.app.confirm_playlist_choice", return_value=False):
            for index, expected in enumerate((None, 2160, 1440, 1080, 720, 480, 360)):
                with self.subTest(quality=expected):
                    self.window.quality_input.setCurrentIndex(index)
                    self.window.start_download()
                    self.assertEqual(self.window.start_background_task.call_args.args[-2:], ("mp4", expected))

    def test_mp3_ignores_video_quality_and_keeps_playlist_choice(self):
        self.window.mp4_button.click()
        self.window.quality_input.setCurrentIndex(5)
        self.window.mp3_button.click()
        with patch("youtube_to_mp3.app.confirm_playlist_choice", return_value=False):
            self.window.start_download()
        self.window.start_background_task.assert_called_once_with(
            self.window.download_manager.ensure_ytdlp_and_download,
            "https://example.com/video", "a folder", False, "mp3", None,
        )
        self.assertFalse(self.window.quality_input.isEnabled())
        self.window.mp4_button.click()
        self.assertEqual(self.window.quality_input.currentData(), 480)

    def test_empty_url_or_missing_folder_does_not_start_download(self):
        with patch("youtube_to_mp3.app.confirm_playlist_choice") as confirm:
            self.window.url_input.setText("   ")
            self.window.start_download()
            self.assertIn("YouTube URL required", self.window.log_view.toPlainText())
            self.window.url_input.setText("https://example.com/video")
            self.window.download_path = "  "
            self.window.start_download()
            self.assertIn("No download path set", self.window.log_view.toPlainText())
            confirm.assert_not_called()
        self.window.start_background_task.assert_not_called()

    def test_busy_state_blocks_another_download(self):
        self.window.mp4_button.click()
        self.window.set_busy(True)
        for control in (
            self.window.download_button, self.window.mp3_button, self.window.mp4_button,
            self.window.quality_input, self.window.settings_button, self.window.open_folder_button,
        ):
            self.assertFalse(control.isEnabled())
        self.assertTrue(self.window.clear_log_button.isEnabled())
        with patch("youtube_to_mp3.app.confirm_playlist_choice") as confirm:
            self.window.start_download()
            confirm.assert_not_called()
        self.window.start_background_task.assert_not_called()
        self.assertIn("Another task is already running", self.window.log_view.toPlainText())

    def test_log_escapes_text_and_clear_removes_previous_entries(self):
        self.window.append_log("<b>literal</b>\nsecond line", "warning")
        self.assertIn("<b>literal</b>\nsecond line", self.window.log_view.toPlainText())
        self.window.clear_log_button.click()
        self.assertIn("Log cleared.", self.window.log_view.toPlainText())
        self.assertNotIn("literal", self.window.log_view.toPlainText())

    def test_progress_updates_replace_the_previous_progress_line(self):
        self.window.append_log("Starting download.")
        for percent in range(100):
            self.window.append_log(f"[download] {percent}%", "progress")
        lines = self.window.log_view.toPlainText().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertIn("Starting download.", lines[0])
        self.assertIn("[download] 99%", lines[1])

        self.window.append_log("Merging formats.", "process")
        self.window.append_log("[download] 10%", "progress")
        self.window.append_log("[download] 20%", "progress")
        lines = self.window.log_view.toPlainText().splitlines()
        self.assertEqual(len(lines), 4)
        self.assertIn("[download] 99%", lines[1])
        self.assertIn("Merging formats.", lines[2])
        self.assertIn("[download] 20%", lines[3])

    def test_clearing_progress_preserves_the_clear_message(self):
        self.window.append_log("[download] 10%", "progress")
        self.window.clear_log()
        self.window.append_log("[download] 20%", "progress")
        lines = self.window.log_view.toPlainText().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertIn("Log cleared.", lines[0])
        self.assertIn("[download] 20%", lines[1])

    def test_folder_choice_updates_display_and_cancel_keeps_previous_path(self):
        with patch("youtube_to_mp3.app.save_config") as save_config:
            with patch("youtube_to_mp3.app.QFileDialog.getExistingDirectory", return_value="new folder"):
                self.window.choose_download_folder()
            self.assertEqual(self.window.path_input.text(), "new folder")
            self.assertEqual(self.window.path_input.toolTip(), "new folder")
            save_config.assert_called_once_with({"download_path": "new folder"})
            with patch("youtube_to_mp3.app.QFileDialog.getExistingDirectory", return_value=""):
                self.window.choose_download_folder()
            self.assertEqual(self.window.path_input.text(), "new folder")
            save_config.assert_called_once()

    @unittest.skipUnless(os.name == "nt", "Tests the Windows folder-opening action")
    def test_open_folder_uses_selected_path_and_reports_errors(self):
        self.window.download_path = "folder with spaces"
        with patch("youtube_to_mp3.app.os.makedirs") as make_dirs:
            with patch("youtube_to_mp3.app.os.startfile") as open_folder:
                self.window.open_download_folder()
                make_dirs.assert_called_once_with("folder with spaces", exist_ok=True)
                open_folder.assert_called_once_with("folder with spaces")
        self.assertIn("Opened download folder", self.window.log_view.toPlainText())
        with patch("youtube_to_mp3.app.os.makedirs", side_effect=OSError("folder unavailable")):
            self.window.open_download_folder()
        self.assertIn("folder unavailable", self.window.log_view.toPlainText())

    def test_settings_and_tool_update_remain_connected(self):
        with patch("youtube_to_mp3.app.show_settings_dialog") as show_settings:
            self.window.open_settings()
        show_settings.assert_called_once_with(
            self.window, "a folder", self.window.choose_download_folder,
            self.window.start_tools_update, self.window.check_for_updates_on_launch,
        )
        self.window.start_tools_update()
        self.window.start_background_task.assert_called_once_with(
            self.window.download_manager.force_update_tools, "a folder"
        )

    def test_worker_error_is_logged_and_busy_state_resets(self):
        target = Mock(side_effect=RuntimeError("test failure"))
        app_module.YoutubeToMP3Window.start_background_task(self.window, target)
        self.window.worker_thread.join(timeout=5)
        self.assertFalse(self.window.worker_thread.is_alive())
        self.qt_app.processEvents()
        self.assertFalse(self.window.busy)
        self.assertTrue(self.window.download_button.isEnabled())
        self.assertIn("test failure", self.window.log_view.toPlainText())


if __name__ == "__main__":
    unittest.main()
