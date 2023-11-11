use std::error::Error;
use std::sync::Mutex;
use std::thread;
use cacao::appkit::{App, AppDelegate};
use cacao::appkit::menu::{Menu, MenuItem};
use cacao::appkit::window::{Window, WindowConfig, WindowDelegate};
use cacao::layout::{Layout, LayoutConstraint};
use cacao::notification_center::Dispatcher;
use cacao::progress::ProgressIndicator;
use cacao::text::{Label, TextAlign};
use cacao::utils::activate_cocoa_multithreading;
use cacao::view::View;
use crate::ui::ProgressAction;
use crate::cfg::UI_APP_NAME;

#[derive(Default)]
struct ProgressState {
    done: bool,
    cancelled: bool,
    has_pending_msg: bool,

    dirty: bool,
    text: String,
    fract: f64
}

pub struct MacOSProgressAction<'a> {
    state: &'a Mutex<ProgressState>
}

impl ProgressAction for MacOSProgressAction<'_> {
    fn set_progress(&self, txt: &str, fract: f64) {
        //Update the progress state
        let mut state = self.state.lock().unwrap();
        state.dirty = true;
        state.text = String::from(txt);
        state.fract = fract;

        //Send a message to the main thread to update the progress window (if there isn't already a pending message)
        if !state.has_pending_msg {
            App::dispatch_main(UpdateProgressMsg);
            state.has_pending_msg = true;
        }
    }

    fn is_cancelled(&self) -> bool { self.state.lock().unwrap().cancelled }
}

pub fn run_progress_action<T: Send>(descr: &str, action: impl FnOnce(&MacOSProgressAction) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    //Setup the progress state
    let prog_state = &Mutex::new(ProgressState::default());

    //Create the application & window
    let window_delegate = ProgressDialogWindow {
        content: View::new(),

        descr_text: descr,
        descr_label: Label::new(),

        progress_label: Label::new(),
        progress_bar: ProgressIndicator::new()
    };

    let app = App::new("io.github.everestapi.piton", ProgressDialogApp {
        window: Window::with(WindowConfig::default(), window_delegate),
        state: prog_state
    });

    thread::scope(move |scope| {
        //Start the worker thread
        let work_thread: thread::ScopedJoinHandle<Option<T>> = scope.spawn(move || {
            activate_cocoa_multithreading();

            //Setup the poison pill which sets the done flag upon exit
            struct PoisonPill<'a>(&'a Mutex<ProgressState>);
            impl Drop for PoisonPill<'_> {
                fn drop(&mut self) {
                    self.0.lock().unwrap().done = true;
                }
            }
            let _pill = PoisonPill(prog_state);

            //Run the action
            let ret = action(&MacOSProgressAction { state: prog_state });

            if !prog_state.lock().unwrap().cancelled {
                Some(ret)
            } else {
                None
            }
        });

        //Run the app
        app.run();

        //Set the cancel flag
        prog_state.lock().unwrap().cancelled = true;

        //Wait for the worker thread to finish
        match work_thread.join() {
            Ok(r) => Ok(r),
            Err(e) => std::panic::resume_unwind(e)
        }
    })
}

struct ProgressDialogApp<'a> {
    window: Window<ProgressDialogWindow<'a>>,
    state: &'a Mutex<ProgressState>
}

impl AppDelegate for ProgressDialogApp<'_> {
    fn did_finish_launching(&self) {
        //Define the app-level menu (required)
        App::set_menu(vec![
            Menu::new("", vec![ MenuItem::Quit ])
        ]);

        //Bring the app to the foreground and show its window
        App::activate();
        self.window.show();
    }

    fn should_terminate_after_last_window_closed(&self) -> bool { true }
}

struct UpdateProgressMsg;
impl Dispatcher for ProgressDialogApp<'_> {
    type Message = UpdateProgressMsg;

    fn on_ui_message(&self, _: Self::Message) {
        //Lock the progress state
        let mut state = self.state.lock().unwrap();
        assert!(state.has_pending_msg);

        //Check if the worker thread is done
        if state.done {
            self.window.close();
            return;
        }

        //Apply the state to the window
        let window = self.window.delegate.unwrap();
        window.progress_label.set_text(state.text);
        window.progress_bar.set_value(state.fract);

        state.has_pending_msg = false;
    }
}

struct ProgressDialogWindow<'a> {
    content: View,

    descr_label: Label,
    descr_text: &'a str,

    progress_label: Label,
    progress_bar: ProgressIndicator,
}

//Implementation of NSWindowDelegate
impl WindowDelegate for ProgressDialogWindow<'_> {
    const NAME: &'static str = "WindowDelegate";

    fn did_load(&mut self, window: Window) {
        // - description label
        self.descr_label.set_text(self.descr_text);
        self.descr_label.set_text_alignment(TextAlign::Center);
        self.content.add_subview(&self.descr_label);

        // - progress label
        self.progress_label.set_text("");
        self.progress_label.set_text_alignment(TextAlign::Left);
        self.content.add_subview(&self.progress_label);

        // - progress bar
        self.progress_bar.set_value(0.);
        self.content.add_subview(&self.progress_bar);

        // - window configuration
        window.set_title(UI_APP_NAME);
        window.set_minimum_content_size(400., 100.);
        window.set_content_size(400., 100.);
        window.set_content_view(&self.content);

        // - layout constraints
        LayoutConstraint::activate(&[
            self.descr_label.top.constraint_equal_to(&self.content.safe_layout_guide.top),
            self.descr_label.center_x.constraint_equal_to(&self.content.safe_layout_guide.center_x),

            self.progress_label.top.constraint_equal_to(&self.descr_label.bottom).offset(10.),
            self.progress_label.leading.constraint_equal_to(&self.content.safe_layout_guide.leading),

            self.progress_bar.top.constraint_equal_to(&self.progress_label.bottom).offset(10.),
            self.progress_bar.leading.constraint_equal_to(&self.content.safe_layout_guide.leading),
            self.progress_bar.trailing.constraint_equal_to(&self.content.safe_layout_guide.trailing),
            self.progress_bar.bottom.constraint_equal_to(&self.content.safe_layout_guide.bottom),
        ]);
    }
}