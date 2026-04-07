# Sign hc_shellext.dll and the sparse package using the dev PFX.
#
# This script:
#   1. Builds the sparse package layout in build/shellext/ from this directory
#      and the freshly built hc_shellext.dll.
#   2. Packs it into HCShellExt.msix using makeappx.exe.
#   3. Signs both the DLL and the .msix with HyperCompress.pfx using signtool.
#   4. Leaves HCShellExt.msix and HyperCompress.cer ready for the installer to ship.
#
# Required tools (from Windows 10/11 SDK, free):
#   makeappx.exe  - usually at C:\Program Files (x86)\Windows Kits\10\bin\<ver>\x64\makeappx.exe
#   signtool.exe  - same SDK location

[CmdletBinding()]
param(
    [string]$Password = "hypercompress-dev",
    [string]$RepoRoot = "",
    [string]$DllSource = ""
)

$ErrorActionPreference = "Stop"

$shellExtDir = if ([string]::IsNullOrEmpty($PSScriptRoot)) { (Resolve-Path ".").Path } else { $PSScriptRoot }
if ([string]::IsNullOrEmpty($RepoRoot)) {
    $RepoRoot = (Resolve-Path (Join-Path $shellExtDir "..\..\..\..")).Path
}
$pfxPath     = Join-Path $shellExtDir "HyperCompress.pfx"
$manifest    = Join-Path $shellExtDir "AppxManifest.xml"
$assetsDir   = Join-Path $shellExtDir "Assets"
$buildDir    = Join-Path $shellExtDir "build"
$layoutDir   = Join-Path $buildDir "layout"
$msixOut     = Join-Path $shellExtDir "HCShellExt.msix"

if (-not (Test-Path $pfxPath)) {
    throw "Cert not found at $pfxPath. Run generate-cert.ps1 first."
}

# Locate hc_shellext.dll
if ([string]::IsNullOrEmpty($DllSource)) {
    $DllSource = Join-Path $RepoRoot "target\release\hc_shellext.dll"
}
if (-not (Test-Path $DllSource)) {
    throw "hc_shellext.dll not found at $DllSource. Run: cargo build -p hc_shellext --release"
}

# Find SDK tools
function Find-Sdk-Tool($name) {
    $candidates = @(
        "C:\Program Files (x86)\Windows Kits\10\bin"
        "C:\Program Files\Windows Kits\10\bin"
    )
    foreach ($base in $candidates) {
        if (-not (Test-Path $base)) { continue }
        $hits = Get-ChildItem -Path $base -Recurse -Filter $name -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -like "*\x64\*" } |
                Sort-Object LastWriteTime -Descending
        if ($hits) { return $hits[0].FullName }
    }
    return $null
}

$makeappx = Find-Sdk-Tool "makeappx.exe"
$signtool = Find-Sdk-Tool "signtool.exe"
if (-not $makeappx) { throw "makeappx.exe not found. Install Windows 10/11 SDK." }
if (-not $signtool) { throw "signtool.exe not found. Install Windows 10/11 SDK." }

Write-Host "Using makeappx: $makeappx" -ForegroundColor DarkGray
Write-Host "Using signtool: $signtool" -ForegroundColor DarkGray

# 1. Build layout
if (Test-Path $buildDir) { Remove-Item -Recurse -Force $buildDir }
New-Item -ItemType Directory -Path $layoutDir | Out-Null
New-Item -ItemType Directory -Path (Join-Path $layoutDir "Assets") | Out-Null

Copy-Item $manifest    (Join-Path $layoutDir "AppxManifest.xml")
Copy-Item $DllSource   (Join-Path $layoutDir "hc_shellext.dll")
Copy-Item -Recurse "$assetsDir\*" (Join-Path $layoutDir "Assets")

# 2. Sign the DLL itself first
Write-Host "Signing hc_shellext.dll..." -ForegroundColor Cyan
& $signtool sign /fd SHA256 /f $pfxPath /p $Password (Join-Path $layoutDir "hc_shellext.dll") | Out-Null
if ($LASTEXITCODE -ne 0) { throw "DLL signing failed" }

# 3. Pack into MSIX
Write-Host "Packing $msixOut..." -ForegroundColor Cyan
if (Test-Path $msixOut) { Remove-Item $msixOut -Force }
& $makeappx pack /o /d "$layoutDir" /p "$msixOut" /nv
if ($LASTEXITCODE -ne 0) { throw "makeappx failed with exit code $LASTEXITCODE" }

# 4. Sign the MSIX
Write-Host "Signing $msixOut..." -ForegroundColor Cyan
& $signtool sign /fd SHA256 /f $pfxPath /p $Password $msixOut | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MSIX signing failed" }

# 5. Stage the DLL into the shellext folder so Tauri's resource bundler picks it up.
#    The installer will place it at $INSTDIR\hc_shellext.dll where the AppxManifest
#    expects it (relative to ExternalLocation = $INSTDIR).
Copy-Item -Force $DllSource (Join-Path $shellExtDir "hc_shellext.dll")

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host "  Signed package:   $msixOut"
Write-Host "  Public cert:      $shellExtDir\HyperCompress.cer"
Write-Host "  Staged DLL:       $shellExtDir\hc_shellext.dll"
Write-Host ""
Write-Host "Now run from the gui directory: npx tauri build" -ForegroundColor Yellow
