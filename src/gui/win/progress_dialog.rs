use std::{ops::Deref, sync::{OnceLock, Mutex}, mem::{self}, error::Error, borrow::Cow, process::abort, ffi::c_void, thread};

use windows::{Win32::{UI::{Controls::{INITCOMMONCONTROLSEX, InitCommonControlsEx, ICC_PROGRESS_CLASS, PROGRESS_CLASS, PBM_SETPOS, PBM_SETRANGE}, WindowsAndMessaging::{WS_CAPTION, WS_POPUP, WS_SYSMENU, DS_MODALFRAME, DialogBoxIndirectParamA, WS_VISIBLE, WS_CHILD, GetDialogBaseUnits, GetSystemMetrics, SM_CYVSCROLL, WM_CLOSE, EndDialog, WM_INITDIALOG, SetWindowPos, SWP_NOZORDER, GetWindowRect, GetDesktopWindow, SWP_NOSIZE, SWP_NOACTIVATE, WM_GETDPISCALEDSIZE, WM_DPICHANGED, WINDOW_LONG_PTR_INDEX, SetWindowLongPtrW, DLGPROC, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, MSG, PeekMessageW, PM_REMOVE, GetWindowLongPtrW, GetDlgItem, WS_EX_COMPOSITED, SetTimer, WM_TIMER, SetWindowTextW, SendMessageA}}, System::{LibraryLoader::GetModuleHandleA, SystemServices::{SS_LEFT, SS_CENTER}}, Foundation::{LPARAM, WPARAM, HWND, RECT, SIZE, SetLastError, ERROR_SUCCESS, GetLastError, LRESULT}, Graphics::Gdi::{GetDC, ReleaseDC, DT_CALCRECT, DT_WORDBREAK, DrawTextW, HDC, RedrawWindow, HRGN, RDW_INVALIDATE, RDW_FRAME, RDW_ERASE, HFONT, DeleteObject, LOGFONTW, CreateFontIndirectW, SelectObject, HGDIOBJ, InvalidateRect}}, core::{PCSTR, HSTRING}};

use crate::gui::{GUI_APP_NAME, win::{dialog_template::{build_dialog_template, DialogControl, DialogControlTitle, WindowClass}, dpi::{DPIAwarenessOverride, DPIAwarenessContext, DialogDPIChangeBehaviors}, WinError}};

use super::{layout::{ComponentLayout, WindowLayout, LayoutParams, LayoutRect}, dpi::DPIMetrics};

const IDC_DESCR_LABEL: u16 = 1;
const IDC_PROGRESS_LABEL: u16 = 2;
const IDC_PROGRESS_BAR: u16 = 3;

#[derive(Default)]
struct ProgressState {
    done: bool,
    cancelled: bool,

    dirty: bool,
    text: String,
    fract: f64
}

struct DialogWindow<'a> {
    state: &'a Mutex<ProgressState>,
    done_delay: i32,

    dpi_override: DPIAwarenessOverride,
    inital_dpi: (i32, i32),

    h_base: i32,
    v_base: i32,
    dialog_font: FontHolder,
    window_layout: WindowLayout,

    descr_text: &'a str,
    descr_label: ComponentLayout,
    progress_label: ComponentLayout,
    progress_bar: ComponentLayout
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
    //Init common controls
    static CONTROL_INIT: OnceLock<bool> = OnceLock::new();
    if !CONTROL_INIT.get_or_init(|| {
        unsafe {
            InitCommonControlsEx(&INITCOMMONCONTROLSEX{
                dwSize: mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS
            }).as_bool()
        }
    }) {
        return Err("Failed to initialize common controls")?;
    }

    //Create a DPI awareness override
    let mut dpi_override: Option<DPIAwarenessOverride> = Option::None;
    for ctx in [
        DPIAwarenessContext::PerMonitorAwareV2,
        DPIAwarenessContext::PerMonitorAware,
        DPIAwarenessContext::SystemAware,
        DPIAwarenessContext::UnwareGDIScaled,
        DPIAwarenessContext::Unaware
    ] {
        //Skip Per-Monitor v2 if we can't override the dialog manager
        if ctx == DPIAwarenessContext::PerMonitorAwareV2 && !DialogDPIChangeBehaviors::has_binding() {
            continue;
        }

        //Try to apply an override
        dpi_override = DPIAwarenessOverride::try_apply(ctx);
        if dpi_override.is_some() { break; }
    }
    let dpi_override = dpi_override.expect("failed to apply DPI awareness override");

    //Build the dialog box template
    let mut diag_template_buf = [0_u8; 1024];
    let diag_template = build_dialog_template(
        &mut diag_template_buf,
        WindowClass::None,
        GUI_APP_NAME,
        (WS_POPUP | WS_CAPTION | WS_SYSMENU).0 | DS_MODALFRAME as u32,
        WS_EX_COMPOSITED.0, //Double buffered
        (0, 0),
        None,
        &[
            DialogControl{
                id: IDC_DESCR_LABEL,
                class: WindowClass::Atom(0x0082), //static
                style: (WS_VISIBLE | WS_CHILD).0 | SS_CENTER.0,
                ext_style: 0, pos: (0, 0), size: (0, 0),
                title: DialogControlTitle::Text(Cow::Borrowed(descr)),
                creation_data: None
            },
            DialogControl{
                id: IDC_PROGRESS_LABEL,
                class: WindowClass::Atom(0x0082), //static
                style: (WS_VISIBLE | WS_CHILD).0 | SS_LEFT.0,
                ext_style: 0, pos: (0, 0), size: (0, 0),
                title: DialogControlTitle::None,
                creation_data: None
            },
            DialogControl{
                id: IDC_PROGRESS_BAR,
                class: WindowClass::try_from(PROGRESS_CLASS)?,
                style: (WS_VISIBLE | WS_CHILD).0,
                ext_style: 0, pos: (0, 0), size: (0, 0),
                title: DialogControlTitle::None,
                creation_data: None
            }
        ]
    );

    //Setup the progress state
    let prog_state = &Mutex::new(ProgressState::default());

    let ret = thread::scope::<'_, _, Result<Option<T>, Box<dyn Error>>>(move |scope| {
        //Start the worker thread
        let work_thread: thread::ScopedJoinHandle<Option<T>> = scope.spawn(move || {
            //Setup the poison pill which sets the done flag upon exit
            struct PoisonPill<'a>(&'a Mutex<ProgressState>);
            impl Drop for PoisonPill<'_> {
                fn drop(&mut self) {
                    self.0.lock().unwrap().done = true;
                }
            }
            let _pill = PoisonPill(&prog_state);

            //Run the action
            let ret = action(&ProgressDialog {
                state: &prog_state
            });

            if !prog_state.lock().unwrap().cancelled {
                Some(ret)
            } else {
                None
            }
        });

        //Create and run the dialog        
        unsafe {
            let mut diag_window: DialogWindow = DialogWindow::new(dpi_override, &prog_state, descr);
            match DialogBoxIndirectParamA(
                GetModuleHandleA(PCSTR::null())?,
                diag_template,
                None,
                Some(progress_dialog_proc),
                LPARAM(&mut diag_window as *mut _ as isize)
            ) {
                0 | -1 => return Err(Box::new(WinError::from_win32())),
                _ => {}
            }
        }

        //Set the cancel flag
        prog_state.lock().unwrap().cancelled = true;

        //Wait for the worker thread to exit
        match work_thread.join() {
            Ok(t) => Ok(t),
            Err(e) => std::panic::resume_unwind(e)
        }
    });

    //Drain the message queue
    unsafe {
        let mut msg: MSG = MSG::default();
        while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {}
    }

    Ok(ret?)
}

extern "system" fn progress_dialog_proc(dialog_wndw: HWND, msg: u32, _msg_param1: WPARAM, _msg_param2: LPARAM) -> isize {
    const DWLP_USER: WINDOW_LONG_PTR_INDEX = WINDOW_LONG_PTR_INDEX((mem::size_of::<LRESULT>() + mem::size_of::<DLGPROC>()) as i32);
    macro_rules! get_dialog_ref {
        ($window:expr) => {
            unsafe {
                let user_data: isize = GetWindowLongPtrW($window, DWLP_USER);
                (user_data as *mut DialogWindow).as_mut().expect("dialog window has no ProgressDialog pointer attached")
            }
        };
    }

    match std::panic::catch_unwind(|| {
        match msg {
            WM_INITDIALOG => {
                //Set the progress dialog pointer
                unsafe {
                    SetLastError(ERROR_SUCCESS);
                    if SetWindowLongPtrW(dialog_wndw, DWLP_USER, _msg_param2.0) == 0 {
                        GetLastError().expect("failed to set dialog ProgressDialog pointer");
                    }
                }
                let prog_diag = get_dialog_ref!(dialog_wndw);

                //Obtain control handles
                macro_rules! get_control_handle {
                    ($id:expr) => {
                        unsafe {
                            let handle = GetDlgItem(dialog_wndw, $id as i32);
                            if handle == HWND::default() { panic!("failed to obtain dialog control handle"); }
                            handle
                        }
                    };
                }

                prog_diag.window_layout.handle = dialog_wndw;
                prog_diag.descr_label.handle = get_control_handle!(IDC_DESCR_LABEL);
                prog_diag.progress_label.handle = get_control_handle!(IDC_PROGRESS_LABEL);
                prog_diag.progress_bar.handle = get_control_handle!(IDC_PROGRESS_BAR);

                prog_diag.init().expect("failed to initialize dialog layout");

                //If we are using Per-Monitor v2 DPI awareness, disable default dialog resizing
                if prog_diag.dpi_override.get_awareness() == DPIAwarenessContext::PerMonitorAwareV2 {
                    DialogDPIChangeBehaviors::set_for(
                        dialog_wndw,
                        DialogDPIChangeBehaviors::ALL,
                        DialogDPIChangeBehaviors::ALL
                    ).expect("failed to set dialog DPI change behavior");
                }

                //Note: we don't bother with EnableNonClientDpiScaling since if it is available, we have access to v2 anyway
                //Also calling it would require our own dialog window subclass for the template, so... eh

                //Update and apply the window
                let dpi = prog_diag.dpi_override.get_current_dpi(dialog_wndw).expect("failed to obtain initial DPI setting");
                prog_diag.update_and_apply(dpi).expect("failed to update / apply dialog layout");

                //Center the window on-screen
                center_window(dialog_wndw).expect("failed to center dialog window");

                //Set a timer to periodically check the progress state
                unsafe {
                    if SetTimer(dialog_wndw, 0, 1000 / 60, None) == 0 {
                        panic!("failed to set timer for dialog window: {:?}", WinError::from_win32());
                    }
                };

                1
            }
            WM_TIMER => {
                let prog_diag = get_dialog_ref!(dialog_wndw);
                let mut prog_state = prog_diag.state.lock().unwrap();

                //Check if the progress state was modified
                //If yes, update controls and invalidate the window
                if prog_state.dirty {
                    unsafe {
                        //Update the progress label
                        SetWindowTextW(prog_diag.progress_label.handle, &HSTRING::from(&prog_state.text)).expect("failed to set progress label text");

                        //Update the progress bar
                        //Top MS design quality here: the bar will "smoothly animate" (=lag behind)
                        //To bypass this, set the position to state+1 first, then decrement to state, which is instant
                        //We have to have a special case for when we hit 100% as well, as we need to temporarily extend the range in that case
                        let val = (prog_state.fract * 100_f64) as usize;
                        if val < 100 {
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETPOS, WPARAM(val+1), LPARAM::default());
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETPOS, WPARAM(val), LPARAM::default());
                        } else {
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETRANGE, WPARAM::default(), LPARAM(101 << 16));
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETPOS, WPARAM(101), LPARAM::default());
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETPOS, WPARAM(100), LPARAM::default());
                            SendMessageA(prog_diag.progress_bar.handle, PBM_SETRANGE, WPARAM::default(), LPARAM(100 << 16));
                        }

                        //Invalidate the window
                        InvalidateRect(dialog_wndw, None, true).expect("failed to invalidate the dialog window");
                    }
                    prog_state.dirty = false;
                }

                //Check if the worker thread has exited
                //If yes, end the dialog
                if prog_state.done {
                    //Wait a bit (0.1s) so that it looks less abrupt
                    if prog_diag.done_delay >= 6 {
                        unsafe {
                            EndDialog(dialog_wndw, 1).expect("failed to end progress dialog");
                        }
                    } else {
                        prog_diag.done_delay += 1;   
                    }
                }

                1
            }
            WM_GETDPISCALEDSIZE => {
                let prog_diag = get_dialog_ref!(dialog_wndw);
                let dpi: i32 = _msg_param1.0 as i32;

                //Update the layout, but don't apply it
                let params = prog_diag.calc_layout_params((dpi, dpi));
                prog_diag.update_font((dpi, dpi)).expect("failed to update dialog font");
                prog_diag.update(&params).expect("failed to update dialog layout");

                //Output the size
                let (window_width, window_height) = prog_diag.window_layout.determine_adj_window_size(
                    &prog_diag.calc_layout_params((dpi, dpi))
                ).expect("failed to calculate adjusted window size on DPI change");

                unsafe {
                    *(_msg_param2.0 as *mut SIZE) = SIZE { cx: window_width, cy: window_height };
                }

                1
            }
            WM_DPICHANGED => {
                let prog_diag = get_dialog_ref!(dialog_wndw);

                //Update the layout and apply it
                let dpi = ((_msg_param1.0 & 0xffff) as i32, ((_msg_param1.0 >> 16) & 0xffff) as i32);
                prog_diag.update_and_apply(dpi).expect("failed to update / apply dialog layout");

                //Redraw the window
                unsafe {
                    RedrawWindow(
                        dialog_wndw,
                        None,
                        HRGN::default(),
                        RDW_ERASE | RDW_FRAME | RDW_INVALIDATE
                    ).expect("failed to redraw the dialog window on DPI change");
                }

                1
            }
            WM_CLOSE => {
                //End the dialog
                unsafe {
                    EndDialog(dialog_wndw, 1).expect("failed to cancel progress dialog");
                }

                1
            }
            _ => 0
        }
    }) {
        Ok(r) => r,
        Err(_) => abort()
    }
}

fn center_window(window_handle: HWND) -> Result<(), WinError> {
    unsafe {
        //Obtain the window and desktop rects
        let mut window_rect = RECT::default();
        let mut desktop_rect = RECT::default();
        GetWindowRect(window_handle, &mut window_rect)?;
        GetWindowRect(GetDesktopWindow(), &mut desktop_rect)?;

        //Calculate center coordinates
        let window_center_x = window_rect.left + (window_rect.right - window_rect.left) / 2;
        let window_center_y = window_rect.top + (window_rect.bottom - window_rect.top) / 2;

        let desktop_center_x = desktop_rect.left + (desktop_rect.right - desktop_rect.left) / 2;
        let desktop_center_y = desktop_rect.top + (desktop_rect.bottom - desktop_rect.top) / 2;

        //Set the window position
        SetWindowPos(
            window_handle,
            HWND::default(),
            desktop_center_x - window_center_x, desktop_center_y - window_center_y,
            0, 0,
            SWP_NOZORDER | SWP_NOSIZE | SWP_NOACTIVATE
        )?;
    }

    Ok(())
}

impl<'a> DialogWindow<'a> {
    const DIALOG_WIDTH: i32 = 180;

    fn new(dpi_override: DPIAwarenessOverride, state: &'a Mutex<ProgressState>, descr_text: &'a str) -> DialogWindow<'a> {
        DialogWindow {
            state,
            done_delay: 0,

            dpi_override,
            inital_dpi: (0, 0),

            h_base: 0,
            v_base: 0,
            dialog_font: FontHolder::default(),
            window_layout: WindowLayout::default(),

            descr_text: descr_text,
            descr_label: ComponentLayout::default(),
            progress_label: ComponentLayout::default(),
            progress_bar: ComponentLayout::default(),
        }
    }

    const BASE_UNIT_DIVS: (i32, i32) = (4, 8);
    fn init(&mut self) -> Result<(), WinError> {
        //Initialize the base units
        let base = unsafe { GetDialogBaseUnits() } as u32;
        (self.h_base, self.v_base) = ((base & 0xffff) as i32, (base >> 16) as i32);

        //Query the initial DPI
        self.inital_dpi = self.dpi_override.get_current_dpi(self.window_layout.handle)?;

        Ok(())
    }

    fn calc_layout_params(&self, dpi: (i32, i32)) -> LayoutParams {
        LayoutParams {
            dpi_awareness: self.dpi_override.get_awareness(),
            dpi: dpi,
            initial_dpi: self.inital_dpi,

            h_scale: self.h_base,
            h_scale_div: DialogWindow::BASE_UNIT_DIVS.0,
            v_scale: self.v_base,
            v_scale_div: DialogWindow::BASE_UNIT_DIVS.1
        }
    }

    fn update_and_apply(&mut self, dpi: (i32, i32)) -> Result<(), WinError> {
        self.update_font(dpi)?;
        self.update(&self.calc_layout_params(dpi))?;
        self.apply_font()?;
        self.apply(dpi)?;
        Ok(())
    }

    fn apply(&self, dpi: (i32, i32)) -> Result<(), WinError> {
        //Apply layouts
        let params = &self.calc_layout_params(dpi);
        self.descr_label.apply(params)?;
        self.progress_label.apply(params)?;
        self.progress_bar.apply(params)?;
        self.window_layout.apply(params)?;

        Ok(())
    }

    fn apply_font(&self) -> Result<(), WinError> {
        //Update control fonts
        self.descr_label.set_font(self.dialog_font.0)?;
        self.progress_label.set_font(self.dialog_font.0)?;
        Ok(())
    }

    fn update(&mut self, params: &LayoutParams) -> Result<(), WinError> {
        //Update the description label layout
        self.descr_label.x = 5;
        self.descr_label.y = 5;
        self.descr_label.width = DialogWindow::DIALOG_WIDTH - 10;
        self.descr_label.height = params.inv_scale_h(self.measure_text_height(params.scale_h(self.descr_label.width), self.descr_text)?);

        //Update the progress label layout
        //FIXME use a proper method to determine the height of a single line of text
        self.progress_label.x = 5;
        self.progress_label.y = self.descr_label.bottom() + 5;
        self.progress_label.width = DialogWindow::DIALOG_WIDTH - 10;
        self.progress_label.height = params.inv_scale_h(self.measure_text_height(params.scale_h(self.progress_label.width), "dummy")?);

        //Update the progress bar layout
        self.progress_bar.x = 5;
        self.progress_bar.y = self.progress_label.bottom() + 3;
        self.progress_bar.width = DialogWindow::DIALOG_WIDTH - 10;
        self.progress_bar.height = (unsafe { GetSystemMetrics(SM_CYVSCROLL) } * DialogWindow::BASE_UNIT_DIVS.1) / self.v_base;

        //Give labels breathing room below
        self.descr_label.set_bottom(self.descr_label.bottom() + 2);
        self.progress_label.set_bottom(self.progress_label.bottom() + 2);

        //Update the window layout
        self.window_layout.width = DialogWindow::DIALOG_WIDTH;
        self.window_layout.height = self.progress_bar.bottom() + 5;

        Ok(())
    }

    fn update_font(&mut self, dpi: (i32, i32)) -> Result<(), WinError> {
        //Query the non-client area properties
        let mut nc_metrics: NONCLIENTMETRICSW = NONCLIENTMETRICSW::default();
        nc_metrics.cbSize = mem::size_of::<NONCLIENTMETRICSW>() as u32;

        if let Some(res) = unsafe {
            DPIMetrics::get_system_parameters_info(SPI_GETNONCLIENTMETRICS, nc_metrics.cbSize, &mut nc_metrics as *mut _ as usize, dpi.0)
        } {
            //We succesfully managed to query the DPI-specific metrics
            res?;
        } else {
            //Fall back to querying the non-DPI-specific metrics
            unsafe {
                SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, nc_metrics.cbSize, Some(&mut nc_metrics as *mut _ as *mut c_void), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS::default())?;
            };

            //Apply DPI scaling manually
            let system_dpi = DPIMetrics::get_system_dpi()?;
            nc_metrics.lfMessageFont.lfWidth = (nc_metrics.lfMessageFont.lfWidth * dpi.0) / system_dpi.0;
            nc_metrics.lfMessageFont.lfHeight = (nc_metrics.lfMessageFont.lfHeight * dpi.1) / system_dpi.1;
        }

        //Create the new font
        self.dialog_font = FontHolder::create_indirect(&nc_metrics.lfMessageFont)?;

        Ok(())
    }

    fn measure_text_height(&self, width: i32, text: &str) -> Result<i32, WinError> {
        unsafe {
            //Acquire a DC
            let dc = GetDC(self.window_layout.handle);
            if dc.is_invalid() { return Err(WinError::from_win32()); }
            let _dc_dropper = DCDropper(self.window_layout.handle, dc);

            //Set the font
            let mut _font_resetter: Option<FontResetter> = None;
            if self.dialog_font.has_font() {
                let prev_font = SelectObject(dc, self.dialog_font.0);
                if prev_font.is_invalid() { return Err(WinError::from_win32()); }
                _font_resetter = Some(FontResetter(dc, prev_font));
            }

            //Draw the text to calculate its rectangle
            let mut rect: RECT = RECT { left: 0, right: width, top: 0, bottom: 0 };
            if DrawTextW(dc, &mut text.encode_utf16().collect::<Vec<u16>>(), &mut rect, DT_CALCRECT | DT_WORDBREAK) == 0 {
                return Err(WinError::from_win32());
            }

            Ok(rect.bottom - rect.top)
        }
    }
}

#[derive(Default)]
struct FontHolder(HFONT);

impl FontHolder {
    fn create_indirect(logfont: &LOGFONTW) -> Result<FontHolder, WinError> {
        unsafe {
            let font = CreateFontIndirectW(logfont);
            if !font.is_invalid() {
                Ok(FontHolder(font))
            } else {
                Err(WinError::from_win32())
            }
        }
    }

    fn has_font(&self) -> bool { !self.0.is_invalid() }
}

impl Drop for FontHolder {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_invalid() && !DeleteObject(self.0).as_bool() {
                panic!("failed to free HFONT handle: {:?}", WinError::from_win32());
            }
        }
    }
}

struct DCDropper(HWND, HDC);
impl Drop for DCDropper {
    fn drop(&mut self) {
        if unsafe { ReleaseDC(self.0, self.1) } != 1 {
            panic!("failed to release DC: {:?}", WinError::from_win32());
        }
    }
}

struct FontResetter(HDC, HGDIOBJ);
impl Drop for FontResetter {
    fn drop(&mut self) {
        if unsafe { SelectObject(self.0, self.1) }.is_invalid() {
            panic!("failed to reset DC font: {:?}", WinError::from_win32());
        }
    }
}