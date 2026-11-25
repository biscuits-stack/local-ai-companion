use std::io::Cursor;
use image::{ImageBuffer, Rgb};

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    },
    UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

pub fn capture_screen_base64() -> Result<String, String> {
    #[cfg(windows)]
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);

        if width <= 0 || height <= 0 {
            return Err("Invalid screen dimensions".to_string());
        }

        let h_wnd: HWND = std::ptr::null_mut();
        let h_dc = GetDC(h_wnd);
        if h_dc.is_null() {
            return Err("Failed to get desktop DC".to_string());
        }

        let mem_dc = CreateCompatibleDC(h_dc);
        if mem_dc.is_null() {
            ReleaseDC(h_wnd, h_dc);
            return Err("Failed to create compatible DC".to_string());
        }

        let h_bitmap = CreateCompatibleBitmap(h_dc, width, height);
        if h_bitmap.is_null() {
            DeleteDC(mem_dc);
            ReleaseDC(h_wnd, h_dc);
            return Err("Failed to create bitmap".to_string());
        }

        let old_obj = SelectObject(mem_dc, h_bitmap);
        let blt_res = BitBlt(mem_dc, 0, 0, width, height, h_dc, 0, 0, SRCCOPY);

        if blt_res == 0 {
            SelectObject(mem_dc, old_obj);
            DeleteObject(h_bitmap);
            DeleteDC(mem_dc);
            ReleaseDC(h_wnd, h_dc);
            return Err("BitBlt failed".to_string());
        }

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width;
        bmi.bmiHeader.biHeight = -height; // Top-down DIB
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let buf_size = (width as usize) * (height as usize) * 4;
        let mut raw_bytes = vec![0u8; buf_size];

        let get_di_res = GetDIBits(
            mem_dc,
            h_bitmap,
            0,
            height as u32,
            raw_bytes.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(mem_dc, old_obj);
        DeleteObject(h_bitmap);
        DeleteDC(mem_dc);
        ReleaseDC(h_wnd, h_dc);

        if get_di_res == 0 {
            return Err("GetDIBits failed".to_string());
        }

        // Convert BGRA to RGB ImageBuffer
        let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> =
            ImageBuffer::new(width as u32, height as u32);

        for y in 0..height as u32 {
            for x in 0..width as u32 {
                let idx = ((y * (width as u32) + x) * 4) as usize;
                let b = raw_bytes[idx];
                let g = raw_bytes[idx + 1];
                let r = raw_bytes[idx + 2];
                img.put_pixel(x, y, Rgb([r, g, b]));
            }
        }

        // Scale down to max 1024x768 to keep prompt snappy and vision fast
        let (thumb_w, thumb_h) = if width > 1024 || height > 768 {
            let ratio = (1024.0 / width as f32).min(768.0 / height as f32);
            ((width as f32 * ratio) as u32, (height as f32 * ratio) as u32)
        } else {
            (width as u32, height as u32)
        };

        let resized = image::imageops::resize(
            &img,
            thumb_w,
            thumb_h,
            image::imageops::FilterType::Triangle,
        );

        let mut jpeg_bytes = Vec::new();
        let mut cursor = Cursor::new(&mut jpeg_bytes);

        resized
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .map_err(|e| format!("Failed to encode JPEG: {e}"))?;

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
        Ok(b64)
    }

    #[cfg(not(windows))]
    {
        Err("Screen capture only implemented on Windows".to_string())
    }
}
