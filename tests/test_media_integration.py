import functools
import http.server
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import Mock, patch

from youtube_to_mp3.downloaders import DownloadManager


FFMPEG = shutil.which("ffmpeg")
FFPROBE = shutil.which("ffprobe")
YTDLP = shutil.which("yt-dlp")


@unittest.skipUnless(
    os.name == "nt" and FFMPEG and FFPROBE and YTDLP,
    "Requires Windows and local yt-dlp, ffmpeg, and ffprobe executables",
)
class LocalMediaIntegrationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        encoders = subprocess.run(
            [FFMPEG, "-hide_banner", "-encoders"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        if "libx264" not in encoders:
            raise unittest.SkipTest("FFmpeg needs libx264 to generate the local media fixture")

        cls.temp_dir = tempfile.TemporaryDirectory(prefix="yt-downloader-tests-")
        cls.addClassCleanup(cls.temp_dir.cleanup)
        root = Path(cls.temp_dir.name)
        source = root / "source"
        source.mkdir()

        for name, size in (("low", "160x90"), ("uhd", "3840x2160")):
            subprocess.run(
                [
                    FFMPEG, "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                    f"color=c=blue:s={size}:r=1", "-t", "1", "-an", "-c:v",
                    "libx264", "-threads", "2", "-preset", "ultrafast", "-pix_fmt",
                    "yuv420p", "-f", "mpegts", str(source / f"{name}.ts"),
                ],
                check=True,
            )
        subprocess.run(
            [
                FFMPEG, "-loglevel", "error", "-y", "-f", "lavfi", "-i",
                "sine=frequency=440:sample_rate=44100", "-t", "1", "-vn",
                "-c:a", "aac", "-f", "mpegts", str(source / "audio.ts"),
            ],
            check=True,
        )
        for name in ("low", "uhd", "audio"):
            (source / f"{name}.m3u8").write_text(
                "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:1\n"
                f"#EXT-X-MEDIA-SEQUENCE:0\n#EXTINF:1.0,\n{name}.ts\n#EXT-X-ENDLIST\n",
                encoding="utf-8",
            )
        (source / "master.m3u8").write_text(
            "#EXTM3U\n#EXT-X-VERSION:3\n"
            '#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",'
            'DEFAULT=YES,AUTOSELECT=YES,URI="audio.m3u8"\n'
            '#EXT-X-STREAM-INF:BANDWIDTH=200000,RESOLUTION=160x90,AUDIO="audio",'
            'CODECS="avc1.42c00b,mp4a.40.2"\nlow.m3u8\n'
            '#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=3840x2160,AUDIO="audio",'
            'CODECS="avc1.42c033,mp4a.40.2"\nuhd.m3u8\n',
            encoding="utf-8",
        )
        (source / "silent.m3u8").write_text(
            "#EXTM3U\n#EXT-X-VERSION:3\n"
            '#EXT-X-STREAM-INF:BANDWIDTH=2000000,RESOLUTION=3840x2160,'
            'CODECS="avc1.42c033"\nuhd.m3u8\n',
            encoding="utf-8",
        )

        class QuietHandler(http.server.SimpleHTTPRequestHandler):
            def log_message(self, *args):
                pass

        cls.server = http.server.ThreadingHTTPServer(
            ("127.0.0.1", 0), functools.partial(QuietHandler, directory=str(source))
        )
        cls.addClassCleanup(cls.server.server_close)
        cls.addClassCleanup(cls.server.shutdown)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        cls.url = f"http://127.0.0.1:{cls.server.server_port}/master.m3u8"

    def setUp(self):
        self.target = Path(tempfile.mkdtemp(prefix="output-", dir=self.temp_dir.name))
        shutil.copy2(YTDLP, self.target / "yt-dlp.exe")
        self.messages = []
        self.manager = DownloadManager(
            lambda message, level="info": self.messages.append((message, level))
        )

    def download(self, output_format, height, url=None):
        self.manager.ensure_ytdlp_and_download(
            url or self.url, str(self.target), True, output_format, height
        )

    def probe_output(self, extension):
        files = list(self.target.glob(f"*.{extension}"))
        self.assertEqual(len(files), 1, self.messages[-10:])
        output = subprocess.check_output(
            [
                FFPROBE, "-v", "error", "-show_entries", "stream=codec_type,width,height:format=format_name",
                "-of", "json", str(files[0]),
            ],
            text=True,
        )
        self.assertIn(("Download complete.", "success"), self.messages)
        return json.loads(output)

    def assert_video_output(self, expected_height):
        data = self.probe_output("mp4")
        videos = [stream for stream in data["streams"] if stream["codec_type"] == "video"]
        audios = [stream for stream in data["streams"] if stream["codec_type"] == "audio"]
        self.assertEqual(len(videos), 1)
        self.assertEqual(videos[0]["height"], expected_height)
        self.assertTrue(audios)
        self.assertIn("mp4", data["format"]["format_name"])
        self.assertTrue(any("[Merger]" in message for message, _ in self.messages))

    def test_4k_merges_separate_video_and_audio(self):
        self.download("mp4", 2160)
        self.assert_video_output(2160)

    def test_resolution_cap_falls_back_to_a_lower_source(self):
        self.download("mp4", 720)
        self.assert_video_output(90)

    def test_best_available_picks_the_highest_source(self):
        self.download("mp4", None)
        self.assert_video_output(2160)

    def test_mp3_output_contains_audio(self):
        self.download("mp3", None)
        data = self.probe_output("mp3")
        self.assertEqual(data["format"]["format_name"], "mp3")
        self.assertTrue(any(stream["codec_type"] == "audio" for stream in data["streams"]))
        self.assertFalse(any(stream["codec_type"] == "video" for stream in data["streams"]))

    def test_silent_video_is_downloaded_without_an_audio_track(self):
        self.download("mp4", 2160, self.url.replace("master.m3u8", "silent.m3u8"))
        data = self.probe_output("mp4")
        videos = [stream for stream in data["streams"] if stream["codec_type"] == "video"]
        self.assertEqual(videos[0]["height"], 2160)
        self.assertFalse(any(stream["codec_type"] == "audio" for stream in data["streams"]))

    def test_format_selector_prefers_m4a_audio_and_falls_back_when_unavailable(self):
        process = Mock(stdout=io.StringIO(""), returncode=0)
        with patch("youtube_to_mp3.downloaders.subprocess.Popen", return_value=process) as popen:
            self.manager.download_media(self.url, str(self.target), True, "mp4", 2160)
        command = popen.call_args.args[0]
        selector = command[command.index("-f") + 1]
        video = {
            "format_id": "video", "url": "https://example.invalid/video.mp4",
            "ext": "mp4", "vcodec": "av01", "acodec": "none", "width": 3840,
            "height": 2160,
        }
        aac = {
            "format_id": "aac", "url": "https://example.invalid/audio.m4a",
            "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128,
        }
        opus = {
            "format_id": "opus", "url": "https://example.invalid/audio.webm",
            "ext": "webm", "vcodec": "none", "acodec": "opus", "abr": 160,
        }
        for formats, expected in (([video, aac, opus], "mp4a.40.2"), ([video, opus], "opus")):
            with self.subTest(expected=expected):
                info_path = self.target / "formats.info.json"
                info_path.write_text(
                    json.dumps({"id": "local-test", "title": "local-test", "formats": formats}),
                    encoding="utf-8",
                )
                selected = subprocess.check_output(
                    [
                        YTDLP, "--ignore-config", "--skip-download", "--load-info-json",
                        str(info_path), "-f", selector, "--print", "%(requested_formats.1.acodec)s",
                    ],
                    text=True,
                    stderr=subprocess.PIPE,
                )
                self.assertEqual(selected.strip(), expected)

    def test_unavailable_resolution_reports_failure(self):
        self.download("mp4", 60)
        self.assertFalse(list(self.target.glob("*.mp4")))
        self.assertTrue(any("Download failed" in message and level == "error" for message, level in self.messages))
        self.assertNotIn(("Download complete.", "success"), self.messages)


if __name__ == "__main__":
    unittest.main()
