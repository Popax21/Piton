use windows::Win32::{Foundation::{RECT, HWND, WPARAM, LPARAM}, Graphics::Gdi::HFONT, UI::WindowsAndMessaging::{MoveWindow, SetWindowPos, WM_SETFONT, SendMessageTimeoutA, SMTO_ERRORONEXIT, SMTO_NOTIMEOUTIFNOTHUNG, SWP_NOMOVE, SWP_NOZORDER, SWP_NOACTIVATE, GetWindowLongA, GWL_STYLE, WINDOW_STYLE, GetMenu, GWL_EXSTYLE, AdjustWindowRectEx, WINDOW_EX_STYLE}};

use super::{WinError, dpi::{DPIMetrics, DPIAwarenessContext, DPI_100P}};

pub trait LayoutRect {
    fn left(&self) -> i32;
    fn right(&self) -> i32;
    fn top(&self) -> i32;
    fn bottom(&self) -> i32;

    fn set_left(&mut self, val: i32);
    fn set_right(&mut self, val: i32);
    fn set_top(&mut self, val: i32);
    fn set_bottom(&mut self, val: i32);

    fn width(&self) -> i32 { self.right() - self.left() }
    fn height(&self) -> i32 { self.bottom() - self.top() }

    fn set_width(&mut self, val: i32) { self.set_right(self.left() + val); }
    fn set_height(&mut self, val: i32) { self.set_bottom(self.top() + val); }

    fn set_x(&mut self, val: i32) {
        let width = self.width();
        self.set_left(val);
        self.set_width(width);
    }
    fn set_y(&mut self, val: i32) {
        let height = self.height();
        self.set_top(val);
        self.set_height(height);
    }

    fn as_rect(&self) -> RECT {
        RECT { left: self.left(), right: self.right(), top: self.top(), bottom: self.bottom() }
    }
}

impl LayoutRect for RECT {
    fn left(&self) -> i32 { self.left }
    fn right(&self) -> i32 { self.right }
    fn top(&self) -> i32 { self.top }
    fn bottom(&self) -> i32 { self.bottom }

    fn set_left(&mut self, val: i32) { self.left = val; }
    fn set_right(&mut self, val: i32) { self.right = val; }
    fn set_top(&mut self, val: i32) { self.top = val; }
    fn set_bottom(&mut self, val: i32) { self.bottom = val; }

    fn as_rect(&self) -> RECT { *self }
}

pub struct LayoutParams {
    pub dpi_awareness: DPIAwarenessContext,
    pub dpi: (i32, i32),
    pub initial_dpi: (i32, i32),

    pub h_scale: i32,
    pub h_scale_div: i32,
    pub v_scale: i32,
    pub v_scale_div: i32
}

#[allow(unused)]
impl LayoutParams {
    pub const fn scale_h(&self, val: i32) -> i32 { val * (self.dpi.0 * self.h_scale) / (DPI_100P * self.h_scale_div) }
    pub const fn scale_v(&self, val: i32) -> i32 { val * (self.dpi.1 * self.v_scale) / (DPI_100P * self.v_scale_div) }
    pub const fn scale_rect(&self, rect: RECT) -> RECT {
        RECT {
            left: self.scale_h(rect.left),
            right: self.scale_h(rect.right),
            top: self.scale_v(rect.top),
            bottom: self.scale_v(rect.bottom)
        }
    }

    pub const fn inv_scale_h(&self, val: i32) -> i32 { val * (DPI_100P * self.h_scale_div) / (self.dpi.0 * self.h_scale) }
    pub const fn inv_scale_v(&self, val: i32) -> i32 { val * (DPI_100P * self.v_scale_div) / (self.dpi.1 * self.v_scale) }
    pub const fn inv_scale_rect(&self, rect: RECT) -> RECT {
        RECT {
            left: self.inv_scale_h(rect.left),
            right: self.inv_scale_h(rect.right),
            top: self.inv_scale_v(rect.top),
            bottom: self.inv_scale_v(rect.bottom)
        }
    }
}

#[derive(Debug, Default)]
pub struct WindowLayout {
    pub handle: HWND,
    pub width: i32,
    pub height: i32
}

impl WindowLayout {
    pub fn determine_adj_window_size(&self, params: &LayoutParams) -> Result<(i32, i32), WinError> {
        //Adjust the window rect to take the non-client area (=border, title bar, etc.) into account
        unsafe {
            let window_style = GetWindowLongA(self.handle, GWL_STYLE);
            if window_style == 0 { return Err(WinError::from_win32()); }

            let ext_style = GetWindowLongA(self.handle, GWL_EXSTYLE);
            if ext_style == 0 { return Err(WinError::from_win32()); }

            let has_menu = !GetMenu(self.handle).is_invalid();

            //Try to use the DPI-aware variant if we're using Per-Monitor v2 awareness
            if params.dpi_awareness == DPIAwarenessContext::PerMonitorAwareV2 {
                let mut rect = RECT {
                    left: 0, right: params.scale_h(self.width),
                    top: 0, bottom: params.scale_v(self.height)
                };

                DPIMetrics::adjust_window_rect(&mut rect, window_style as u32, has_menu, ext_style as u32, params.dpi.0)
                    .expect("AdjustWindowRectExForDpi must be available when using Per-Monitor v2 DPI awarenes")?;

                return Ok((rect.width(), rect.height()));
            }

            //Fall back to manual DPI handling
            //When using Per-Monitor v1 awareness, our NC area is locked to our initial DPI
            //AdjustWindowRectEx scales the NC area based on the system DPI, complicating things even more
            let system_dpi = match params.dpi_awareness {
                DPIAwarenessContext::Unaware | DPIAwarenessContext::UnwareGDIScaled => (DPI_100P, DPI_100P),
                _ => DPIMetrics::get_system_dpi()?
            };

            let nca_dpi = if params.dpi_awareness == DPIAwarenessContext::PerMonitorAware {
                params.initial_dpi
            } else {
                params.dpi
            };

            let mut rect = RECT {
                left: 0, right: params.scale_h(self.width * system_dpi.0) / nca_dpi.0,
                top: 0, bottom: params.scale_v(self.height * system_dpi.1) / nca_dpi.1
            };

            AdjustWindowRectEx(
                &mut rect,
                WINDOW_STYLE(window_style as u32),
                has_menu,
                WINDOW_EX_STYLE(ext_style as u32)
            )?;

            Ok((rect.width() * nca_dpi.0 / system_dpi.0, rect.height() * nca_dpi.1 / system_dpi.1))
        }

    }

    pub fn apply(&self, params: &LayoutParams) -> Result<(), WinError> {
        let (adj_width, adj_height) = self.determine_adj_window_size(params)?;

        //Resize the window
        unsafe {
            SetWindowPos(
                self.handle,
                HWND::default(),
                0, 0,
                adj_width, adj_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE
            )
        }
    }
}

#[derive(Default, Debug)]
pub struct ComponentLayout {
    pub handle: HWND,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32
}

impl LayoutRect for ComponentLayout {
    fn left(&self) -> i32 { self.x }
    fn right(&self) -> i32 { self.x + self.width }
    fn top(&self) -> i32 { self.y }
    fn bottom(&self) -> i32 { self.y + self.height }

    fn set_left(&mut self, val: i32) {
        self.width = self.right() - val;
        self.x = val;
    }
    fn set_right(&mut self, val: i32) {
        self.width = val - self.left();
        self.x = val - self.width;
    }
    fn set_top(&mut self, val: i32) {
        self.height = self.bottom() - val;
        self.y = val;
    }
    fn set_bottom(&mut self, val: i32) {
        self.height = val - self.top();
        self.y = val - self.height;
    }

    fn width(&self) -> i32 { self.width }
    fn height(&self) -> i32 { self.height }

    fn set_width(&mut self, val: i32) { self.width = val; }
    fn set_height(&mut self, val: i32) { self.height = val; }

    fn set_x(&mut self, val: i32) { self.x = val; }
    fn set_y(&mut self, val: i32) { self.y = val; }
}

impl ComponentLayout {
    pub fn apply(&self, params: &LayoutParams) -> Result<(), WinError> {
        //Take scaling into account
        let scaled_rect = params.scale_rect(self.as_rect());

        //Update the component window
        unsafe {
            MoveWindow(self.handle, scaled_rect.left(), scaled_rect.top(), scaled_rect.width(), scaled_rect.height(), true)
        }
    }

    pub fn set_font(&self, font: HFONT) -> Result<(), WinError> {
        //Send a WM_SETFONT message
        unsafe {
            if SendMessageTimeoutA(self.handle, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(true as isize), SMTO_ERRORONEXIT | SMTO_NOTIMEOUTIFNOTHUNG, 10000, None).0 == 0 {
                return Err(WinError::from_win32());
            }
        }

        Ok(())
    }
}