//! Wheels we know by USB VID/PID, after Wanszai's table (studied in
//! rea-rad-mobile's `docs/WANSZAI_WHEELS.md` §1; from rea-rad-mobile
//! `wheel/known.rs`).
//!
//! Only what holds on every platform is kept: the name, the wheel's full
//! rotation (for the steering-lock maths) and the shift paddles' button
//! numbers. Axis numbers differ between DirectInput and the Linux drivers,
//! so pedals are found by their resting position instead
//! ([`super::guess_pedals`]) and confirmed by the setup wizard.

/// One wheel model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownWheel {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    /// Lock to lock, in degrees.
    pub rotation: u16,
    /// Shift paddle buttons (right = up, left = down) on Windows.
    pub paddles: Option<(u8, u8)>,
}

const LOGITECH: u16 = 0x046d;
const THRUSTMASTER: u16 = 0x044f;
const FANATEC: u16 = 0x0eb7;
const MOZA: u16 = 0x346e;
/// Lite Star, the vendor ID of PXN's older wheels (V10, V12; Linux
/// `hid-ids.h` `USB_VENDOR_ID_LITE_STAR`). It used to be labelled Simagic
/// here.
const LITE_STAR_PXN: u16 = 0x11ff;
/// PXN's own vendor ID, on its direct-drive wheel bases (the VD6 is
/// `36e6:400f`; SDL added it to its wheel list in March 2026, after the
/// SDL2 we ship).
const PXN: u16 = 0x36e6;
const SIMUCUBE: u16 = 0x16d0;

const fn w(vid: u16, pid: u16, name: &'static str, rotation: u16, up: u8, down: u8) -> KnownWheel {
    KnownWheel {
        vid,
        pid,
        name,
        rotation,
        paddles: Some((up, down)),
    }
}

/// Common face/menu buttons on a wheel rim (0-based SDL button indices):
/// used when no user-calibrated `wheel.ini` exists so Start, Coin/Back and
/// A/B/X/Y work out of the box on known wheels instead of leaving wheel-only
/// rigs with no buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelButtons {
    pub a: u8,
    pub b: u8,
    pub x: u8,
    pub y: u8,
    pub start: u8,
    pub back: u8,
}

impl WheelButtons {
    /// Generic wheel button fallback (standard HID / Logitech / Thrustmaster).
    pub const GENERIC: WheelButtons = WheelButtons {
        a: 0,
        b: 1,
        x: 2,
        y: 3,
        start: 7,
        back: 6,
    };

    /// Fanatec CSL DD / GT DD Pro / CSL Elite (`0eb7:*`, verified on a real
    /// CSL DD `0eb7:0020` via `rea-rad-mobile`'s wizard in rea-launcher-sdk#3).
    pub const FANATEC: WheelButtons = WheelButtons {
        a: 0,
        b: 3,
        x: 2,
        y: 1,
        start: 9,
        back: 8,
    };

    /// Logitech G25 / G27 / G29 / G920 / G923 (`046d:*`).
    pub const LOGITECH: WheelButtons = WheelButtons {
        a: 0,
        b: 1,
        x: 2,
        y: 3,
        start: 9,
        back: 8,
    };

    /// Default wheel button numbers for `(vid, pid)`.
    pub fn for_device(vid: u16, _pid: u16) -> WheelButtons {
        match vid {
            FANATEC => WheelButtons::FANATEC,
            LOGITECH => WheelButtons::LOGITECH,
            _ => WheelButtons::GENERIC,
        }
    }
}

/// Every wheel model we know.
pub const KNOWN: &[KnownWheel] = &[
    w(LOGITECH, 0xc298, "Logitech Driving Force Pro", 900, 4, 5),
    w(LOGITECH, 0xc299, "Logitech G25", 900, 4, 5),
    w(LOGITECH, 0xc29a, "Logitech Driving Force GT", 900, 4, 5),
    w(LOGITECH, 0xc29b, "Logitech G27", 900, 4, 5),
    w(LOGITECH, 0xc24f, "Logitech G29", 900, 4, 5),
    w(LOGITECH, 0xc262, "Logitech G920", 900, 4, 5),
    w(LOGITECH, 0xc266, "Logitech G923 (PS)", 900, 4, 5),
    w(LOGITECH, 0xc26e, "Logitech G923 (Xbox)", 900, 4, 5),
    w(THRUSTMASTER, 0xb677, "Thrustmaster T150", 900, 0, 1),
    w(THRUSTMASTER, 0xb67f, "Thrustmaster TMX", 900, 0, 1),
    w(THRUSTMASTER, 0xb66e, "Thrustmaster T300RS", 1080, 0, 1),
    w(THRUSTMASTER, 0xb669, "Thrustmaster TX", 900, 0, 1),
    w(THRUSTMASTER, 0xb696, "Thrustmaster T248", 900, 0, 1),
    w(THRUSTMASTER, 0xb684, "Thrustmaster T-GT", 1080, 0, 1),
    w(FANATEC, 0x0e03, "Fanatec CSL Elite", 1080, 0, 1),
    w(
        FANATEC,
        0x0004,
        "Fanatec CSL DD (Compatibility)",
        1080,
        0,
        1,
    ),
    w(FANATEC, 0x0005, "Fanatec CSL Elite (PS4)", 1080, 0, 1),
    w(FANATEC, 0x0020, "Fanatec CSL DD / GT DD Pro", 1080, 0, 1),
    w(MOZA, 0x0005, "MOZA R3", 900, 13, 12),
];

/// Rotation assumed for a wheel we don't know.
pub const GENERIC_ROTATION: u16 = 900;

/// Shift paddles (up, down) assumed for a wheel we don't know: Wanszai's
/// generic row.
pub const GENERIC_PADDLES: (u8, u8) = (0, 1);

/// The row for a model, if we know it.
pub fn lookup(vid: u16, pid: u16) -> Option<&'static KnownWheel> {
    KNOWN.iter().find(|k| k.vid == vid && k.pid == pid)
}

/// A vendor that makes wheels. Only a hint: Logitech and Thrustmaster make
/// gamepads too, so [`super::classify`] asks it last, after the known
/// rows, SDL's wheel type and the game-controller test.
pub fn wheel_vendor(vid: u16) -> bool {
    matches!(
        vid,
        LOGITECH | THRUSTMASTER | FANATEC | MOZA | LITE_STAR_PXN | PXN | SIMUCUBE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owners_wheel_is_known() {
        let csl = lookup(0x0eb7, 0x0020).unwrap();
        assert_eq!(csl.name, "Fanatec CSL DD / GT DD Pro");
        assert_eq!(csl.rotation, 1080);
    }

    #[test]
    fn ids_are_unique() {
        for (i, a) in KNOWN.iter().enumerate() {
            for b in &KNOWN[i + 1..] {
                assert!(
                    (a.vid, a.pid) != (b.vid, b.pid),
                    "{} and {}",
                    a.name,
                    b.name
                );
            }
        }
    }

    #[test]
    fn unknown_ids_fall_through() {
        assert_eq!(lookup(0x1234, 0x5678), None);
        assert!(wheel_vendor(0x046d));
        assert!(wheel_vendor(0x16d0));
        assert!(!wheel_vendor(0x045e)); // Microsoft (Xbox pads)
        assert!(wheel_vendor(0x11ff)); // PXN V10 / V12
        assert!(wheel_vendor(0x36e6)); // PXN VD6
    }
}
