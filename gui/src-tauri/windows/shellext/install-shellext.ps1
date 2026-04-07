# install-shellext.ps1 — invoked by the NSIS installer to set up the modern
# Win11 context menu. Run elevated (perMachine install gives us admin).
#
# Steps:
#   1. Import the self-signed cert into LocalMachine\Root and LocalMachine\TrustedPeople
#      using the .NET X509Store API directly (avoids any UAC/security prompts that
#      the Import-Certificate cmdlet would trigger for Root store writes).
#   2. Register the sparse package via Add-AppxPackage with -ExternalLocation.
#
# All output goes to a log file at $InstallDir\shellext\install.log so the
# installer can show it on failure.

param(
    [Parameter(Mandatory)][string]$InstallDir
)

$ErrorActionPreference = "Continue"
$logPath = Join-Path $InstallDir "shellext\install.log"

function Log {
    param([string]$Msg)
    $line = "[$(Get-Date -Format 'HH:mm:ss')] $Msg"
    Write-Host $line
    Add-Content -Path $logPath -Value $line -ErrorAction SilentlyContinue
}

function Add-CertToStore {
    param([string]$CerPath, [string]$StoreName, [string]$Location)
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 -ArgumentList @($CerPath)
    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store -ArgumentList @($StoreName, $Location)
    $store.Open("ReadWrite")
    # Avoid duplicate inserts on reinstall
    $existing = $store.Certificates | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
    if (-not $existing) {
        $store.Add($cert)
    }
    $store.Close()
}

# Reset log
"" | Out-File -FilePath $logPath -Force

Log "Install dir: $InstallDir"

$msix = Join-Path $InstallDir "shellext\HCShellExt.msix"
$cer  = Join-Path $InstallDir "shellext\HyperCompress.cer"

if (-not (Test-Path $msix)) { Log "ERROR: $msix not found"; exit 1 }
if (-not (Test-Path $cer))  { Log "ERROR: $cer not found"; exit 1 }

# Step 1: install cert into LocalMachine cert stores
Log "Installing cert to LocalMachine\Root..."
try {
    Add-CertToStore -CerPath $cer -StoreName "Root" -Location "LocalMachine"
    Log "  ok"
} catch {
    Log "  FAIL: $($_.Exception.Message)"
}

Log "Installing cert to LocalMachine\TrustedPeople..."
try {
    Add-CertToStore -CerPath $cer -StoreName "TrustedPeople" -Location "LocalMachine"
    Log "  ok"
} catch {
    Log "  FAIL: $($_.Exception.Message)"
}

# Step 2: clean up any old package, register the new one
Log "Removing any pre-existing HyperCompress.ShellExtension package..."
Get-AppxPackage HyperCompress.ShellExtension -AllUsers -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-AppxPackage -Package $_.PackageFullName -AllUsers -ErrorAction SilentlyContinue }

Log "Registering sparse package..."
try {
    Add-AppxPackage -Path $msix -ExternalLocation $InstallDir -ForceApplicationShutdown -ErrorAction Stop
    Log "  ok"
} catch {
    Log "  FAIL: $($_.Exception.Message)"
    Log "  HRESULT: 0x$('{0:X8}' -f $_.Exception.HResult)"
    exit 2
}

Log "Final state:"
Get-AppxPackage HyperCompress.ShellExtension | ForEach-Object {
    Log "  $($_.PackageFullName) - $($_.Status)"
}

Log "Done."
exit 0
