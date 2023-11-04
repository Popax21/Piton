use std::error::Error;
use std::sync::Mutex;
use cacao::appkit::{App, AppDelegate};
use cacao::appkit::menu::{Menu, MenuItem};
use cacao::appkit::window::{Window, WindowConfig, WindowDelegate};
use cacao::layout::{Layout, LayoutConstraint};
use cacao::progress::ProgressIndicator;
use cacao::text::{Label, TextAlign};
use cacao::view::View;
use dispatch::Queue;

use crate::ui::ProgressAction;
use crate::cfg::UI_APP_NAME;

struct BasicApp {
    window: Window<AppWindow>
}

//Implementation of NSApplicationDelegate
impl AppDelegate for BasicApp {
    fn did_finish_launching(&self) {
        //Define the app-level menu (required)
        App::set_menu(vec![
            Menu::new("", vec![
                MenuItem::Quit,
            ]),
        ]);

        //Bring the app to the foreground and show its window
        App::activate();
        self.window.show();
    }

    fn should_terminate_after_last_window_closed(&self) -> bool {
        true
    }
}

#[derive(Default)]
struct AppWindow {
    content: View,
    top_label: Label,
    bottom_label: Label,
    progress: ProgressIndicator,
}

impl AppWindow {
    //Updates the progress bar and label using the main dispatch queue
    fn apply_state(&mut self, state: ProgressState) {
        Queue::main().exec_async(|| {
            self.bottom_label.set_text(state.text);
            self.progress.set_value(state.fract);
        });
    }
}

//Implementation of NSWindowDelegate
impl WindowDelegate for AppWindow {
    const NAME: &'static str = "WindowDelegate";

    fn did_load(&mut self, window: Window) {
        // - window configuration
        window.set_title(UI_APP_NAME);
        window.set_minimum_content_size(400., 100.);
        window.set_content_size(400., 100.);

        // - top label
        self.top_label.set_text_alignment(TextAlign::Center);
        self.content.add_subview(&self.top_label);

        // - bottom label
        self.bottom_label.set_text("");
        self.bottom_label.set_text_alignment(TextAlign::Center);
        self.content.add_subview(&self.bottom_label);

        // - progress bar
        self.progress.set_value(0.);
        self.content.add_subview(&self.progress);

        // - content view
        window.set_content_view(&self.content);

        // - layout constraints
        LayoutConstraint::activate(&[
            self.top_label.top.constraint_equal_to(&self.content.safe_layout_guide.top),
            self.top_label.center_x.constraint_equal_to(&self.content.safe_layout_guide.center_x),
            self.bottom_label.top.constraint_equal_to(&self.top_label.bottom).offset(10.),
            self.bottom_label.center_x.constraint_equal_to(&self.content.safe_layout_guide.center_x),
            self.progress.top.constraint_equal_to(&self.bottom_label.bottom).offset(10.),
            self.progress.leading.constraint_equal_to(&self.content.safe_layout_guide.leading),
            self.progress.trailing.constraint_equal_to(&self.content.safe_layout_guide.trailing),
            self.progress.bottom.constraint_equal_to(&self.content.safe_layout_guide.bottom),
        ]);
    }
}

#[derive(Default)]
struct ProgressState {
    done: bool,
    cancelled: bool,

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
    }

    fn is_cancelled(&self) -> bool { self.state.lock().unwrap().cancelled }
}

pub fn run_progress_action<T: Send>(descr: &str, action: impl FnOnce(&MacOSProgressAction) -> T + Send) -> Result<Option<T>, Box<dyn Error>> {
    //Create a window
    let window = AppWindow::default();
    window.top_label.set_text(descr);

    //Create an app using the specified delegates
    //TODO: replace bundle id
    let app = App::new("com.hello.world", BasicApp {
        window: Window::with(WindowConfig::default(), window)
    });

    //TODO: worker thread etc. that calls window.apply_state(state) periodically

    //Block while the application runs
    app.run();

    Ok(None)
}
