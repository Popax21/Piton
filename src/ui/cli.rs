use std::error::Error;

use indicatif::{ProgressBar, ProgressStyle};

use super::ProgressAction;

pub type CLIProgressAction = ProgressBar;

impl ProgressAction for CLIProgressAction {
    fn set_progress(&self, txt: &str, fract: f64) {
        self.set_message(String::from(txt));
        self.set_position((fract * 100_000_f64) as u64);
    }
 
    fn is_cancelled(&self) -> bool { false }

    fn println(&self, msg: &str) { self.println(msg) }
}

pub fn run_progress_action<T: Send>(descr: &str, action: impl FnOnce(&CLIProgressAction) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    let prog_bar = ProgressBar::new(100_000)
        .with_style(ProgressStyle::default_bar().template("\n{prefix}\n> {msg}\n{wide_bar}").expect("failed to create progress bar style"))
        .with_prefix(String::from(descr));

    let res = action(&prog_bar);

    prog_bar.finish_and_clear();

    Ok(Some(res))
}