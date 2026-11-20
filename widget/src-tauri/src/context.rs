//! Which app and window is in front. Read locally every 20 s while auto-tracking runs.
//!
//! macOS: CoreGraphics window list (front-to-back). The window title needs the Screen
//! Recording permission; without it only the app name is known.
//! Windows: the foreground window's title and process name.

/// The frontmost window. `center` is in global screen coordinates (to pick the display
/// to capture on multi-monitor setups).
#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub app: String,
    pub title: String,
    pub center: Option<(f64, f64)>,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::Active;
    use objc2_core_foundation::{
        CFArray, CFDictionary, CFIndex, CFNumber, CFNumberType, CFRetained, CFString, CFType,
        CGRect,
    };
    use objc2_core_graphics::{
        CGPreflightScreenCaptureAccess, CGRectMakeWithDictionaryRepresentation,
        CGRequestScreenCaptureAccess, CGWindowListCopyWindowInfo, CGWindowListOption,
        kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowName, kCGWindowOwnerName,
        kCGWindowOwnerPID,
    };

    type Info = CFDictionary<CFString, CFType>;

    fn number(d: &Info, key: &CFString) -> Option<i64> {
        // SAFETY: the dictionary is not mutated while we read it.
        let n = unsafe { d.get_unchecked(key) }?.downcast_ref::<CFNumber>()?;
        let mut out: i64 = 0;
        // SAFETY: `out` is a valid i64 for the SInt64 conversion.
        unsafe { n.value(CFNumberType::SInt64Type, (&mut out as *mut i64).cast()) }.then_some(out)
    }

    fn string(d: &Info, key: &CFString) -> Option<String> {
        // SAFETY: as above.
        let s = unsafe { d.get_unchecked(key) }?.downcast_ref::<CFString>()?;
        Some(s.to_string())
    }

    pub fn active() -> Option<Active> {
        let list = CGWindowListCopyWindowInfo(
            CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
            kCGNullWindowID,
        )?;
        // SAFETY: CGWindowListCopyWindowInfo returns an array of CFDictionary with CFString keys.
        let list: CFRetained<CFArray<Info>> = unsafe { CFRetained::cast_unchecked(list) };
        let me = std::process::id() as i64;
        // SAFETY: the keys are immutable CoreGraphics constants.
        let (k_layer, k_pid, k_owner, k_name, k_bounds) = unsafe {
            (
                kCGWindowLayer,
                kCGWindowOwnerPID,
                kCGWindowOwnerName,
                kCGWindowName,
                kCGWindowBounds,
            )
        };
        for i in 0..list.len() {
            // SAFETY: index in bounds; the array is ours and not mutated.
            let d = unsafe { list.get_unchecked(i as CFIndex) };
            // Normal app windows are on layer 0 (menu bar, Dock and our floating
            // windows are on higher layers).
            if number(d, k_layer) != Some(0) || number(d, k_pid) == Some(me) {
                continue;
            }
            let Some(app) = string(d, k_owner) else {
                continue;
            };
            let mut rect = CGRect::default();
            // SAFETY: the bounds value is a CGRect dictionary representation.
            let center = unsafe { d.get_unchecked(k_bounds) }
                .and_then(|b| b.downcast_ref::<CFDictionary>())
                .filter(|b| unsafe { CGRectMakeWithDictionaryRepresentation(Some(b), &mut rect) })
                .map(|_| {
                    (
                        rect.origin.x + rect.size.width / 2.0,
                        rect.origin.y + rect.size.height / 2.0,
                    )
                });
            if rect.size.width < 50.0 || rect.size.height < 50.0 {
                continue; // tooltips, invisible helper windows
            }
            return Some(Active {
                app,
                title: string(d, k_name).unwrap_or_default(),
                center,
            });
        }
        None
    }

    pub fn screen_permission() -> Permission {
        if CGPreflightScreenCaptureAccess() {
            Permission::Granted
        } else {
            Permission::Denied
        }
    }

    /// Shows the system prompt the first time; afterwards the user must enable the
    /// app in System Settings → Privacy & Security → Screen Recording.
    pub fn request_screen_permission() -> Permission {
        if CGRequestScreenCaptureAccess() {
            Permission::Granted
        } else {
            Permission::Denied
        }
    }

    use super::Permission;
}

#[cfg(windows)]
mod imp {
    use super::{Active, Permission};
    use windows_sys::Win32::Foundation::{CloseHandle, RECT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    };

    pub fn active() -> Option<Active> {
        // SAFETY: plain Win32 calls with valid buffers.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 || pid == std::process::id() {
                return None;
            }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);

            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if proc.is_null() {
                return None;
            }
            let mut path = [0u16; 1024];
            let mut len = path.len() as u32;
            let ok =
                QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut len);
            CloseHandle(proc);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&path[..len as usize]);
            let app = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut r: RECT = std::mem::zeroed();
            let center = (GetWindowRect(hwnd, &mut r) != 0).then(|| {
                (
                    (r.left as f64 + r.right as f64) / 2.0,
                    (r.top as f64 + r.bottom as f64) / 2.0,
                )
            });
            Some(Active { app, title, center })
        }
    }

    /// Windows needs no permission to read window titles or capture the screen.
    pub fn screen_permission() -> Permission {
        Permission::Granted
    }

    pub fn request_screen_permission() -> Permission {
        Permission::Granted
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::{Active, Permission};
    pub fn active() -> Option<Active> {
        None
    }
    pub fn screen_permission() -> Permission {
        Permission::Unsupported
    }
    pub fn request_screen_permission() -> Permission {
        Permission::Unsupported
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Granted,
    Denied,
    #[cfg_attr(any(target_os = "macos", windows), allow(dead_code))]
    Unsupported,
}

pub use imp::{active, request_screen_permission, screen_permission};
