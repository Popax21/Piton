#[cfg(not(any(target_os = "linux", target_os = "windows")))]
compile_error!("no GUI support for target OS");

#[cfg(target_os = "linux")] pub mod linux;
#[cfg(target_os = "linux")] pub use linux::*;

#[cfg(target_os = "windows")] pub mod win;
#[cfg(target_os = "windows")] pub use win::*;