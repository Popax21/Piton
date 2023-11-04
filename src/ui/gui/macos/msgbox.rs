use std::error::Error;

use crate::cfg::UI_APP_NAME;
use cacao::appkit::*;

pub fn show_error_msgbox(error_msg: &str) -> Result<(), Box<dyn Error>>{
    let alert = Alert::new(UI_APP_NAME, error_msg);
    alert.show();
    Ok(())
}