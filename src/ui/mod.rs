use std::error::Error;

use crate::cfg::UI_DRIVER;

#[cfg(not(any(feature = "gui", feature = "cli")))]
compile_error!("either feature \"cli\" or feature \"gui\" (or both) has to be enabled");

#[cfg(feature = "cli")] mod cli;
#[cfg(feature = "gui")] mod gui;

pub mod log;

#[derive(serde::Deserialize)]
pub enum UIDriver {
    None,

    #[cfg(feature = "cli")]
    #[serde(rename = "cli")]
    Cli,

    #[cfg(feature = "gui")]
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
        
        #[cfg(feature = "cli")]
        UIDriver::Cli => cli::run_progress_action(descr, move |act| action(act)),

        #[cfg(feature = "gui")]
        UIDriver::Gui => gui::run_progress_action(descr, move |act| action(act))
    }
}

pub fn show_error_msg(msg: &str) {
    match UI_DRIVER {
        #[cfg(feature = "gui")]
        UIDriver::Gui => gui::show_error_msgbox(msg).expect("failed to show the error message box"),

        _ => eprintln!("{msg}")
    };
}