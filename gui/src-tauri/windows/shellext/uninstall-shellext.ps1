# uninstall-shellext.ps1 — removes the sparse package and the trusted cert.

param(
    [string]$InstallDir = ""
)

$ErrorActionPreference = "SilentlyContinue"

# Remove the registered AppX package
Get-AppxPackage HyperCompress.ShellExtension -AllUsers |
    ForEach-Object { Remove-AppxPackage -Package $_.PackageFullName -AllUsers }

# Remove the cert from both LocalMachine stores
foreach ($storeName in @("Root", "TrustedPeople")) {
    try {
        $store = New-Object System.Security.Cryptography.X509Certificates.X509Store -ArgumentList @($storeName, "LocalMachine")
        $store.Open("ReadWrite")
        $hcCerts = $store.Certificates | Where-Object { $_.Subject -eq "CN=HyperCompress" }
        foreach ($c in $hcCerts) { $store.Remove($c) }
        $store.Close()
    } catch {}
}

exit 0
