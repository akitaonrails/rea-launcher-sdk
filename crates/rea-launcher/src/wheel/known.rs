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
const SIMAGIC: u16 = 0x11ff;
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
        LOGITECH | THRUSTMASTER | FANATEC | MOZA | SIMAGIC | SIMUCUBE
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
    }
}
