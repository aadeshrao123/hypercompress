# HyperCompress Shell Extension (Win11 Modern Context Menu)

This folder contains everything needed to ship HyperCompress as a top-level
Windows 11 right-click menu entry — the same approach used by PowerToys, NanaZip,
and modern 7-Zip / WinRAR.

## What's here

| File | Purpose |
|---|---|
| `AppxManifest.xml` | Sparse package manifest. Registers the COM CLSID and binds it to files, folders, and `.hc` archives. |
| `Assets/` | Square logos referenced by the manifest. |
| `generate-cert.ps1` | One-time: creates `HyperCompress.pfx` (private) and `HyperCompress.cer` (public) self-signed code-signing cert. |
| `sign-package.ps1` | Builds and signs `HCShellExt.msix`. Run after every code change to `hc_shellext`. |
| `README.md` | This file. |

After signing you also get:

| File | Purpose |
|---|---|
| `HyperCompress.cer` | Public cert. Shipped in installer; trusted at install time so Windows accepts the sparse package. |
| `HyperCompress.pfx` | Private cert. **NEVER commit this** — `.gitignore` excludes it. |
| `HCShellExt.msix` | Signed sparse package. Bundled by Tauri and registered by the installer hook. |
| `hc_shellext.dll` | Signed Rust COM DLL implementing `IExplorerCommand`. Bundled at install root. |

## Build process

### One-time setup

```powershell
# 1. Generate the dev signing cert (only needed once per machine)
cd gui\src-tauri\windows\shellext
.\generate-cert.ps1
```

### Each release build

```powershell
# 2. Build the COM DLL (Rust crate at hc_shellext/)
cd ..\..\..\..
cargo build -p hc_shellext --release

# 3. Sign the DLL and pack/sign the sparse package
cd gui\src-tauri\windows\shellext
.\sign-package.ps1

# 4. Build the Tauri installer (bundles everything from this folder)
cd ..\..\..
npm install
npx tauri build
```

The resulting installer is at `gui\src-tauri\target\release\bundle\nsis\HyperCompress_*.exe`.

## How it works at install time

1. NSIS installer drops these files into Program Files\HyperCompress\:
   - `HyperCompress.exe` (the GUI/CLI binary)
   - `hc_shellext.dll` (the COM DLL)
   - `Assets\*.png`
   - `shellext\HCShellExt.msix`
   - `shellext\HyperCompress.cer`
2. The installer hook (`installer-hooks.nsh`) runs:
   - `certutil -addstore "TrustedPeople" HyperCompress.cer` — trusts the dev cert
   - `Add-AppxPackage HCShellExt.msix -ExternalLocation $INSTDIR` — registers the sparse package
3. Windows immediately picks up the new context menu handler. (May need an Explorer restart on some systems.)

## How it works at runtime

When the user right-clicks a file or folder, Explorer:
1. Sees the modern Win11 context menu has a registered handler for this item type
2. Loads `hc_shellext.dll` out-of-process
3. Calls `IClassFactory::CreateInstance` → returns an `HCRootCommand` (`IExplorerCommand`)
4. Calls `GetTitle()` → "HyperCompress", `GetFlags()` → `ECF_HASSUBCOMMANDS`
5. Calls `EnumSubCommands()` → returns 4 child commands
6. For each child, `GetState()` decides whether to show or hide it based on the file extension
7. When the user clicks a child, `Invoke()` shells out to `HyperCompress.exe` with the right CLI args (`compress` / `decompress`) and the selected paths

## Subcommands

| Subcommand | Visible when | Action |
|---|---|---|
| Compress to .hc | not all selected items are `.hc` | `HyperCompress.exe compress <paths>` |
| Extract Here | any selected item is `.hc` | `HyperCompress.exe decompress <hc> <parent dir>` |
| Extract to "name\\" | any selected item is `.hc` | `HyperCompress.exe decompress <hc> <parent>\<stem>` |
| Open with HyperCompress | always | Launches GUI with the selected items |

## Going to production (real cert)

When ready to drop the SmartScreen warning, replace `generate-cert.ps1` with a
real code-signing cert from one of:

- **SignPath Foundation** — free for OSS, takes 1-3 weeks to apply. https://signpath.org/apply
- **Azure Trusted Signing** — ~$10/month, no USB token needed
- **DigiCert / Sectigo** — $100-400/yr, USB token required since June 2023

The sign-package.ps1 script doesn't need to change — just point it at the new PFX
or use a cloud-signing alternative that exposes a `signtool` driver.
