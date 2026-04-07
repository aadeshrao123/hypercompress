//! IExplorerCommand implementations for HyperCompress.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::core::{implement, Result, GUID, PWSTR};
use windows::Win32::Foundation::{BOOL, E_NOTIMPL, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IBindCtx;
use windows::Win32::UI::Shell::{
    IEnumExplorerCommand, IEnumExplorerCommand_Impl, IExplorerCommand, IExplorerCommand_Impl,
    IShellItemArray, ECF_HASSUBCOMMANDS, ECS_ENABLED, ECS_HIDDEN,
};

use crate::util::{
    all_have_ext, alloc_pwstr, any_has_ext, collect_paths, find_hypercompress_exe,
    spawn_hypercompress,
};

const HC_EXT: &str = "hc";

fn paths_from(items: Option<&IShellItemArray>) -> Vec<PathBuf> {
    collect_paths(items)
}

/// Return an icon path string of the form `<gui.exe>,0` so Explorer pulls
/// the embedded icon resource. Falls back to empty if the binary isn't found.
fn icon_path() -> String {
    match find_hypercompress_exe() {
        Some(p) => format!("{},0", p.to_string_lossy()),
        None => String::new(),
    }
}

// ----- Root command --------------------------------------------------------

#[implement(IExplorerCommand)]
#[derive(Default)]
pub struct HCRootCommand;

impl IExplorerCommand_Impl for HCRootCommand_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr("HyperCompress")
    }
    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr(&icon_path())
    }
    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567891))
    }
    fn GetState(&self, _items: Option<&IShellItemArray>, _slow: BOOL) -> Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }
    fn Invoke(&self, _items: Option<&IShellItemArray>, _ctx: Option<&IBindCtx>) -> Result<()> {
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> {
        Ok(ECF_HASSUBCOMMANDS.0 as u32)
    }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> {
        let children: Vec<IExplorerCommand> = vec![
            HCCompressCmd::default().into(),
            HCExtractHereCmd::default().into(),
            HCExtractToCmd::default().into(),
            HCOpenGuiCmd::default().into(),
        ];
        let enumerator: IEnumExplorerCommand = HCEnumCommands::new(children).into();
        Ok(enumerator)
    }
}

// ----- Enumerator ----------------------------------------------------------

#[implement(IEnumExplorerCommand)]
struct HCEnumCommands {
    items: Vec<IExplorerCommand>,
    cursor: Mutex<usize>,
}

impl HCEnumCommands {
    fn new(items: Vec<IExplorerCommand>) -> Self {
        Self {
            items,
            cursor: Mutex::new(0),
        }
    }
}

impl IEnumExplorerCommand_Impl for HCEnumCommands_Impl {
    fn Next(
        &self,
        celt: u32,
        rgelt: *mut Option<IExplorerCommand>,
        pceltfetched: *mut u32,
    ) -> windows::core::HRESULT {
        if rgelt.is_null() {
            return E_POINTER;
        }
        let mut cursor = self.cursor.lock().unwrap();
        let mut fetched = 0u32;
        for i in 0..celt {
            if *cursor >= self.items.len() {
                break;
            }
            let item = self.items[*cursor].clone();
            unsafe {
                *rgelt.add(i as usize) = Some(item);
            }
            *cursor += 1;
            fetched += 1;
        }
        if !pceltfetched.is_null() {
            unsafe { *pceltfetched = fetched; }
        }
        if fetched == celt { S_OK } else { S_FALSE }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        let mut cursor = self.cursor.lock().unwrap();
        *cursor = (*cursor + celt as usize).min(self.items.len());
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        *self.cursor.lock().unwrap() = 0;
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumExplorerCommand> {
        let cursor = *self.cursor.lock().unwrap();
        let cloned = HCEnumCommands {
            items: self.items.clone(),
            cursor: Mutex::new(cursor),
        };
        Ok(cloned.into())
    }
}

// ----- Compress to .hc -----------------------------------------------------

#[implement(IExplorerCommand)]
#[derive(Default)]
struct HCCompressCmd;

impl IExplorerCommand_Impl for HCCompressCmd_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr("Compress to .hc")
    }
    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr(&icon_path())
    }
    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567892))
    }
    fn GetState(&self, items: Option<&IShellItemArray>, _slow: BOOL) -> Result<u32> {
        let paths = paths_from(items);
        if !paths.is_empty() && all_have_ext(&paths, HC_EXT) {
            Ok(ECS_HIDDEN.0 as u32)
        } else {
            Ok(ECS_ENABLED.0 as u32)
        }
    }
    fn Invoke(&self, items: Option<&IShellItemArray>, _ctx: Option<&IBindCtx>) -> Result<()> {
        let paths = paths_from(items);
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["compress".to_string()];
        for p in &paths {
            args.push(p.to_string_lossy().into_owned());
        }
        spawn_hypercompress(&args)
    }
    fn GetFlags(&self) -> Result<u32> { Ok(0) }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> { Err(E_NOTIMPL.into()) }
}

// ----- Extract Here --------------------------------------------------------

#[implement(IExplorerCommand)]
#[derive(Default)]
struct HCExtractHereCmd;

impl IExplorerCommand_Impl for HCExtractHereCmd_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr("Extract Here")
    }
    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr(&icon_path())
    }
    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567893))
    }
    fn GetState(&self, items: Option<&IShellItemArray>, _slow: BOOL) -> Result<u32> {
        let paths = paths_from(items);
        if any_has_ext(&paths, HC_EXT) {
            Ok(ECS_ENABLED.0 as u32)
        } else {
            Ok(ECS_HIDDEN.0 as u32)
        }
    }
    fn Invoke(&self, items: Option<&IShellItemArray>, _ctx: Option<&IBindCtx>) -> Result<()> {
        let paths = paths_from(items);
        for p in paths.iter().filter(|p| has_hc_ext(p)) {
            let parent = p.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let args = vec![
                "decompress".to_string(),
                p.to_string_lossy().into_owned(),
                parent.to_string_lossy().into_owned(),
            ];
            let _ = spawn_hypercompress(&args);
        }
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> { Ok(0) }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> { Err(E_NOTIMPL.into()) }
}

// ----- Extract to "name\" --------------------------------------------------

#[implement(IExplorerCommand)]
#[derive(Default)]
struct HCExtractToCmd;

impl IExplorerCommand_Impl for HCExtractToCmd_Impl {
    fn GetTitle(&self, items: Option<&IShellItemArray>) -> Result<PWSTR> {
        let paths = paths_from(items);
        let label = if paths.len() == 1 {
            paths[0]
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| format!("Extract to \"{s}\\\""))
                .unwrap_or_else(|| "Extract to subfolder".to_string())
        } else {
            "Extract each to subfolder".to_string()
        };
        alloc_pwstr(&label)
    }
    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr(&icon_path())
    }
    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567894))
    }
    fn GetState(&self, items: Option<&IShellItemArray>, _slow: BOOL) -> Result<u32> {
        let paths = paths_from(items);
        if any_has_ext(&paths, HC_EXT) {
            Ok(ECS_ENABLED.0 as u32)
        } else {
            Ok(ECS_HIDDEN.0 as u32)
        }
    }
    fn Invoke(&self, items: Option<&IShellItemArray>, _ctx: Option<&IBindCtx>) -> Result<()> {
        let paths = paths_from(items);
        for p in paths.iter().filter(|p| has_hc_ext(p)) {
            let parent = p.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let stem = p.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
            let target = parent.join(stem);
            let args = vec![
                "decompress".to_string(),
                p.to_string_lossy().into_owned(),
                target.to_string_lossy().into_owned(),
            ];
            let _ = spawn_hypercompress(&args);
        }
        Ok(())
    }
    fn GetFlags(&self) -> Result<u32> { Ok(0) }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> { Err(E_NOTIMPL.into()) }
}

// ----- Open with HyperCompress (launch GUI) --------------------------------

#[implement(IExplorerCommand)]
#[derive(Default)]
struct HCOpenGuiCmd;

impl IExplorerCommand_Impl for HCOpenGuiCmd_Impl {
    fn GetTitle(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr("Open with HyperCompress")
    }
    fn GetIcon(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        alloc_pwstr(&icon_path())
    }
    fn GetToolTip(&self, _items: Option<&IShellItemArray>) -> Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }
    fn GetCanonicalName(&self) -> Result<GUID> {
        Ok(GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567895))
    }
    fn GetState(&self, _items: Option<&IShellItemArray>, _slow: BOOL) -> Result<u32> {
        Ok(ECS_ENABLED.0 as u32)
    }
    fn Invoke(&self, items: Option<&IShellItemArray>, _ctx: Option<&IBindCtx>) -> Result<()> {
        let paths = paths_from(items);
        let mut args: Vec<String> = Vec::new();
        if let Some(p) = paths.first() {
            args.push(if has_hc_ext(p) { "decompress".to_string() } else { "compress".to_string() });
            for p in &paths {
                args.push(p.to_string_lossy().into_owned());
            }
        }
        spawn_hypercompress(&args)
    }
    fn GetFlags(&self) -> Result<u32> { Ok(0) }
    fn EnumSubCommands(&self) -> Result<IEnumExplorerCommand> { Err(E_NOTIMPL.into()) }
}

fn has_hc_ext(p: &PathBuf) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case(HC_EXT))
        .unwrap_or(false)
}
