const GUI_APP_NAME: &'static str = "Everest Runtime Bootstrapper";

#[cfg(target_os = "linux")] pub mod linux;
#[cfg(target_os = "linux")] pub use linux::*;

#[cfg(target_os = "windows")] pub mod win;
#[cfg(target_os = "windows")] pub use win::*;