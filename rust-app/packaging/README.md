# Rust release packaging

The Windows release workflow builds the Rust app for the normal installed app and keeps the existing installer identity and install location. It stages two byte-identical portable assets:

- `YouTube to MP3.exe`: recognized by the Python updater and used by the existing installer.
- `YouTube.to.MP3.Rust.exe`: recognized by the Rust updater and unchanged by GitHub's filename normalization.

The first asset upgrades existing Python installations to Rust when that release is published. Publishing either asset is a separate release action; the local build script does not commit, push, publish, or install anything.

For a local build, run from PowerShell on Windows with Rust and the MSVC build tools installed:

```powershell
.\rust-app\packaging\build-release.ps1 -Version 0.1.6
```

Pass the intended release tag as `-Version` without a `v` prefix. The script supplies it as `YTDLP_APP_VERSION` during compilation, checks the hashes match, and writes both executables to a new unique folder under `packaging\releases`. The version must match the release tag for update comparisons to work.

The production workflow uses `installer\YoutubeToMP3.iss` to build the installer and preserve the existing application identity and path. The local script builds only the portable assets and does not run or install anything.
