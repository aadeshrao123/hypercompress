#![allow(non_snake_case)]
#![cfg(windows)]

mod commands;
mod util;

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::core::{implement, IUnknown, Interface, Result, GUID, HRESULT};
use windows::Win32::Foundation::{BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, S_FALSE, S_OK};
use windows::Win32::System::Com::IClassFactory;
use windows::Win32::System::Com::IClassFactory_Impl;
use windows::Win32::UI::Shell::IExplorerCommand;

use crate::commands::HCRootCommand;

/// CLSID for the HyperCompress shell extension root command.
/// This GUID is referenced from AppxManifest.xml.
pub const CLSID_HC_ROOT_COMMAND: GUID = GUID::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567890);

/// DLL reference count — Explorer calls DllCanUnloadNow periodically.
static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);

pub fn dll_add_ref() {
    DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn dll_release() {
    DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
}

#[implement(IClassFactory)]
struct HCClassFactory;

impl IClassFactory_Impl for HCClassFactory_Impl {
    fn CreateInstance(
        &self,
        outer: Option<&IUnknown>,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if ppv.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *ppv = std::ptr::null_mut();
        }
        let cmd: IExplorerCommand = HCRootCommand::default().into();
        unsafe { cmd.query(&*riid, ppv).ok() }
    }

    fn LockServer(&self, lock: BOOL) -> Result<()> {
        if lock.as_bool() {
            dll_add_ref();
        } else {
            dll_release();
        }
        Ok(())
    }
}

#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        *ppv = std::ptr::null_mut();
        if *rclsid != CLSID_HC_ROOT_COMMAND {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = HCClassFactory.into();
        match factory.query(&*riid, ppv) {
            S_OK => S_OK,
            err => err,
        }
    }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if DLL_REF_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

/// Sparse-package shell extensions don't use DllRegisterServer/DllUnregisterServer
/// because the AppxManifest handles registration. We export stubs anyway in case
/// regsvr32 is invoked manually for debugging.
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    S_OK
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    S_OK
}
