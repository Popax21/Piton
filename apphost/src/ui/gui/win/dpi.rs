//Excuse my french, but what the absolute f*ck is this, Microsoft ._.

use std::{sync::OnceLock, ops::Deref, error::Error};

use windows::{Win32::{System::LibraryLoader::{GetProcAddress, LoadLibraryA}, Foundation::{BOOL, HWND, HMODULE, FreeLibrary, HANDLE, ERROR_BAD_ARGUMENTS, ERROR_MOD_NOT_FOUND, RECT}, Graphics::Gdi::{HMONITOR, MonitorFromWindow, MONITOR_DEFAULTTONEAREST, GetDC, GetDeviceCaps, LOGPIXELSX, ReleaseDC, LOGPIXELSY}, UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_ACTION}, core::{s, HRESULT}};

use super::WinError;

pub const DPI_100P: i32 = 96;

pub struct DPIMetrics;
impl DPIMetrics {
    pub fn get_system_dpi() -> Result<(i32, i32), WinError> {
        //Use GetDpiForSystem (Win10 build 1607+)
        //If that fails, inspect the device capabilities
        if let Some(fnc) = GetDpiForSystem::get() {
            let dpi = fnc() as i32;
            return Ok((dpi, dpi));
        }

        //Fall back to inspecting device capabilities
        unsafe {
            let dc = GetDC(HWND::default());
            let h_dpi = GetDeviceCaps(dc, LOGPIXELSX);
            let v_dpi = GetDeviceCaps(dc, LOGPIXELSY);
            ReleaseDC(HWND::default(), dc);

            Ok((h_dpi, v_dpi))
        }
    }

    pub unsafe fn get_system_parameters_info(action: SYSTEM_PARAMETERS_INFO_ACTION, param1: u32, param2: usize, dpi: i32) -> Option<Result<(), WinError>> {
        let Some(fnc) = SystemParametersInfoForDpi::get() else { return None; };
        if fnc(action.0, param1, param2, 0, dpi as u32).as_bool() {
            Some(Ok(()))
        } else {
            Some(Err(WinError::from_win32()))
        }
    }

    pub fn adjust_window_rect(rect: &mut RECT, style: u32, menu: bool, ext_style: u32, dpi: i32) -> Option<Result<(), WinError>> {
        let Some(fnc) = AdjustWindowRectExForDpi::get() else { return None; };
        if fnc(rect as *mut _, style, BOOL::from(menu), ext_style, dpi as u32).as_bool() {
            Some(Ok(()))
        } else {
            Some(Err(WinError::from_win32()))
        }
    }
}

#[repr(isize)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DPIAwarenessContext {
    Unaware = -1,
    SystemAware = -2,
    PerMonitorAware = -3,
    PerMonitorAwareV2 = -4,
    UnwareGDIScaled = -5
}
impl Copy for DPIAwarenessContext {}

impl DPIAwarenessContext {
    pub fn get_current() -> DPIAwarenessContext {
        //Try the per-thread API
        if let Some(fnc) = GetThreadDpiAwarenessContext::get() {
            let ctx = fnc();
            let equal_fnc = AreDpiAwarenessContextsEqual::get().as_ref().expect("failed to resolve AreDpiAwarenessContextsEqual binding");

            return if equal_fnc(ctx, DPIAwarenessContext::Unaware as isize).as_bool() {
                DPIAwarenessContext::Unaware
            } else if equal_fnc(ctx, DPIAwarenessContext::SystemAware as isize).as_bool() {
                DPIAwarenessContext::SystemAware
            } else if equal_fnc(ctx, DPIAwarenessContext::PerMonitorAware as isize).as_bool() {
                DPIAwarenessContext::PerMonitorAware
            } else if equal_fnc(ctx, DPIAwarenessContext::PerMonitorAwareV2 as isize).as_bool() {
                DPIAwarenessContext::PerMonitorAwareV2
            } else if equal_fnc(ctx, DPIAwarenessContext::UnwareGDIScaled as isize).as_bool() {
                DPIAwarenessContext::UnwareGDIScaled
            } else { panic!("unexpected GetThreadDpiAwarenessContext result: {ctx}"); };
        }

        //Try the older per-process API
        if let Some(fnc) = GetProcessDpiAwareness::get() {
            let mut awareness = ProcessDPIAwareness::Unaware as u32;
            fnc(HANDLE::default(), &mut awareness).ok().expect("failed to query process DPI awareness");

            return if awareness == ProcessDPIAwareness::Unaware as u32 {
                DPIAwarenessContext::Unaware
            } else if awareness == ProcessDPIAwareness::SystemAware as u32 {
                DPIAwarenessContext::SystemAware
            } else if awareness == ProcessDPIAwareness::PerMonitorAware as u32 {
                DPIAwarenessContext::PerMonitorAware
            } else { panic!("unexpected GetProcessDpiAwareness result: {awareness}"); };
        }

        //Try the even older per-process API
        if let Some(fnc) = IsProcessDPIAware::get() {
            return if fnc().as_bool() { DPIAwarenessContext::SystemAware } else { DPIAwarenessContext::Unaware };
        }

        //Fallback to unaware if no API is present for the zero users who are running this on Windows XP
        DPIAwarenessContext::Unaware
    }
}

#[derive(Debug)]
pub struct DPIAwarenessOverride(DPIAwarenessContext, isize);

impl DPIAwarenessOverride {
    pub fn try_apply(new_ctx: DPIAwarenessContext) -> Option<DPIAwarenessOverride> {
        //Check if SetThreadDpiAwarenessContext is available
        let Some(fnc) = SetThreadDpiAwarenessContext::get() else {
            //Fake a DPI override if it matches the process awareness setting
            return if DPIAwarenessContext::get_current() == new_ctx { Some(DPIAwarenessOverride(new_ctx, 0)) } else { None };
        };
        
        //Set the thread context
        let old_ctx = fnc(new_ctx as isize);
        if old_ctx == 0 { return None; }

        Some(DPIAwarenessOverride(new_ctx, old_ctx))
    }

    pub fn get_awareness(&self) -> DPIAwarenessContext { self.0 }

    pub fn get_current_dpi(&self, window: HWND)-> Result<(i32, i32), WinError> {
        unsafe {
            match self.0 {
                DPIAwarenessContext::Unaware | DPIAwarenessContext::UnwareGDIScaled => Ok((96, 96)),
                DPIAwarenessContext::SystemAware => DPIMetrics::get_system_dpi(),
                DPIAwarenessContext::PerMonitorAware => {
                    //Use GetDpiForWindow if available
                    //If not, then this must be before Win10 build 1607, so use the Win8.1 GetDpiForMonitor API
                    //This API must be available, since it was introduced as the same time as Per-Monitor DPI awareness
                    if let Some(fnc) = GetDpiForWindow::get() {
                        let dpi = fnc(window) as i32;
                        return if dpi != 0 { Ok((dpi, dpi)) } else { Err(WinError::from(ERROR_BAD_ARGUMENTS)) }
                    }
                    
                    //Fall back to GetDpiForMonitor
                    const MDT_EFFECTIVE_DPI: u32 = 0;

                    let fnc = GetDpiForMonitor::get().as_ref().expect("failed to obtain GetDpiForMonitor binding even though PerMonitorAware is in-use");
                    let mon = MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST);

                    let (mut h_dpi, mut v_dpi) = (0, 0);
                    fnc(mon, MDT_EFFECTIVE_DPI, &mut h_dpi, &mut v_dpi).ok()?;
                    Ok((h_dpi as i32, v_dpi as i32))
                }
                DPIAwarenessContext::PerMonitorAwareV2 => {
                    //We always have access to GetDpiForWindow for v2 since both were introduced in Win10 build 1607
                    let fnc = GetDpiForWindow::get().as_ref().expect("failed to obtain GetDpiForWindow binding even though PerMonitorAwareV2 is in-use");
                    let dpi = fnc(window) as i32;
                    if dpi != 0 { Ok((dpi, dpi)) } else { Err(WinError::from(ERROR_BAD_ARGUMENTS)) }
                }
            }
        }
    }
}

impl Drop for DPIAwarenessOverride {
    fn drop(&mut self) {
        if self.1 != 0 {
            SetThreadDpiAwarenessContext::get().as_ref().expect("failed to get SetThreadDpiAwarenessContext binding for DPI awareness override drop")(self.1);
        }
    }
}

#[allow(unused)]
#[repr(u32)]
#[derive(Debug)]
pub enum DialogDPIChangeBehaviors {
    Default = 0,
    DisableAll = 1,
    DisableResize = 2,
    DisableControlRelayout = 4,
    ALL = 7
}

impl DialogDPIChangeBehaviors {
    pub fn has_binding() -> bool {
        SetDialogDpiChangeBehavior::get().is_some()
    }

    pub fn set_for(window: HWND, mask: DialogDPIChangeBehaviors, val: DialogDPIChangeBehaviors) -> Result<(), Box<dyn Error>> {
        let Some(fnc) = SetDialogDpiChangeBehavior::get() else {
            return Err("failed to get SetDialogDpiChangeBehavior binding")?;
        };

        if fnc(window, mask, val).as_bool() {
            Ok(())
        } else {
            Err(Box::new(WinError::from_win32()))
        }
    }
}

#[allow(unused)]
#[repr(u32)]
#[derive(Debug)]
enum ProcessDPIAwareness {
    Unaware = 0,
    SystemAware = 1,
    PerMonitorAware = 2
}

//We have to generator our own weak bindings because the ones provided by windows-rs panic when not found
//And we can't break Win7 support ._.
macro_rules! gen_dll_binding {
    ($lib:literal, $fn_name:literal, $type_name:ident, $signature:ty) => {
        struct $type_name($signature, HMODULE);
        unsafe impl Send for $type_name {}
        unsafe impl Sync for $type_name {}
        
        impl Deref for $type_name {
            type Target = $signature;
            fn deref(&self) -> &Self::Target { &self.0 }
        }
        
        impl Drop for $type_name {
            fn drop(&mut self) {
                unsafe {
                    FreeLibrary(self.1).expect(concat!("failed to free library ", stringify!($lib), " handle on DPI function ", stringify!($fn_name), "binding drop"));
                }
            }
        }

        impl $type_name {
            fn get() -> &'static Option<$type_name> {
                static FUNC: OnceLock<Option<$type_name>> = OnceLock::new();
                FUNC.get_or_init(|| {
                    unsafe {
                        let lib_handle = match LoadLibraryA(s!($lib)) {
                            Ok(handle) => handle,
                            Err(err) => {
                                if err.code() == ERROR_MOD_NOT_FOUND.to_hresult() {
                                    return None;
                                } else {
                                    panic!("couldn't obtain {lib} handle: {err}", lib=stringify!($lib))
                                }
                            }
                        };
                        GetProcAddress(lib_handle, s!($fn_name))
                        .map(|fnc| $type_name(std::mem::transmute::<_, $signature>(fnc), lib_handle))
                    }
                })
            }
        }        
    };
}

//Windows 10 build 1703+
gen_dll_binding!("user32.dll", "SetDialogDpiChangeBehavior", SetDialogDpiChangeBehavior, extern "system" fn(HWND, DialogDPIChangeBehaviors, DialogDPIChangeBehaviors) -> BOOL);

//Windows 10 build 1607+
gen_dll_binding!("user32.dll", "GetThreadDpiAwarenessContext", GetThreadDpiAwarenessContext, extern "system" fn() -> isize);
gen_dll_binding!("user32.dll", "SetThreadDpiAwarenessContext", SetThreadDpiAwarenessContext, extern "system" fn(isize) -> isize);
gen_dll_binding!("user32.dll", "AreDpiAwarenessContextsEqual", AreDpiAwarenessContextsEqual, extern "system" fn(isize, isize) -> BOOL);
gen_dll_binding!("user32.dll", "GetDpiForSystem", GetDpiForSystem, extern "system" fn() -> u32);
gen_dll_binding!("user32.dll", "GetDpiForWindow", GetDpiForWindow, extern "system" fn(HWND) -> u32);
gen_dll_binding!("user32.dll", "SystemParametersInfoForDpi", SystemParametersInfoForDpi, extern "system" fn(u32, u32, usize, u32, u32) -> BOOL);
gen_dll_binding!("user32.dll", "AdjustWindowRectExForDpi", AdjustWindowRectExForDpi, extern "system" fn(*mut RECT, u32, BOOL, u32, u32) -> BOOL);

//Windows 8.1
gen_dll_binding!("shcore.dll", "GetProcessDpiAwareness", GetProcessDpiAwareness, extern "system" fn(HANDLE, &mut u32) -> HRESULT);
gen_dll_binding!("shcore.dll", "GetDpiForMonitor", GetDpiForMonitor, extern "system" fn(HMONITOR, u32, &mut u32, &mut u32) -> HRESULT);

//Windows Vista
gen_dll_binding!("user32.dll", "IsProcessDPIAware", IsProcessDPIAware, extern "system" fn() -> BOOL);