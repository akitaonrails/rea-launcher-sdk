//! Monitors, display modes and Wine's DPI (Windows only).
//!
//! - `monitors` is CMR4's: every monitor, sized by its current *physical*
//!   mode (`EnumDisplaySettingsW`), so desktop scaling never shrinks it and
//!   a portrait primary is seen as one. The other launchers used
//!   `GetSystemMetrics`, which is DPI-virtualised and primary-only.
//!   Choosing among them is `rea_launcher::display::game_monitor`.
//! - `mode_supported` is SRR/OR's.
//! - `wine_dpi_scale` is OR's (commit ee8bf02): under Wine, egui-winit
//!   creates the window at a logical size that Wine multiplies by its DPI,
//!   so the settings window's creation zoom must divide that scale out.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    DEVMODEW, ENUM_CURRENT_SETTINGS, EnumDisplayMonitors, EnumDisplaySettingsW, GetDC,
    GetDeviceCaps, GetMonitorInfoW, HDC, HMONITOR, LOGPIXELSY, MONITORINFOEXW, ReleaseDC,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXSCREEN, SM_CYSCREEN, SetProcessDPIAware,
};

use rea_launcher::display::{self, Monitor};

/// Every monitor, in enumeration order, sized by its current physical
/// mode.
pub fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        // SAFETY: `data` is the Vec passed by `monitors`, alive for the
        // whole enumeration; both structs are sized before the calls
        // fill them.
        unsafe {
            let found = &mut *(data as *mut Vec<Monitor>);
            let mut info: MONITORINFOEXW = std::mem::zeroed();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(monitor, (&raw mut info).cast()) == 0 {
                return 1;
            }
            let rect = info.monitorInfo.rcMonitor;
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = size_of::<DEVMODEW>() as u16;
            let (width, height) =
                if EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode)
                    != 0
                {
                    (mode.dmPelsWidth, mode.dmPelsHeight)
                } else {
                    (
                        (rect.right - rect.left).max(0) as u32,
                        (rect.bottom - rect.top).max(0) as u32,
                    )
                };
            if width > 0 && height > 0 {
                found.push(Monitor {
                    width,
                    height,
                    primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                });
            }
            1
        }
    }

    let mut found: Vec<Monitor> = Vec::new();
    // SAFETY: the callback only writes into `found` during this call.
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(visit), (&raw mut found) as LPARAM);
    }
    found
}

/// The primary monitor's physical size; `GetSystemMetrics` only when
/// enumeration finds nothing.
pub fn screen_size() -> Option<(u32, u32)> {
    display::primary(&monitors())
        .map(Monitor::size)
        .or_else(|| {
            // SAFETY: plain metric queries.
            let (width, height) =
                unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
            (width > 0 && height > 0).then_some((width as u32, height as u32))
        })
}

/// Whether the primary display adapter lists a `width`×`height` mode
/// (exclusive fullscreen needs one on Windows).
pub fn mode_supported(width: u32, height: u32) -> bool {
    let mut index = 0;
    loop {
        // SAFETY: the mode struct is sized before the call fills it.
        let mode = unsafe {
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = size_of::<DEVMODEW>() as u16;
            if EnumDisplaySettingsW(null(), index, &mut mode) == 0 {
                return false;
            }
            mode
        };
        if mode.dmPelsWidth == width && mode.dmPelsHeight == height {
            return true;
        }
        index += 1;
    }
}

/// Wine's DPI setting as a scale (1.0 at 96 DPI, 1.5 at 144), to pass to
/// the settings window's creation zoom as the native scale. Call it before
/// any window exists.
///
/// Under Wine it first makes the process system-DPI-aware (Wine has one DPI
/// for every monitor, so nothing is lost). On native Windows it does
/// nothing and returns 1.0: changing the awareness there would override
/// winit's per-monitor awareness, and Windows sizes the window itself.
pub fn wine_dpi_scale() -> f32 {
    if !crate::wine::is_wine() {
        return 1.0;
    }
    // SAFETY: awareness is a process flag; the screen DC is released
    // after the one query.
    let dpi = unsafe {
        SetProcessDPIAware();
        let dc = GetDC(null_mut());
        if dc.is_null() {
            return 1.0;
        }
        let dpi = GetDeviceCaps(dc, LOGPIXELSY as i32);
        ReleaseDC(null_mut(), dc);
        dpi
    };
    if dpi > 0 { dpi as f32 / 96.0 } else { 1.0 }
}
