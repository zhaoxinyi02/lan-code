param(
    [switch]$SkipDesktopBuild
)

$ErrorActionPreference = "Stop"
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$desktop = Join-Path $root "apps\desktop"
$installerManifest = Join-Path $root "apps\installer\src-tauri\Cargo.toml"
$targetDir = Join-Path $root "target\installer"
$outputDir = Join-Path $root "dist"

if (-not $SkipDesktopBuild) {
    Push-Location $desktop
    try {
        npm run build
        if ($LASTEXITCODE -ne 0) {
            throw "Desktop frontend build failed with exit code $LASTEXITCODE"
        }
        npm run tauri -- build --no-bundle
        if ($LASTEXITCODE -ne 0) {
            throw "Desktop Tauri production build failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

$desktopExe = Join-Path $root "target\release\lan-desktop.exe"
if (-not (Test-Path -LiteralPath $desktopExe)) {
    throw "Desktop executable not found: $desktopExe"
}

Push-Location $desktop
try {
    npm run build:installer
    if ($LASTEXITCODE -ne 0) {
        throw "Installer frontend build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

$installerIndex = Join-Path $desktop "installer-dist\index.html"
if (-not (Test-Path -LiteralPath $installerIndex)) {
    throw "Installer frontend is invalid: index.html was not generated at $installerIndex"
}

$previousTarget = $env:CARGO_TARGET_DIR
$env:CARGO_TARGET_DIR = $targetDir
try {
    cargo build --release --manifest-path $installerManifest
    if ($LASTEXITCODE -ne 0) {
        throw "Installer Rust build failed with exit code $LASTEXITCODE"
    }
}
finally {
    $env:CARGO_TARGET_DIR = $previousTarget
}

New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
$source = Join-Path $targetDir "release\lan-code-installer.exe"
$destination = Join-Path $outputDir "Lan Code_0.2.9_x64-setup.exe"
Copy-Item -LiteralPath $source -Destination $destination -Force

$hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
"$hash  $([IO.Path]::GetFileName($destination))" | Set-Content -Encoding ascii -LiteralPath "$destination.sha256"

Write-Host "Branded installer: $destination"
Write-Host "SHA-256: $hash"
