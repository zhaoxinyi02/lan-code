$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$repo = Split-Path -Parent $PSScriptRoot
$dist = Join-Path $repo "dist"
$packageName = "lan-code-0.2.0-windows-x64"
$package = Join-Path $dist $packageName
$zip = Join-Path $dist "$packageName.zip"

cargo build --release --workspace --manifest-path (Join-Path $repo "Cargo.toml")
$desktop = Join-Path $repo "apps\desktop"
Push-Location $desktop
try {
    npm ci
    npm run tauri build -- --no-bundle
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null
if (Test-Path $package) {
    $resolvedDist = [IO.Path]::GetFullPath($dist) + [IO.Path]::DirectorySeparatorChar
    $resolvedPackage = [IO.Path]::GetFullPath($package)
    if (-not $resolvedPackage.StartsWith($resolvedDist)) {
        throw "Refusing to remove package path outside dist"
    }
    Remove-Item -Recurse -Force -LiteralPath $resolvedPackage
}
New-Item -ItemType Directory -Path $package | Out-Null

Copy-Item (Join-Path $repo "target\release\lan-cli.exe") $package
Copy-Item (Join-Path $repo "target\release\lan-daemon.exe") $package
Copy-Item (Join-Path $repo "target\release\lan-desktop.exe") $package
Copy-Item (Join-Path $repo "lan.example.toml") $package
Copy-Item (Join-Path $repo "README.md") $package
Compress-Archive -Force -Path (Join-Path $package "*") -DestinationPath $zip

Write-Output $zip
