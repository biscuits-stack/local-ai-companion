#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR,
    },
    UI::WindowsAndMessaging::{FindWindowW, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE},
};

pub struct WindowHitTestManager {
    #[cfg(windows)]
    hwnd: Option<HWND>,
    #[cfg(windows)]
    last_regions: Vec<[i32; 4]>,
}

impl WindowHitTestManager {
    pub fn new() -> Self {
        Self {
            #[cfg(windows)]
            hwnd: None,
            #[cfg(windows)]
            last_regions: Vec::new(),
        }
    }

    #[cfg(windows)]
    pub fn ensure_hwnd(&mut self, window_title: &str) -> Option<HWND> {
        if let Some(h) = self.hwnd {
            return Some(h);
        }

        let mut wide: Vec<u16> = window_title.encode_utf16().collect();
        wide.push(0);

        unsafe {
            let h = FindWindowW(std::ptr::null(), wide.as_ptr());
            if !h.is_null() {
                self.hwnd = Some(h);
                // Ensure topmost
                SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
                return Some(h);
            }
        }
        None
    }

    #[cfg(windows)]
    pub fn update_interactive_regions(
        &mut self,
        window_title: &str,
        rects: &[eframe::egui::Rect],
        scale_factor: f32,
    ) {
        let Some(hwnd) = self.ensure_hwnd(window_title) else {
            return;
        };

        if rects.is_empty() {
            return;
        }

        let mut current_regions: Vec<[i32; 4]> = Vec::with_capacity(rects.len());
        for r in rects {
            // Expand by +1.0 logical point so Win32 SetWindowRgn never slices off the right or bottom 1px border.
            // Flooring min and ceiling max ensures full physical pixel coverage across all DPI scaling tiers.
            let expanded = r.expand(1.0);
            let left = (expanded.min.x * scale_factor).floor() as i32;
            let top = (expanded.min.y * scale_factor).floor() as i32;
            let right = (expanded.max.x * scale_factor).ceil() as i32;
            let bottom = (expanded.max.y * scale_factor).ceil() as i32;

            if right > left && bottom > top {
                current_regions.push([left, top, right, bottom]);
            }
        }

        // Only call SetWindowRgn when physical regions change by >= 1 pixel.
        // This avoids continuous OS calls during stationary animation and prevents DWM background flashes.
        if current_regions == self.last_regions {
            return;
        }

        unsafe {
            let combined = CreateRectRgn(0, 0, 0, 0);

            for r in &current_regions {
                let rgn = CreateRectRgn(r[0], r[1], r[2], r[3]);
                CombineRgn(combined, combined, rgn, RGN_OR);
                DeleteObject(rgn);
            }

            // SetWindowRgn transfers ownership of `combined` to the operating system.
            // bRedraw = 0 (FALSE) ensures Windows does NOT erase/redraw with the default
            // window background brush, eliminating the white flash behind the shrimp overlay during drag.
            SetWindowRgn(hwnd, combined, 0);
        }

        self.last_regions = current_regions;
    }

    #[cfg(not(windows))]
    pub fn update_interactive_regions(
        &mut self,
        _window_title: &str,
        _rects: &[eframe::egui::Rect],
        _scale_factor: f32,
    ) {}
}
