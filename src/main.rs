#![windows_subsystem = "windows"]

use std::{process::ExitCode, fs, path::PathBuf, io};

mod cfg;
mod runtime;
mod setup;
mod ui;

use runtime::*;
use setup::*;

#[cfg(not(feature="testapp"))]
//Contains the placeholder string (sha256 of foobar), which is replaced by Microsoft.NET.HostModel.HostWriter on build
const APP_BINARY_PATH: &str = "c3ab8ff13720e8ad9047dd39466b3c8974e592c2fa383d4a3960714caef0c4f2";

#[cfg(feature="testapp")]
const APP_BINARY_PATH: &str = "Test.dll";

macro_rules! handle_error {
    ($res:expr, $($msg_arg:tt)+) => {
        match $res {
            Ok(v) => v,
            Err(err) => {
                let msg = format!($($msg_arg)+);
                log!("Piton encountered an error while setting up the .NET runtime:");
                log!("{}: {err:?}", msg);

                let err_msg: String;
                if cfg::UI_ERRORMSG_HEADER.len() > 0 {
                    err_msg = format!("{header}\n\n{msg}:\n{err}", header = cfg::UI_ERRORMSG_HEADER);
                } else {
                    err_msg = format!("{msg}:\n{err}");
                }
                ui::show_error_msg(&err_msg);

                return ExitCode::FAILURE;
            }
        }
    };
}

macro_rules! run_app_binary {
    ($runtime_dir:expr, $app_bin_path:expr) => {
        let args = std::env::args().collect::<Vec<String>>();
        let app_res = handle_error!(launch_app_binary(&$runtime_dir, &$app_bin_path, &args.iter().skip(1).map(String::as_ref).collect::<Vec<&str>>()), "Failed to launch the application binary '{}'", $app_bin_path.display());
        return ExitCode::from(app_res as u8);
    }
}

fn main() -> ExitCode {
    //Handle PITON_WIN_CONSOLE on Windows
    #[cfg(all(target_os = "windows", feature = "gui"))]
    if let Ok(console_env) = std::env::var("PITON_WIN_CONSOLE") {
        unsafe {
            match &console_env {
                "alloc" => {
                    windows::Win32::System::Console::AllocConsole().expect("failed to allocate a Win32 console");
                },
                "attach" => {
                    windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS).expect("failed to attach to parent Win32 console");
                },
                _ => panic!("Unexpected value '{console_env}' for PITON_WIN_CONSOLE")
            }
        }
    }

    //Setup paths
    let install_dir = if !cfg!(feature="testapp") {
        PathBuf::from(std::env::current_exe().unwrap().parent().unwrap())
    } else {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("test");
        path
    };
    
    let app_bin_path = install_dir.join(&APP_BINARY_PATH[..APP_BINARY_PATH.chars().position(|c| c == '\x00').unwrap_or(APP_BINARY_PATH.len())]);

    if !app_bin_path.is_file() {
        handle_error!(Err(io::Error::from(io::ErrorKind::NotFound)), "Failed to find managed application binary '{}'", app_bin_path.display());
    }

    //Read this target's runtime descriptor
    let target_id = format!("{os}-{bits}", os = std::env::consts::OS, bits = std::env::consts::ARCH);

    let runtimes_file = install_dir.join(cfg::RUNTIME_DESCR_FILE);
    let runtime_descr = handle_error!(read_runtime_descr(&runtimes_file, &target_id), "Failed to read the runtime descriptor for target '{target_id}'");
    log!("Read runtime descriptor for target '{target_id}': version {runtime_ver}", runtime_ver = runtime_descr.version);

    //Check if the runtime already exists
    for runtime_dir in cfg::RUNTIME_DIR_PATHS { 
        let runtime_dir = install_dir.join(runtime_dir);
        match check_runtime_install(&runtime_dir, &runtime_descr, &target_id) {
            RuntimeCheckResult::Compatible => {
                log!("Detected compatible existing runtime '{}', launching...", runtime_dir.display());
                run_app_binary!(runtime_dir, app_bin_path);
            }
            check_res => log!("Existing runtime isn't compatible: {check_res:?}")
        };
    }

    log!("Unable to locate existing compatible runtime, setting up new one");
    let runtime_dir = install_dir.join(cfg::RUNTIME_DIR_PATHS[0]);
    
    //Remove the old runtime
    if runtime_dir.exists() {
        handle_error!(fs::remove_dir_all(&runtime_dir), "Failed to remove existing runtime");
    }

    //Set up the runtime
    let runtime_setup_res = setup_runtime(&target_id, &runtime_descr, &runtime_dir);
    match runtime_setup_res {
        Err(SetupError::DownloadServerUnreachable { server, error: err }) => {
            ui::show_error_msg(&format!(
r#"Failed to download the .NET runtime.
The download server '{server}' could not be reached.
Please ensure you are connected to the internet, then try again.

Detailed error information:
{err}"#
            ));
            return ExitCode::FAILURE;
        }
        Err(SetupError::Cancelled) => { return ExitCode::SUCCESS; }
        r => { handle_error!(r, "Failed to set up the .NET runtime"); }
    }

    //Run the app binary now
    log!("Launching app after runtime setup completed successfully...");
    run_app_binary!(runtime_dir, app_bin_path);
}