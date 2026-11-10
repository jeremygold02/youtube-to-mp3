"""Exercise the shipped Python updater against the Rust release assets, without a GUI."""

from __future__ import annotations

import argparse
import hashlib
import importlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
from unittest import mock


REPO = Path(__file__).resolve().parents[2]
LEGACY_COMMIT = "0d196b7b54e58d07683bfd47b684eb6cf2060f0d"
LEGACY_ASSET = "YouTube to MP3.exe"
RUST_ASSET = "YouTube.to.MP3.Rust.exe"
NO_WINDOW = getattr(subprocess, "CREATE_NO_WINDOW", 0)


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def load_legacy_updater(directory: Path):
    package = directory / "legacy_updater_fixture"
    package.mkdir()
    (package / "__init__.py").write_text("", encoding="utf-8")
    for filename in ("version.py", "updates.py"):
        contents = subprocess.run(
            ["git", "show", f"{LEGACY_COMMIT}:youtube_to_mp3/{filename}"],
            cwd=REPO,
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=NO_WINDOW,
        ).stdout
        (package / filename).write_bytes(contents)
    sys.path.insert(0, str(directory))
    try:
        return importlib.import_module("legacy_updater_fixture.updates")
    finally:
        sys.path.remove(str(directory))


def run_test(legacy_asset: Path, rust_asset: Path, version: str) -> None:
    if os.name != "nt":
        raise RuntimeError("The legacy upgrade integration test requires Windows.")
    if legacy_asset.name != LEGACY_ASSET or rust_asset.name != RUST_ASSET:
        raise AssertionError("Both release assets must use the exact updater filenames.")
    expected_hash = sha256(rust_asset)
    if expected_hash != sha256(legacy_asset):
        raise AssertionError("Python and Rust updater assets contain different executables.")

    with tempfile.TemporaryDirectory(prefix="ytmp3-upgrade-test-") as directory:
        root = Path(directory)
        updater = load_legacy_updater(root)
        if updater.version_parts(version) <= updater.version_parts("0.1.5"):
            raise AssertionError("The release tag must be newer than the installed Python release.")

        release = {
            "tag_name": version,
            "html_url": f"https://github.com/jeremygold02/youtube-to-mp3/releases/tag/{version}",
            "assets": [
                {"name": RUST_ASSET, "browser_download_url": "https://example.invalid/rust.exe"},
                {"name": "YouTube.to.MP3.exe", "browser_download_url": "https://example.invalid/legacy.exe"},
            ],
        }
        install_dir = root / "installed"
        install_dir.mkdir()
        target = install_dir / LEGACY_ASSET
        target.write_bytes(b"old Python executable placeholder")
        update_dir = root / "update"
        update_dir.mkdir()

        def download(url: str, destination: Path) -> None:
            if url != "https://example.invalid/legacy.exe":
                raise AssertionError(f"Legacy updater chose the wrong asset: {url}")
            shutil.copyfile(legacy_asset, destination)

        with (
            mock.patch.object(updater, "APP_VERSION", "0.1.5"),
            mock.patch.object(updater, "latest_release", return_value=release),
            mock.patch.object(updater, "can_install_updates", return_value=True),
            mock.patch.object(updater, "download_file", side_effect=download),
            mock.patch.object(updater.sys, "executable", str(target)),
            mock.patch.object(updater.os, "getpid", return_value=2_147_483_647),
            mock.patch.object(updater.tempfile, "mkdtemp", return_value=str(update_dir)),
            mock.patch.object(updater.subprocess, "Popen") as launch,
            mock.patch.object(updater.threading, "Timer") as timer,
        ):
            result = updater.install_update()
            command = launch.call_args.args[0]
            timer.return_value.start.assert_called_once()
            script = Path(command[command.index("-File") + 1])
            if result["latest_version"] != version or not script.is_file():
                raise AssertionError("The Python updater did not prepare the expected update.")

        # Execute the real generated helper while replacing Start-Process inside this
        # child PowerShell only. The test never opens the graphical application.
        capture = root / "restart-target.txt"
        environment = os.environ.copy()
        environment["YTMP3_TEST_SCRIPT"] = str(script)
        environment["YTMP3_TEST_RESTART"] = str(capture)
        helper = (
            "function global:Start-Process { "
            "param([string]$FilePath, [string]$WorkingDirectory) "
            "[System.IO.File]::WriteAllText($env:YTMP3_TEST_RESTART, $FilePath) }\n"
            "& $env:YTMP3_TEST_SCRIPT"
        )
        completed = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", helper],
            env=environment,
            capture_output=True,
            text=True,
            timeout=35,
            creationflags=NO_WINDOW,
        )
        if completed.returncode:
            raise AssertionError(f"Legacy updater helper failed: {completed.stdout}\n{completed.stderr}")
        if sha256(target) != expected_hash:
            raise AssertionError("The old executable was not replaced with the Rust binary.")
        restart_target = Path(capture.read_text(encoding="utf-8-sig"))
        try:
            same_target = os.path.samefile(restart_target, target)
        except OSError as error:
            raise AssertionError(
                f"Could not compare restart target {restart_target!s} with {target!s}: {error}"
            ) from error
        if not same_target:
            raise AssertionError(
                f"The updater requested restart of {restart_target!s}, expected {target!s}."
            )

        # --version is a headless path and must not create a window or read settings.
        actual = subprocess.run(
            [str(target), "--version"],
            check=True,
            capture_output=True,
            text=True,
            timeout=10,
            creationflags=NO_WINDOW,
        ).stdout.strip()
        if actual != version:
            raise AssertionError(f"Migrated executable reports {actual!r}, expected {version!r}.")

    print(f"Legacy Python updater selected and replaced Rust {version}, and requested its restart (headless).")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("legacy_asset", type=Path)
    parser.add_argument("rust_asset", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    run_test(args.legacy_asset.resolve(), args.rust_asset.resolve(), args.version)
