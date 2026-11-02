//! Monitors, display modes and Wine's DPI (Windows only).
//!
//! - `monitors` is CMR4's: every monitor, sized by its current *physical*
//!   mode (`EnumDisplaySettingsW`), so desktop scaling never shrinks it and
//!   a portrait primary is seen as one. The other launchers used
//!   `GetSystemMetrics`, which is DPI-virtualised and primary-only.
//!   Choosing among them is `rea_launcher::display::game_monitor`.
//! - `displays`/`adapters` add SCR's refresh rate and adapter names.
//! - `mode_supported` is SRR/OR's.
//! - `wine_dpi_scale` is OR's (commit ee8bf02): under Wine, egui-winit
//!   creates the window at a logical size that Wine multiplies by its DPI,
//!   so the settings window's creation zoom must divide that scale out.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    DEVMODEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW,
    ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsW, GetDC,
    GetDeviceCaps, GetMonitorInfoW, HDC, HMONITOR, LOGPIXELSY, MONITORINFOEXW, ReleaseDC,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, MONITORINFOF_PRIMARY, SM_CXSCREEN, SM_CYSCREEN, SetProcessDPIAware,
};

use rea_launcher::display::{self, Monitor};

use crate::wide::from_wide;

/// One monitor with what SCR's detection reads besides its size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// Size (physical pixels, current mode) and primary flag.
    pub monitor: Monitor,
    /// Current refresh rate in Hz; 0 when the driver doesn't say (Windows
    /// reports 0 or 1 for "hardware default").
    pub refresh_hz: u32,
    /// The GDI device name, e.g. `\\.\DISPLAY1`.
    pub device: String,
    /// The display adapter driving it, e.g. `NVIDIA GeForce RTX 4070`
    /// (empty when unknown).
    pub adapter: String,
}

/// One display adapter output (`EnumDisplayDevicesW`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adapter {
    /// The GDI device name its monitors report, e.g. `\\.\DISPLAY1`.
    pub device: String,
    /// The adapter's name, e.g. `AMD Radeon RX 7800 XT`.
    pub name: String,
    /// Part of the desktop (has a monitor in use).
    pub attached: bool,
    pub primary: bool,
}

/// Every display adapter output, attached or not (SCR picks the GPU by
/// name from the attached ones).
pub fn adapters() -> Vec<Adapter> {
    let mut found = Vec::new();
    for index in 0.. {
        // SAFETY: the struct is sized before the call fills it.
        let device = unsafe {
            let mut device: DISPLAY_DEVICEW = std::mem::zeroed();
            device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(null(), index, &mut device, 0) == 0 {
                break;
            }
            device
        };
        found.push(Adapter {
            device: from_wide(&device.DeviceName).to_string_lossy().into_owned(),
            name: from_wide(&device.DeviceString)
                .to_string_lossy()
                .trim()
                .to_owned(),
            attached: device.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP != 0,
            primary: device.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
        });
    }
    found
}

/// Every monitor, in enumeration order, sized by its current physical
/// mode.
pub fn monitors() -> Vec<Monitor> {
    displays()
        .into_iter()
        .map(|display| display.monitor)
        .collect()
}

/// [`monitors`] with each one's refresh rate, device and adapter name.
pub fn displays() -> Vec<Display> {
    unsafe extern "system" fn visit(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
        // SAFETY: `data` is the Vec passed by `displays`, alive for the
        // whole enumeration; both structs are sized before the calls
        // fill them.
        unsafe {
            let found = &mut *(data as *mut Vec<Display>);
            let mut info: MONITORINFOEXW = std::mem::zeroed();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if GetMonitorInfoW(monitor, (&raw mut info).cast()) == 0 {
                return 1;
            }
            let rect = info.monitorInfo.rcMonitor;
            let mut mode: DEVMODEW = std::mem::zeroed();
            mode.dmSize = size_of::<DEVMODEW>() as u16;
            let (width, height, refresh_hz) =
                if EnumDisplaySettingsW(info.szDevice.as_ptr(), ENUM_CURRENT_SETTINGS, &mut mode)
                    != 0
                {
                    let hz = mode.dmDisplayFrequency;
                    (
                        mode.dmPelsWidth,
                        mode.dmPelsHeight,
                        if hz > 1 { hz } else { 0 },
                    )
                } else {
                    (
                        (rect.right - rect.left).max(0) as u32,
                        (rect.bottom - rect.top).max(0) as u32,
                        0,
                    )
                };
            if width > 0 && height > 0 {
                found.push(Display {
                    monitor: Monitor {
                        width,
                        height,
                        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
                    },
                    refresh_hz,
                    device: from_wide(&info.szDevice).to_string_lossy().into_owned(),
                    adapter: String::new(),
                });
            }
            1
        }
    }

    let mut found: Vec<Display> = Vec::new();
    // SAFETY: the callback only writes into `found` during this call.
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(visit), (&raw mut found) as LPARAM);
    }
    let adapters = adapters();
    for display in &mut found {
        if let Some(adapter) = adapters
            .iter()
            .find(|adapter| adapter.device.eq_ignore_ascii_case(&display.device))
        {
            display.adapter.clone_from(&adapter.name);
        }
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

/// Run on Windows or under Wine (`scripts/wine-test`).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_agree_with_monitors_and_name_their_adapter() {
        let displays = displays();
        assert_eq!(
            monitors(),
            displays.iter().map(|d| d.monitor).collect::<Vec<_>>()
        );
        let adapters = adapters();
        for display in &displays {
            assert!(display.device.starts_with(r"\\.\"), "{display:?}");
            if let Some(adapter) = adapters.iter().find(|a| a.device == display.device) {
                assert_eq!(display.adapter, adapter.name);
                assert!(adapter.attached, "{adapter:?}");
            }
            assert_ne!(display.refresh_hz, 1);
        }
    }
}
