[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'This script builds the Windows application and must run on Windows.'
}

$projectDir = Split-Path -Parent $PSScriptRoot
$manifest = Join-Path $projectDir 'Cargo.toml'
$targetDir = Join-Path $PSScriptRoot 'target'
$binary = Join-Path $targetDir 'release\youtube-to-mp3.exe'

if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
    throw 'Rust project manifest not found.'
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'Cargo is required to build the application.'
}

if (Test-Path -LiteralPath $binary -PathType Leaf) {
    $binaryFullPath = [IO.Path]::GetFullPath($binary)
    $running = Get-CimInstance Win32_Process -Filter "Name='youtube-to-mp3.exe'" |
        Where-Object { $_.ExecutablePath -and [string]::Equals($_.ExecutablePath, $binaryFullPath, [StringComparison]::OrdinalIgnoreCase) }
    if ($running) {
        throw 'The build target is running. Close it before rebuilding.'
    }
}

$previousVersion = [Environment]::GetEnvironmentVariable('YTDLP_APP_VERSION', 'Process')
try {
    $env:YTDLP_APP_VERSION = $Version
    & cargo build --locked --release --manifest-path $manifest --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo failed with exit code $LASTEXITCODE."
    }
} finally {
    [Environment]::SetEnvironmentVariable('YTDLP_APP_VERSION', $previousVersion, 'Process')
}

if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
    throw 'Cargo did not produce the expected executable.'
}

$releaseRoot = Join-Path $PSScriptRoot 'releases'
New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null
$releaseDir = Join-Path $releaseRoot ($Version + '-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $releaseDir | Out-Null
$legacyAsset = Join-Path $releaseDir 'YouTube to MP3.exe'
$rustAsset = Join-Path $releaseDir 'YouTube.to.MP3.Rust.exe'
Copy-Item -LiteralPath $binary -Destination $legacyAsset
Copy-Item -LiteralPath $binary -Destination $rustAsset
$legacyHash = (Get-FileHash -LiteralPath $legacyAsset -Algorithm SHA256).Hash
$rustHash = (Get-FileHash -LiteralPath $rustAsset -Algorithm SHA256).Hash
if ($legacyHash -ne $rustHash) {
    throw 'The legacy and Rust updater assets do not match.'
}

Write-Output "Release artifacts: $releaseDir"
Write-Output "SHA-256: $rustHash"
