use std::error::Error;

use windows::{Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK, MB_SYSTEMMODAL}, core::HSTRING};

use crate::cfg::UI_APP_NAME;

use super::WinError;

pub fn show_error_msgbox(error_msg: &str) -> Result<(), Box<dyn Error>>{
    unsafe {
        match MessageBoxW(None, &HSTRING::from(error_msg), &HSTRING::from(UI_APP_NAME), MB_OK | MB_ICONERROR | MB_SYSTEMMODAL).0 {
            0 => Err(Box::new(WinError::from_win32())),
            _ => Ok(())
        }
    }
}