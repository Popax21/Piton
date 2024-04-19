use std::error::Error;

use indicatif::{ProgressBar, ProgressStyle};

use super::{ProgressAction, log::LogHook};

pub type CLIProgressAction = ProgressBar;

impl ProgressAction for CLIProgressAction {
    fn set_progress(&self, txt: &str, fract: f64) {
        self.set_message(String::from(txt));
        self.set_position((fract * 100_000_f64) as u64);
    }
 
    fn is_cancelled(&self) -> bool { false }
}

pub fn run_progress_action<T: Send>(descr: &str, action: impl FnOnce(&CLIProgressAction) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    //Create the progress bar
    let prog_bar = ProgressBar::new(100_000)
        .with_style(ProgressStyle::default_bar().template("{prefix}\n> {msg}\n{wide_bar}").expect("failed to create progress bar style"))
        .with_prefix(String::from(descr));
    let prog_bar = &prog_bar;

    //Hook the logs to properly work with the progress bar
    let log_hook_fnc = |msg: &_| prog_bar.println(msg);
    let log_hook = LogHook::create(&log_hook_fnc);

    //Run the action
    let res = action(prog_bar);

    //Cleanup
    prog_bar.finish_and_clear();
    drop(log_hook);

    Ok(Some(res))
}