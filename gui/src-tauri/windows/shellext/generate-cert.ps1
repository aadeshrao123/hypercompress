# HyperCompress shell-extension signing cert generator.
#
# Run this ONCE on your dev machine. It creates a self-signed code-signing cert
# whose CN matches the AppxManifest's <Identity Publisher>, exports it as a PFX
# (private key, used to sign the package) and a CER (public key, the installer
# adds it to TrustedPeople so Windows accepts the sparse package).
#
# Output (in this directory):
#   HyperCompress.pfx  - private cert, password-protected. NEVER commit this.
#   HyperCompress.cer  - public cert, safe to commit and ship inside the installer.
#   .gitignore         - excludes the PFX from git
#
# To re-sign after a code change, run sign-package.ps1 (uses the existing PFX).

[CmdletBinding()]
param(
    [string]$Subject = "CN=HyperCompress",
    [string]$Password = "hypercompress-dev",
    [string]$OutDir  = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrEmpty($OutDir)) {
    if (-not [string]::IsNullOrEmpty($PSScriptRoot)) {
        $OutDir = $PSScriptRoot
    } else {
        $OutDir = (Resolve-Path ".").Path
    }
}

Write-Host "Generating self-signed code-signing cert: $Subject" -ForegroundColor Cyan

$cert = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -KeyAlgorithm RSA `
    -KeyLength 2048 `
    -KeyExportPolicy Exportable `
    -KeyUsage DigitalSignature `
    -NotAfter (Get-Date).AddYears(10) `
    -CertStoreLocation "Cert:\CurrentUser\My"

$pfxPath = Join-Path $OutDir "HyperCompress.pfx"
$cerPath = Join-Path $OutDir "HyperCompress.cer"

$pwd = ConvertTo-SecureString -String $Password -Force -AsPlainText
Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $pwd | Out-Null
Export-Certificate    -Cert $cert -FilePath $cerPath -Type CERT  | Out-Null

# Remove from CurrentUser store after exporting (we only need the files now)
Remove-Item -Path "Cert:\CurrentUser\My\$($cert.Thumbprint)" -Force

# Make sure .pfx is gitignored
$gi = Join-Path $OutDir ".gitignore"
$rules = @(
    "HyperCompress.pfx"
    "*.pfx"
)
Set-Content -Path $gi -Value $rules

Write-Host ""
Write-Host "Done." -ForegroundColor Green
Write-Host "  Private (sign with): $pfxPath"
Write-Host "  Public  (ship in installer): $cerPath"
Write-Host "  Password: $Password"
Write-Host ""
Write-Host "Next: run sign-package.ps1 to sign hc_shellext.dll and the AppxManifest." -ForegroundColor Yellow
