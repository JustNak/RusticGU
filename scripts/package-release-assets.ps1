# Build the GitHub Release asset set under dist-release/.
#
# Prerequisites (from repo root):
#   - target/release/rusticgu.exe, rusticgu-updater.exe
#   - cargo-packager 0.11.x on PATH
#
# Usage:
#   powershell -ExecutionPolicy Bypass -File scripts/package-release-assets.ps1
#
# Output:
#   dist-release/RusticGU-windows-x64-setup.exe
#   dist-release/RusticGU-windows-x64.zip

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$packageWindows = Join-Path $PSScriptRoot "package-windows.ps1"
if (-not (Test-Path $packageWindows)) {
    throw "Missing $packageWindows"
}

Write-Host "Packaging NSIS installer..."
& $packageWindows -SkipBuild
if ($LASTEXITCODE -ne 0) {
    throw "package-windows.ps1 failed with exit code $LASTEXITCODE"
}

$dist = Join-Path $repoRoot "dist-release"
$setup = Join-Path $dist "RusticGU-windows-x64-setup.exe"
if (-not (Test-Path $setup)) {
    throw "NSIS setup.exe not found at $setup"
}
Write-Host "Packed RusticGU-windows-x64-setup.exe ($([math]::Round((Get-Item $setup).Length / 1MB, 2)) MB)"

function Assert-File([string]$Path) {
    if (-not (Test-Path $Path)) {
        throw "Required file missing: $Path"
    }
}

Assert-File "target/release/rusticgu.exe"
Assert-File "target/release/rusticgu-updater.exe"

$appDir = Join-Path $dist "app"
if (Test-Path $appDir) {
    Remove-Item -Recurse -Force $appDir
}
New-Item -ItemType Directory -Force -Path $appDir | Out-Null
Copy-Item "target/release/rusticgu.exe" $appDir
Copy-Item "target/release/rusticgu-updater.exe" $appDir
Copy-Item "LICENSE" $appDir
Copy-Item "README.md" $appDir
if (Test-Path "assets") {
    Copy-Item "assets" $appDir -Recurse
}
Compress-Archive -Path (Join-Path $appDir "*") -DestinationPath (Join-Path $dist "RusticGU-windows-x64.zip") -Force

Get-ChildItem $dist -Filter *.zip | ForEach-Object {
    Write-Host "Packed $($_.Name) ($([math]::Round($_.Length / 1MB, 2)) MB)"
}
