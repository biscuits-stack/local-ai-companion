#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR},
    UI::WindowsAndMessaging::{FindWindowW, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE},
};

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSApplication, NSEvent, NSWindow};
#[cfg(target_os = "macos")]
use objc2_foundation::MainThreadMarker;

pub struct WindowHitTestManager {
    #[cfg(target_os = "windows")]
    hwnd: Option<HWND>,
    #[cfg(target_os = "windows")]
    last_regions: Vec<[i32; 4]>,
    #[cfg(target_os = "macos")]
    last_ignores_mouse: Option<bool>,
    #[cfg(target_os = "macos")]
    configured_window: bool,
}

impl WindowHitTestManager {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            hwnd: None,
            #[cfg(target_os = "windows")]
            last_regions: Vec::new(),
            #[cfg(target_os = "macos")]
            last_ignores_mouse: None,
            #[cfg(target_os = "macos")]
            configured_window: false,
        }
    }

    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "windows")]
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

    #[cfg(target_os = "macos")]
    pub fn update_interactive_regions(
        &mut self,
        window_title: &str,
        rects: &[eframe::egui::Rect],
        _scale_factor: f32,
    ) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };

        let app = NSApplication::sharedApplication(mtm);
        let windows = app.windows();
        let count = windows.count();
        if count == 0 {
            return;
        }

        // Locate matching companion NSWindow by title, falling back to first window
        let mut target: Option<objc2::rc::Retained<NSWindow>> = None;
        for i in 0..count {
            let win = unsafe { windows.objectAtIndex(i) };
            if win.title().to_string() == window_title {
                target = Some(win);
                break;
            }
        }
        let window = target.unwrap_or_else(|| unsafe { windows.objectAtIndex(0) });

        if !self.configured_window {
            window.setHasShadow(false);
            window.setOpaque(false);
            self.configured_window = true;
        }

        let mouse = unsafe { NSEvent::mouseLocation() };
        let frame = window.frame();

        // Convert macOS screen coordinates (bottom-left origin, logical points)
        // into egui window-local coordinates (top-left origin, logical points).
        let local_x = (mouse.x - frame.origin.x) as f32;
        let local_y = (frame.origin.y + frame.size.height - mouse.y) as f32;
        let cursor_pos = eframe::egui::Pos2::new(local_x, local_y);

        let over_ui = rects.iter().any(|r| r.expand(2.0).contains(cursor_pos));
        let should_ignore = !over_ui;

        if self.last_ignores_mouse != Some(should_ignore) {
            window.setIgnoresMouseEvents(should_ignore);
            self.last_ignores_mouse = Some(should_ignore);
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub fn update_interactive_regions(
        &mut self,
        _window_title: &str,
        _rects: &[eframe::egui::Rect],
        _scale_factor: f32,
    ) {
    }
}
