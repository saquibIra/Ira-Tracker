//! Screenshot for auto-tracking: one display, downscaled by the OS while capturing
//! (no full-resolution buffer), converted to grayscale and JPEG-encoded in memory,
//! plus a small thumbnail for the day review. Storage (on this computer only, for the
//! retention period) is handled by `shots.rs`.

/// A captured screen: the JPEG sent to the AI / shown in the review, and a thumbnail.
pub struct Shot {
    pub jpeg: Vec<u8>,
    pub thumb: Vec<u8>,
}

/// Thumbnail width in pixels for the review strip.
pub const THUMB_WIDTH: usize = 240;

/// Output size with the longest edge at `max_px`, never more than 2x the size in
/// points (avoids upscaling small displays). Both edges stay even and at least 2.
pub fn target_size(width: f64, height: f64, max_px: u32) -> (u32, u32) {
    let long = width.max(height).max(1.0);
    let scale = (max_px as f64 / long).min(2.0);
    let w = ((width * scale).round() as u32).max(2) & !1;
    let h = ((height * scale).round() as u32).max(2) & !1;
    (w, h)
}

/// BGRA (or BGRX) pixels with `stride` bytes per row → 8-bit luma (ITU-R BT.601).
pub fn bgra_to_luma(px: &[u8], width: usize, height: usize, stride: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = &px[y * stride..y * stride + width * 4];
        for p in row.as_chunks::<4>().0 {
            let (b, g, r) = (p[0] as u32, p[1] as u32, p[2] as u32);
            out.push(((299 * r + 587 * g + 114 * b + 500) / 1000) as u8);
        }
    }
    out
}

/// Box-filter downscale of a luma image to `tw` pixels wide (keeps aspect ratio).
pub fn downscale(luma: &[u8], w: usize, h: usize, tw: usize) -> (Vec<u8>, usize, usize) {
    if w <= tw || w == 0 || h == 0 {
        return (luma.to_vec(), w, h);
    }
    let th = (h * tw / w).max(1);
    let mut out = vec![0u8; tw * th];
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let mut sum = 0u32;
            for y in y0..y1 {
                for &v in &luma[y * w + x0..y * w + x1] {
                    sum += v as u32;
                }
            }
            out[ty * tw + tx] = (sum / ((y1 - y0) * (x1 - x0)) as u32) as u8;
        }
    }
    (out, tw, th)
}

/// Full JPEG plus thumbnail from one luma buffer.
pub fn encode_shot(luma: &[u8], width: u32, height: u32, quality: u8) -> Result<Shot, String> {
    let jpeg = encode_jpeg(luma, width, height, quality)?;
    let (small, tw, th) = downscale(luma, width as usize, height as usize, THUMB_WIDTH);
    let thumb = encode_jpeg(&small, tw as u32, th as u32, 55)?;
    Ok(Shot { jpeg, thumb })
}

pub fn encode_jpeg(luma: &[u8], width: u32, height: u32, quality: u8) -> Result<Vec<u8>, String> {
    let w = u16::try_from(width).map_err(|_| "image too wide")?;
    let h = u16::try_from(height).map_err(|_| "image too tall")?;
    let mut out = Vec::with_capacity(luma.len() / 10);
    jpeg_encoder::Encoder::new(&mut out, quality.clamp(1, 100))
        .encode(luma, w, h, jpeg_encoder::ColorType::Luma)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{Shot, bgra_to_luma, encode_shot, target_size};
    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_core_foundation::{CGPoint, CGRect};
    use objc2_core_graphics::{CGDataProvider, CGImage, CGMainDisplayID};
    use objc2_foundation::{NSArray, NSError};
    use objc2_screen_capture_kit::{
        SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
    };
    use std::sync::mpsc;
    use std::time::Duration;

    type Reply = mpsc::Sender<Result<Shot, String>>;

    fn contains(r: CGRect, p: CGPoint) -> bool {
        p.x >= r.origin.x
            && p.y >= r.origin.y
            && p.x < r.origin.x + r.size.width
            && p.y < r.origin.y + r.size.height
    }

    fn pick_display(
        content: &SCShareableContent,
        near: Option<(f64, f64)>,
    ) -> Option<Retained<SCDisplay>> {
        // SAFETY: plain property reads on a valid object.
        let displays = unsafe { content.displays() };
        let all: Vec<Retained<SCDisplay>> = displays.iter().collect();
        if let Some((x, y)) = near
            && let Some(d) = all
                .iter()
                .find(|d| contains(unsafe { d.frame() }, CGPoint { x, y }))
        {
            return Some(d.clone());
        }
        let main = CGMainDisplayID();
        all.iter()
            .find(|d| unsafe { d.displayID() } == main)
            .or_else(|| all.first())
            .cloned()
    }

    fn image_to_shot(image: &CGImage, quality: u8) -> Result<Shot, String> {
        let w = CGImage::width(Some(image));
        let h = CGImage::height(Some(image));
        let stride = CGImage::bytes_per_row(Some(image));
        if CGImage::bits_per_pixel(Some(image)) != 32 || w == 0 || h == 0 {
            return Err("unexpected pixel format".into());
        }
        let provider = CGImage::data_provider(Some(image)).ok_or("no image data")?;
        let data = CGDataProvider::data(Some(&provider)).ok_or("no image data")?;
        // SAFETY: the CFData is immutable and outlives this borrow.
        let bytes = unsafe { data.as_bytes_unchecked() };
        if bytes.len() < stride * h {
            return Err("short image buffer".into());
        }
        let luma = bgra_to_luma(bytes, w, h, stride);
        encode_shot(&luma, w as u32, h as u32, quality)
    }

    fn shoot(
        content: &SCShareableContent,
        max_px: u32,
        quality: u8,
        near: Option<(f64, f64)>,
        tx: Reply,
    ) {
        let Some(display) = pick_display(content, near) else {
            let _ = tx.send(Err("no display".into()));
            return;
        };
        // SAFETY: ScreenCaptureKit objects are created and configured on this thread
        // and only passed to the screenshot API.
        unsafe {
            let (w, h) = target_size(display.width() as f64, display.height() as f64, max_px);
            let filter = SCContentFilter::initWithDisplay_excludingWindows(
                SCContentFilter::alloc(),
                &display,
                &NSArray::new(),
            );
            let config = SCStreamConfiguration::new();
            config.setWidth(w as usize);
            config.setHeight(h as usize);
            config.setShowsCursor(false);
            config.setPixelFormat(u32::from_be_bytes(*b"BGRA"));
            let done = RcBlock::new(move |image: *mut CGImage, err: *mut NSError| {
                let res = match image.as_ref() {
                    Some(img) => image_to_shot(img, quality),
                    None => Err(err
                        .as_ref()
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "capture failed".into())),
                };
                let _ = tx.send(res);
            });
            SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
                &filter,
                &config,
                Some(&done),
            );
        }
    }

    /// Blocking; call from a background thread. Needs macOS 14 and the Screen Recording
    /// permission.
    pub fn capture(max_px: u32, quality: u8, near: Option<(f64, f64)>) -> Result<Shot, String> {
        if objc2::runtime::AnyClass::get(c"SCScreenshotManager").is_none() {
            return Err("screenshots need macOS 14 or later".into());
        }
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
            // SAFETY: ScreenCaptureKit passes a valid object or null.
            match unsafe { content.as_ref() } {
                Some(c) => shoot(c, max_px, quality, near, tx.clone()),
                None => {
                    let msg = unsafe { err.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "screen recording not allowed".into());
                    let _ = tx.send(Err(msg));
                }
            }
        });
        // SAFETY: the block is retained by ScreenCaptureKit until it is called.
        unsafe {
            SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
                false, true, &block,
            );
        }
        rx.recv_timeout(Duration::from_secs(10))
            .map_err(|_| "screenshot timed out".to_string())?
    }
}

#[cfg(windows)]
mod imp {
    use super::{Shot, bgra_to_luma, encode_shot, target_size};
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, HALFTONE,
        MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
        SetBrushOrgEx, SetStretchBltMode, StretchBlt,
    };

    /// Captures the monitor under `near` (the foreground window's centre).
    pub fn capture(max_px: u32, quality: u8, near: Option<(f64, f64)>) -> Result<Shot, String> {
        // SAFETY: GDI calls with handles we create and release in this function.
        unsafe {
            let (x, y) = near.unwrap_or((0.0, 0.0));
            let mon = MonitorFromPoint(
                POINT {
                    x: x as i32,
                    y: y as i32,
                },
                MONITOR_DEFAULTTOPRIMARY,
            );
            let mut mi: MONITORINFO = std::mem::zeroed();
            mi.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
            if GetMonitorInfoW(mon, &mut mi) == 0 {
                return Err("no monitor".into());
            }
            let r = mi.rcMonitor;
            let (sw, sh) = (r.right - r.left, r.bottom - r.top);
            let (w, h) = target_size(sw as f64, sh as f64, max_px);
            let (w, h) = (w.min(sw as u32 & !1), h.min(sh as u32 & !1));

            let screen = GetDC(std::ptr::null_mut());
            if screen.is_null() {
                return Err("no screen DC".into());
            }
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w as i32, h as i32);
            let old = SelectObject(mem, bmp);
            SetStretchBltMode(mem, HALFTONE);
            SetBrushOrgEx(mem, 0, 0, std::ptr::null_mut());
            let copied = StretchBlt(
                mem, 0, 0, w as i32, h as i32, screen, r.left, r.top, sw, sh, SRCCOPY,
            );

            let mut info: BITMAPINFO = std::mem::zeroed();
            info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };
            let mut px = vec![0u8; (w * h * 4) as usize];
            let rows = GetDIBits(
                mem,
                bmp,
                0,
                h,
                px.as_mut_ptr().cast(),
                &mut info,
                DIB_RGB_COLORS,
            );

            SelectObject(mem, old);
            DeleteObject(bmp);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);

            if copied == 0 || rows != h as i32 {
                return Err("screen copy failed".into());
            }
            let luma = bgra_to_luma(&px, w as usize, h as usize, w as usize * 4);
            drop(px);
            encode_shot(&luma, w, h, quality)
        }
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    pub fn capture(_: u32, _: u8, _: Option<(f64, f64)>) -> Result<super::Shot, String> {
        Err("screenshots are not supported on this platform".into())
    }
}

pub use imp::capture;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_keeps_aspect_and_caps_long_edge() {
        assert_eq!(target_size(1512.0, 982.0, 1568), (1568, 1018));
        assert_eq!(target_size(3840.0, 2160.0, 1568), (1568, 882));
        assert_eq!(target_size(1080.0, 1920.0, 1568), (882, 1568));
        // small display: at most 2x its size in points
        assert_eq!(target_size(600.0, 400.0, 1568), (1200, 800));
    }

    #[test]
    fn luma_respects_stride_and_weights() {
        // 2x1 image, stride 12 (4 bytes padding): white, pure red
        let px = [255, 255, 255, 255, 0, 0, 255, 255, 9, 9, 9, 9];
        assert_eq!(bgra_to_luma(&px, 2, 1, 12), vec![255, 76]);
    }

    #[test]
    fn downscale_averages_blocks_and_keeps_aspect() {
        // 4x2 image: left half 0, right half 200 -> 2x1 thumbnail
        let luma = [0, 0, 200, 200, 0, 0, 200, 200];
        assert_eq!(downscale(&luma, 4, 2, 2), (vec![0, 200], 2, 1));
        let (_, w, h) = downscale(&vec![9u8; 1568 * 1018], 1568, 1018, THUMB_WIDTH);
        assert_eq!((w, h), (240, 155));
        assert_eq!(
            downscale(&[1, 2], 2, 1, 240),
            (vec![1, 2], 2, 1),
            "no upscaling"
        );
    }

    #[test]
    fn shot_has_full_image_and_thumbnail() {
        let (w, h) = (800u32, 500u32);
        let luma: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
        let shot = encode_shot(&luma, w, h, 60).unwrap();
        assert!(shot.jpeg.starts_with(&[0xFF, 0xD8]) && shot.thumb.starts_with(&[0xFF, 0xD8]));
        assert!(shot.thumb.len() < shot.jpeg.len());
    }

    #[test]
    fn jpeg_is_small_and_valid() {
        let (w, h) = (320u32, 200u32);
        let luma: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
        let jpg = encode_jpeg(&luma, w, h, 60).unwrap();
        assert!(jpg.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert!(jpg.len() < luma.len());
    }
}

/// Manual check on a real machine: `cargo test live_ -- --ignored --nocapture`.
/// Prints what the widget would see; the screenshot stays in memory.
///
/// Preview (testing only, never done by the app): set `TIMEPILOT_PREVIEW=/path/file.jpg` to
/// save the exact image the app would send, and `TIMEPILOT_PREVIEW_DELAY=5` to get time to
/// bring another window to the front first. The terminal running this needs the
/// Screen Recording permission.
#[cfg(test)]
mod live {
    #[test]
    #[ignore]
    fn live_context_and_capture() {
        let delay = std::env::var("TIMEPILOT_PREVIEW_DELAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0u64);
        if delay > 0 {
            println!("bring the window to test to the front; capturing in {delay} s…");
            std::thread::sleep(std::time::Duration::from_secs(delay));
        }
        let ctx = crate::context::active();
        println!("permission: {:?}", crate::context::screen_permission());
        println!("active window (sent as text): {ctx:?}");
        let started = std::time::Instant::now();
        match super::capture(1568, 60, ctx.and_then(|c| c.center)) {
            Ok(shot) => {
                println!(
                    "screenshot: {} KB (thumbnail {} KB) in {:?}",
                    shot.jpeg.len() / 1024,
                    shot.thumb.len() / 1024,
                    started.elapsed()
                );
                if let Ok(path) = std::env::var("TIMEPILOT_PREVIEW") {
                    std::fs::write(&path, &shot.jpeg).expect("write preview");
                    println!("preview saved to {path} (delete it when done)");
                }
            }
            Err(e) => println!("screenshot failed: {e}"),
        }
    }
}
