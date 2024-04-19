type WinError = windows::core::Error;

mod layout;
mod dialog_template;
mod dpi;

pub mod msgbox;
pub use msgbox::*;

pub mod progress_dialog;
pub use progress_dialog::*;