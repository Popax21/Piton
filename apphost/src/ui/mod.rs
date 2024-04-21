use std::error::Error;

use crate::cfg::UI_DRIVER;

#[cfg(feature = "ui-cli")] mod cli;
#[cfg(feature = "ui-gui")] mod gui;

pub mod log;

#[derive(serde::Deserialize)]
pub enum UIDriver {
    #[serde(rename = "none")]
    None,

    #[cfg(feature = "ui-cli")]
    #[serde(rename = "cli")]
    Cli,

    #[cfg(feature = "ui-gui")]
    #[serde(rename = "gui")]
    Gui
}

pub trait ProgressAction {
    fn set_progress(&self, txt: &str, fract: f64);
    fn is_cancelled(&self) -> bool;
}

pub fn run_progress_action<T: Send>(descr: &str, action: impl FnOnce(&dyn ProgressAction) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    match UI_DRIVER {
        UIDriver::None => {
            struct NoOpProgressAction;
            impl ProgressAction for NoOpProgressAction {
                fn set_progress(&self, _txt: &str, _fract: f64) {}
                fn is_cancelled(&self) -> bool { false }
            }

            Ok(Some(action(&NoOpProgressAction{})))
        }
        
        #[cfg(feature = "ui-cli")]
        UIDriver::Cli => cli::run_progress_action(descr, move |act| action(act)),

        #[cfg(feature = "ui-gui")]
        UIDriver::Gui => gui::run_progress_action(descr, move |act| action(act))
    }
}

pub fn show_error_msg(msg: &str) {
    match UI_DRIVER {
        #[cfg(feature = "ui-gui")]
        UIDriver::Gui => gui::show_error_msgbox(msg).expect("failed to show the error message box"),

        _ => eprintln!("{msg}")
    };
}