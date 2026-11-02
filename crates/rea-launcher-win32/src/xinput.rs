//! XInput: pad state, rumble and USB ids (Windows only).
//!
//! The function lookup is cached once (CMR4: the settings window polls
//! every frame, and each `LoadLibraryW` would add a DLL reference); the
//! rumble test is SRR/OR's; the USB ids come from CMR4's
//! `XInputGetCapabilitiesEx` (xinput1_4 ordinal 108, Windows 10+ and Wine).
//! State parsing, button bits and the motor scale are pure and live in
//! `rea_launcher::pad`.

use std::sync::OnceLock;
use std::time::Duration;

use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::latest::Latest;
use crate::wide::wide;
use rea_launcher::pad::{Pad, ids_from_capabilities_ex, motor_speed};

type GetState = unsafe extern "system" fn(u32, *mut [u8; 16]) -> u32;
type FarProc = unsafe extern "system" fn() -> isize;
type SetState = unsafe extern "system" fn(u32, *mut [u16; 2]) -> u32;
type CapabilitiesEx = unsafe extern "system" fn(u32, u32, u32, *mut [u8; 32]) -> u32;

#[derive(Clone, Copy)]
struct Procs {
    get_state: Option<GetState>,
    set_state: Option<SetState>,
    capabilities_ex: Option<CapabilitiesEx>,
}

/// The XInput entry points, looked up once from the newest DLL present.
fn procs() -> Procs {
    static PROCS: OnceLock<Procs> = OnceLock::new();
    *PROCS.get_or_init(|| {
        let mut procs = Procs {
            get_state: None,
            set_state: None,
            capabilities_ex: None,
        };
        for dll in ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"] {
            // SAFETY: loader calls with NUL-terminated names (the module
            // stays loaded for the process). Each transmute gives the
            // documented signature of that export; ordinal 108 exists
            // only in xinput1_4.
            unsafe {
                let module = LoadLibraryW(wide(dll).as_ptr());
                if module.is_null() {
                    continue;
                }
                let Some(get) = GetProcAddress(module, c"XInputGetState".as_ptr().cast()) else {
                    continue;
                };
                procs.get_state = Some(std::mem::transmute::<FarProc, GetState>(get));
                procs.set_state = GetProcAddress(module, c"XInputSetState".as_ptr().cast())
                    .map(|set| std::mem::transmute::<FarProc, SetState>(set));
                if dll == "xinput1_4.dll" {
                    procs.capabilities_ex = GetProcAddress(module, 108 as *const u8)
                        .map(|caps| std::mem::transmute::<FarProc, CapabilitiesEx>(caps));
                }
            }
            break;
        }
        procs
    })
}

/// Every pad connected now; empty without XInput.
pub fn pads() -> Vec<Pad> {
    let Some(get) = procs().get_state else {
        return Vec::new();
    };
    (0..4)
        .filter_map(|slot| {
            let mut state = [0u8; 16];
            // SAFETY: XInputGetState fills exactly 16 bytes.
            (unsafe { get(slot, &mut state) } == 0).then(|| Pad::from_state(slot, &state))
        })
        .collect()
}

/// How many pads are connected now.
pub fn connected() -> usize {
    pads().len()
}

/// The buttons held on any pad now (`wButtons` bits ORed); 0 without
/// XInput. What the settings window's pad navigation reads.
pub fn buttons() -> u16 {
    pads().iter().fold(0, |all, pad| all | pad.buttons)
}

/// Rumble requests; a pending [`test_rumble`] timer only stops the motors
/// when no later request came in.
static RUMBLE: Latest = Latest::new();

/// Sets both motors of every pad to `speed`; the number of pads that
/// accepted it. Cancels a pending [`test_rumble`] stop.
pub fn set_rumble(speed: u16) -> usize {
    RUMBLE.next();
    motors(speed)
}

fn motors(speed: u16) -> usize {
    let Some(set) = procs().set_state else {
        return 0;
    };
    (0..4)
        // SAFETY: XInputSetState reads a 4-byte XINPUT_VIBRATION.
        .filter(|&slot| unsafe { set(slot, &mut [speed, speed]) } == 0)
        .count()
}

/// Stops every pad's motors.
pub fn stop_rumble() {
    set_rumble(0);
}

/// Rumbles every pad at `percent` for `length`, then stops (on a
/// background thread); the number of pads that rumbled. The latest call
/// wins: a second test while the first runs is not cut short by the
/// first one's timer.
pub fn test_rumble(percent: u32, length: Duration) -> usize {
    let ticket = RUMBLE.next();
    let pads = motors(motor_speed(percent));
    if pads > 0 {
        std::thread::spawn(move || {
            std::thread::sleep(length);
            if RUMBLE.is_current(ticket) {
                motors(0);
            }
        });
    }
    pads
}

/// USB vendor and product id of the pad in `slot` (slot 0 is what Xidi
/// presents as controller 1); `None` without xinput1_4 or ids.
pub fn ids(slot: u32) -> Option<(u16, u16)> {
    let get = procs().capabilities_ex?;
    let mut caps = [0u8; 32];
    // SAFETY: XInputGetCapabilitiesEx(1, slot, 0, out) fills a 32-byte
    // XINPUT_CAPABILITIES_EX.
    if unsafe { get(1, slot, 0, &mut caps) } != 0 {
        return None;
    }
    ids_from_capabilities_ex(&caps)
}
