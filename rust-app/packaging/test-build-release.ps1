Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Exercise the packaging script with a fake Cargo in an isolated, ignored fixture.
$fixtureRoot = Join-Path $PSScriptRoot ('target\packaging-test-' + [guid]::NewGuid().ToString('N'))
$fixtureProject = Join-Path $fixtureRoot 'project'
$fixturePackaging = Join-Path $fixtureProject 'packaging'
New-Item -ItemType Directory -Path $fixturePackaging -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'build-release.ps1') -Destination $fixturePackaging
Set-Content -LiteralPath (Join-Path $fixtureProject 'Cargo.toml') -Value '[package]' -Encoding UTF8

function cargo {
    if ($env:YTDLP_APP_VERSION -ne '0.1.6') {
        throw 'The release version was not supplied to Cargo.'
    }
    $index = [Array]::IndexOf($args, '--target-dir')
    if ($index -lt 0) { throw 'Cargo target directory was not supplied.' }
    $release = Join-Path $args[$index + 1] 'release'
    New-Item -ItemType Directory -Path $release -Force | Out-Null
    [IO.File]::WriteAllBytes((Join-Path $release 'youtube-to-mp3.exe'), [byte[]](77, 90, 1, 2, 3))
    $global:LASTEXITCODE = 0
}

$env:YTDLP_APP_VERSION = 'previous-value'
$script = Join-Path $fixturePackaging 'build-release.ps1'
& $script -Version '0.1.6' | Out-Null
if ($env:YTDLP_APP_VERSION -ne 'previous-value') {
    throw 'The existing process version was not restored.'
}
$releaseDirs = @(Get-ChildItem -LiteralPath (Join-Path $fixturePackaging 'releases') -Directory)
if ($releaseDirs.Count -ne 1) { throw 'Expected exactly one isolated release folder.' }
$legacy = Join-Path $releaseDirs[0].FullName 'YouTube to MP3.exe'
$rust = Join-Path $releaseDirs[0].FullName 'YouTube.to.MP3.Rust.exe'
if (-not (Test-Path -LiteralPath $legacy -PathType Leaf) -or -not (Test-Path -LiteralPath $rust -PathType Leaf)) {
    throw 'Both updater assets were not produced with the required names.'
}
if ((Get-FileHash -LiteralPath $legacy).Hash -ne (Get-FileHash -LiteralPath $rust).Hash) {
    throw 'Updater assets must contain identical bytes.'
}

Write-Output 'Release packaging smoke test passed.'
