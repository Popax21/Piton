use crate::ui::UIDriver;

pub const IS_QUIET: bool = true;

pub const RUNTIME_DESCR_FILE: &str = "piton-runtime.yaml";

pub const RUNTIME_DIR_PATHS: &[&str] = &[
    "piton-runtime"
];

#[allow(unreachable_code)]
const fn det_default_ui_driver() -> UIDriver {
    #[cfg(feature = "gui")] return UIDriver::Gui;
    #[cfg(feature = "cli")] return UIDriver::Cli;
    UIDriver::None
}

pub const UI_DRIVER: UIDriver = det_default_ui_driver();

#[allow(unused)]
pub const UI_APP_NAME: &str = "Everest Runtime Bootstrapper";

#[allow(unused)]
pub const UI_ERRORMSG_HEADER: &str =
r#"An error occurred while trying to prepare Everest for startup.
Please report this in the Celeste discord server!
(https://discord.gg/celeste, channel #modding_help)"#;