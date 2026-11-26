use std::io::Cursor;
use image::{ImageBuffer, Rgb};

#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    },
    UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

fn encode_rgb_to_jpeg_base64(img: &ImageBuffer<Rgb<u8>, Vec<u8>>) -> Result<String, String> {
    let width = img.width();
    let height = img.height();

    // Scale down to max 1024x768 to keep prompt snappy and vision fast
    let (thumb_w, thumb_h) = if width > 1024 || height > 768 {
        let ratio = (1024.0 / width as f32).min(768.0 / height as f32);
        ((width as f32 * ratio) as u32, (height as f32 * ratio) as u32)
    } else {
        (width, height)
    };

    let resized = image::imageops::resize(
        img,
        thumb_w.max(1),
        thumb_h.max(1),
        image::imageops::FilterType::Triangle,
    );

    let mut jpeg_bytes = Vec::new();
    let mut cursor = Cursor::new(&mut jpeg_bytes);

    resized
        .write_to(&mut cursor, image::ImageFormat::Jpeg)
        .map_err(|e| format!("Failed to encode JPEG: {e}"))?;

    use base64::Engine;
    Ok(base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes))
}

#[cfg(target_os = "windows")]
pub fn capture_screen_base64() -> Result<String, String> {
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

        encode_rgb_to_jpeg_base64(&img)
    }
}

#[cfg(target_os = "macos")]
pub fn capture_screen_base64() -> Result<String, String> {
    let display = core_graphics::display::CGDisplay::main();
    let cg_image = display
        .image()
        .ok_or_else(|| "CGDisplay::main().image() returned None (check Screen Recording permission in macOS System Settings)".to_string())?;

    let width = cg_image.width();
    let height = cg_image.height();
    let bytes_per_row = cg_image.bytes_per_row();
    let data = cg_image.data();
    let raw = data.bytes();

    if width == 0 || height == 0 || raw.is_empty() {
        return Err("Captured macOS screen image is empty".to_string());
    }

    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::new(width as u32, height as u32);

    for y in 0..height {
        let row_offset = y * bytes_per_row;
        for x in 0..width {
            let idx = row_offset + x * 4;
            if idx + 3 <= raw.len() {
                // CoreGraphics CGDisplay::image() provides 32-bit BGRA bytes
                let b = raw[idx];
                let g = raw[idx + 1];
                let r = raw[idx + 2];
                img.put_pixel(x as u32, y as u32, Rgb([r, g, b]));
            }
        }
    }

    encode_rgb_to_jpeg_base64(&img)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn capture_screen_base64() -> Result<String, String> {
    Err("Screen capture is currently implemented on Windows and macOS only".to_string())
}
