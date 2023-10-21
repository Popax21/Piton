use std::error::Error;
use std::ops::Deref;
use std::sync::{Mutex, OnceLock};
use std::thread::{self, ThreadId};

use gtk::glib::translate::ToGlibPtr;
use gtk::glib::{ControlFlow, BoolError};
use gtk::{prelude::*, Dialog, Label, Orientation, ProgressBar, Window, ResponseType};
use gtk::{DialogFlags, MessageDialog, MessageType, ButtonsType};

use crate::cfg::GUI_APP_NAME;

fn init_gtk() -> Result<(), BoolError> {
    static GTK_INIT_LOCK: OnceLock<Result<ThreadId, BoolError>> = OnceLock::new();

    //Ensure GTK is only initialized once
    let gtk_main_thread = GTK_INIT_LOCK.get_or_init(|| {
        gtk::init()?;
        gtk::glib::set_prgname(Some(GUI_APP_NAME));
        Ok(thread::current().id())
    }).clone()?;

    //Ensure that this call comes from the same thread which first initialized GTK (and as such became the main thread)
    assert_eq!(thread::current().id(), gtk_main_thread);
    Ok(())
}

fn set_window_wmclass(window: &Window) {
    //gtk3-rs doesn't expose gtk_window_set_wmclass in the safe API
    //gtk-rs (the obsolete predecesor) does however
    //So, uhm, yeah, we have to invoke it on our own ._.
    let name = gtk::glib::application_name().unwrap();
    unsafe {
        gtk::ffi::gtk_window_set_wmclass(window.to_glib_none().0, name.as_ptr(), name.as_ptr())
    }
}

pub fn show_error_msgbox(error_msg: &str) -> Result<(), Box<dyn Error>>{
    init_gtk()?;

    //Create the dialog box
    let dialog = MessageDialog::new(None::<&gtk::Window>, DialogFlags::MODAL, MessageType::Error, ButtonsType::Close, error_msg);
    set_window_wmclass(dialog.upcast_ref());
    dialog.set_title(&format!("{GUI_APP_NAME} Error"));
    
    //Show the dialog box
    dialog.connect_response(|_, _| gtk::main_quit());
    dialog.show();
    gtk::main();
 
    Ok(())
}

#[derive(Default)]
struct ProgressState {
    done: bool,
    cancelled: bool,

    dirty: bool,
    text: String,
    fract: f64
}

pub struct ProgressDialog<'d> where Self: 'd {
    state: &'d Mutex<ProgressState>
}

impl<'d> ProgressDialog<'d> {
    pub fn set_progress(&self, txt: impl Deref<Target=str> + Send + 'd, fract: f64) {
        //Update the progress state
        let mut state = self.state.lock().unwrap();
        state.dirty = true;
        state.text = String::from(txt.deref());
        state.fract = fract;
    }
 
    pub fn is_cancelled(&self) -> bool { self.state.lock().unwrap().cancelled }
}

pub fn run_progress_dialog<T: Send>(descr: &str, action: impl FnOnce(&ProgressDialog) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    init_gtk()?;

    //Create the dialog GUI
    let dialog = Dialog::new();
    set_window_wmclass(dialog.upcast_ref());
    dialog.set_title(GUI_APP_NAME);
    dialog.set_size_request(400, 0);
    dialog.set_resizable(false);

    let dialog_content = dialog.content_area();
    dialog_content.set_spacing(10);
    dialog_content.set_margin(10);

    // - description label
    let descr_label: Label = Label::new(Some(descr));
    descr_label.set_markup(&format!("<span size='medium'>{descr}</span>"));
    descr_label.set_margin_bottom(5);
    dialog_content.add(&descr_label);

    // - progress label
    let progress_label_box = gtk::Box::new(Orientation::Horizontal, 0);
    let progress_label = Label::new(None);
    progress_label_box.add(&progress_label);
    dialog_content.add(&progress_label_box);
    
    // - progress bar
    let progress_bar = ProgressBar::new();
    dialog_content.add(&progress_bar);

    //Setup the progress state
    let prog_state = &Mutex::new(ProgressState::default());

    thread::scope(move |scope| {
        //Start the worker thread
        let work_thread: thread::ScopedJoinHandle<Option<T>> = scope.spawn(move || {
            //Setup the poison pill which sets the done flag upon exit
            struct PoisonPill<'a>(&'a Mutex<ProgressState>);
            impl Drop for PoisonPill<'_> {
                fn drop(&mut self) {
                    self.0.lock().unwrap().done = true;
                }
            }
            let _pill = PoisonPill(prog_state);

            //Run the action
            let ret = action(&ProgressDialog { state: prog_state });

            if !prog_state.lock().unwrap().cancelled {
                Some(ret)
            } else {
                None
            }
        });

        //Show the dialog while executing callbacks from the worker thread
        //Use unsafe to marshal references into the callback (which requires a 'static lifetime)
        unsafe {
            struct ProgressRefs<'a>(&'a Mutex<ProgressState>, &'a Label, &'a ProgressBar);
            let prog_refs = ProgressRefs(prog_state, &progress_label, &progress_bar);
            let prog_refs = std::mem::transmute::<ProgressRefs::<'_>, ProgressRefs::<'static>>(prog_refs);

            dialog.add_tick_callback(move |dialog, _| {
                //Check if the state is dirty
                //If yes, update widgets
                let mut prog_state = prog_refs.0.lock().unwrap();
                if prog_state.dirty {
                    prog_refs.1.set_text(&prog_state.text);
                    prog_refs.2.set_fraction(prog_state.fract);
                    prog_state.dirty = false;
                }

                //Check if the worker thread is done
                //If yes, close the dialog
                if prog_state.done {
                    dialog.response(ResponseType::Close);
                }

                ControlFlow::Continue
            });

            dialog.show_all();
            dialog.run(); //The tick callback can only be invoked through this method, so the above transmute is safe
        }

        //Set the cancel flag
        prog_state.lock().unwrap().cancelled = true;

        //Wait for the worker thread to finish
        match work_thread.join() {
            Ok(r) => Ok(r),
            Err(e) => std::panic::resume_unwind(e)
        }
    })
}