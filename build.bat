@echo off
setlocal EnableExtensions EnableDelayedExpansion

set "PROJECT_ROOT=%~dp0"
if "%PROJECT_ROOT:~-1%"=="\" set "PROJECT_ROOT=%PROJECT_ROOT:~0,-1%"
set "VENV_DIR=%PROJECT_ROOT%\.build_venv"
set "BUILD_DIR=%PROJECT_ROOT%\build"
set "DIST_DIR=%PROJECT_ROOT%\dist"
set "SPEC_PATH=%PROJECT_ROOT%\YouTube to MP3.spec"
set "ROOT_EXE=%PROJECT_ROOT%\YouTube to MP3.exe"
set "BUILT_EXE=%DIST_DIR%\YouTube to MP3.exe"
set "VERSION_BUILD_PATH=%PROJECT_ROOT%\youtube_to_mp3\_version_build.py"
set "APP_VERSION=0.1.0"
set "BUILD_STATUS=0"

pushd "%PROJECT_ROOT%" || exit /b 1

if not exist "%PROJECT_ROOT%\app.py" (
    echo ERROR: "%PROJECT_ROOT%\app.py" not found.
    set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" if not exist "%PROJECT_ROOT%\icon.ico" (
    echo ERROR: "%PROJECT_ROOT%\icon.ico" not found.
    set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" if not exist "%PROJECT_ROOT%\requirements.txt" (
    echo ERROR: "%PROJECT_ROOT%\requirements.txt" not found.
    set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" if exist "%ROOT_EXE%" (
    powershell -NoProfile -ExecutionPolicy Bypass -Command "$target = [System.IO.Path]::GetFullPath('%ROOT_EXE%'); $running = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -and ([System.IO.Path]::GetFullPath($_.ExecutablePath) -ieq $target) }); if ($running.Count -gt 0) { Write-Host ('ERROR: Close the running app before rebuilding: ' + $target); exit 1 }"
    if errorlevel 1 set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    if exist "%VENV_DIR%\" rd /S /Q "%VENV_DIR%" || set "BUILD_STATUS=1"
    if exist "%BUILD_DIR%\" rd /S /Q "%BUILD_DIR%" || set "BUILD_STATUS=1"
    if exist "%DIST_DIR%\" rd /S /Q "%DIST_DIR%" || set "BUILD_STATUS=1"
    if exist "%SPEC_PATH%" del /F /Q "%SPEC_PATH%" || set "BUILD_STATUS=1"
    if exist "%VERSION_BUILD_PATH%" del /F /Q "%VERSION_BUILD_PATH%" || set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    echo Creating temporary virtual environment...
    python -m venv "%VENV_DIR%" || set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" if not exist "%VENV_DIR%\Scripts\python.exe" (
    echo Virtual environment Python was not created at "%VENV_DIR%\Scripts\python.exe"
    set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    echo Installing build dependencies...
    "%VENV_DIR%\Scripts\python.exe" -m pip install --upgrade pip || set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    "%VENV_DIR%\Scripts\python.exe" -m pip install -r "%PROJECT_ROOT%\requirements.txt" pyinstaller || set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    where git >nul 2>&1
    if "!ERRORLEVEL!"=="0" (
        git fetch --tags --quiet >nul 2>&1
        for /f "usebackq delims=" %%V in (`powershell -NoProfile -ExecutionPolicy Bypass -Command "$latest = '0.1.0'; $maxPatch = -1; foreach ($tag in git tag --list '0.1.*') { if ($tag -match '^0\.1\.(\d+)$') { $patch = [int]$Matches[1]; if ($patch -gt $maxPatch) { $maxPatch = $patch; $latest = $tag } } }; $latest"`) do (
            set "APP_VERSION=%%V"
        )
    )

    echo Building version !APP_VERSION!...
    > "%VERSION_BUILD_PATH%" echo APP_VERSION = "!APP_VERSION!"
)

if "!BUILD_STATUS!"=="0" (
    echo Building executable...
    "%VENV_DIR%\Scripts\python.exe" -m PyInstaller ^
        --noconfirm ^
        --clean ^
        --onefile ^
        --windowed ^
        --icon "icon.ico" ^
        --name "YouTube to MP3" ^
        --add-data "icon.ico;." ^
        "app.py" || set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" if not exist "%BUILT_EXE%" (
    echo PyInstaller did not create "%BUILT_EXE%"
    set "BUILD_STATUS=1"
)

if "!BUILD_STATUS!"=="0" (
    move /Y "%BUILT_EXE%" "%ROOT_EXE%" >nul || set "BUILD_STATUS=1"
)

echo Cleaning temporary build files...
if exist "%VENV_DIR%\" rd /S /Q "%VENV_DIR%"
if exist "%BUILD_DIR%\" rd /S /Q "%BUILD_DIR%"
if exist "%DIST_DIR%\" rd /S /Q "%DIST_DIR%"
if exist "%SPEC_PATH%" del /F /Q "%SPEC_PATH%"
if exist "%VERSION_BUILD_PATH%" del /F /Q "%VERSION_BUILD_PATH%"
popd

if "%BUILD_STATUS%"=="0" (
    echo Executable created: "%ROOT_EXE%"
    echo Done.
) else (
    echo Build failed.
)

exit /b %BUILD_STATUS%
