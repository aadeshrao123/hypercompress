use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::core::{Result, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HMODULE, E_FAIL, E_OUTOFMEMORY};
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::Win32::UI::Shell::{IShellItemArray, SIGDN_FILESYSPATH};

/// Allocate a PWSTR via CoTaskMemAlloc and copy a UTF-16 string into it.
/// Caller (Explorer) frees with CoTaskMemFree.
pub fn alloc_pwstr(s: &str) -> Result<PWSTR> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * 2;
    let ptr = unsafe { CoTaskMemAlloc(bytes) } as *mut u16;
    if ptr.is_null() {
        return Err(windows::core::Error::from(E_OUTOFMEMORY));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
    }
    Ok(PWSTR(ptr))
}

/// Read all filesystem paths from an IShellItemArray.
pub fn collect_paths(items: Option<&IShellItemArray>) -> Vec<PathBuf> {
    let Some(arr) = items else { return Vec::new(); };
    let count = match unsafe { arr.GetCount() } {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let Ok(item) = (unsafe { arr.GetItemAt(i) }) else { continue };
        let Ok(name) = (unsafe { item.GetDisplayName(SIGDN_FILESYSPATH) }) else { continue };
        if name.is_null() {
            continue;
        }
        let path = unsafe { pwstr_to_path(name) };
        unsafe { CoTaskMemFree(Some(name.as_ptr() as *const _)) };
        if let Some(p) = path {
            out.push(p);
        }
    }
    out
}

unsafe fn pwstr_to_path(p: PWSTR) -> Option<PathBuf> {
    if p.is_null() {
        return None;
    }
    let mut len = 0usize;
    while *p.0.add(len) != 0 {
        len += 1;
    }
    let slice = std::slice::from_raw_parts(p.0, len);
    Some(PathBuf::from(OsString::from_wide(slice)))
}

pub fn any_has_ext(paths: &[PathBuf], ext: &str) -> bool {
    paths.iter().any(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case(ext))
            .unwrap_or(false)
    })
}

pub fn all_have_ext(paths: &[PathBuf], ext: &str) -> bool {
    !paths.is_empty()
        && paths.iter().all(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case(ext))
                .unwrap_or(false)
        })
}

pub fn find_hypercompress_exe() -> Option<PathBuf> {
    let dll_path = current_module_path()?;
    let dir = dll_path.parent()?;
    // Tauri's NSIS bundle uses the cargo package name `gui` for the binary.
    // Try the most likely names in order.
    for name in ["gui.exe", "HyperCompress.exe", "hypercompress.exe"] {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

pub fn current_install_dir() -> Option<PathBuf> {
    current_module_path()?.parent().map(|p| p.to_path_buf())
}

pub fn current_module_path() -> Option<PathBuf> {
    let mut hmodule = HMODULE::default();
    let addr = current_module_path as *const ();
    let ok = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(addr as *const u16),
            &mut hmodule as *mut _,
        )
    };
    if ok.is_err() {
        return None;
    }
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetModuleFileNameW(hmodule, &mut buf) };
    if n == 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(PathBuf::from(OsString::from_wide(&buf)))
}

/// Launch hypercompress.exe with the given args. Detached, no console window.
pub fn spawn_hypercompress(args: &[String]) -> Result<()> {
    let exe = find_hypercompress_exe()
        .ok_or_else(|| windows::core::Error::from(E_FAIL))?;

    let mut cmdline = String::new();
    cmdline.push('"');
    cmdline.push_str(&exe.to_string_lossy());
    cmdline.push('"');
    for a in args {
        cmdline.push_str(" \"");
        cmdline.push_str(&a.replace('"', "\\\""));
        cmdline.push('"');
    }
    let mut cmd_w: Vec<u16> = cmdline.encode_utf16().chain(std::iter::once(0)).collect();

    let mut si = STARTUPINFOW::default();
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi = PROCESS_INFORMATION::default();

    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmd_w.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        )?;
        let _ = CloseHandle(pi.hProcess);
        let _ = CloseHandle(pi.hThread);
    }
    Ok(())
}
