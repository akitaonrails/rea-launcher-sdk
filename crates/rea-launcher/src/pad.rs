//! Gamepad state and settings-window navigation, without any I/O.
//!
//! The button bits are XInput's `wButtons`; a native Linux launcher maps
//! its joystick into the same bits (SCR). [`Pad::from_state`],
//! [`ids_from_capabilities_ex`] and [`motor_speed`] are the win32 crate's
//! pure parts (CMR4's cached XInput lookup, SRR/OR's rumble test), which
//! read the raw structs. [`pressed`] is CMR4/SCR's `pad_actions` edge
//! detection — one step per press, never per frame held — with SCR's set
//! of steps plus B/Back (the audit's "visible focus, left/right stepping,
//! B/back"). The egui key mapping stays in `rea-launcher-ui`.

pub const DPAD_UP: u16 = 0x0001;
pub const DPAD_DOWN: u16 = 0x0002;
pub const DPAD_LEFT: u16 = 0x0004;
pub const DPAD_RIGHT: u16 = 0x0008;
pub const START: u16 = 0x0010;
pub const BACK: u16 = 0x0020;
pub const LEFT_THUMB: u16 = 0x0040;
pub const RIGHT_THUMB: u16 = 0x0080;
pub const LEFT_SHOULDER: u16 = 0x0100;
pub const RIGHT_SHOULDER: u16 = 0x0200;
pub const A: u16 = 0x1000;
pub const B: u16 = 0x2000;
pub const X: u16 = 0x4000;
pub const Y: u16 = 0x8000;

/// How far a stick must lean to count as a d-pad press (about half way).
pub const STICK_THRESHOLD: i16 = 16_000;

/// One connected pad at one moment (`XINPUT_STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pad {
    /// XInput slot, 0..=3.
    pub slot: u32,
    pub packet: u32,
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
}

impl Pad {
    /// Parses the 16-byte `XINPUT_STATE`: a packet number, then the
    /// gamepad with `wButtons` first.
    pub fn from_state(slot: u32, state: &[u8; 16]) -> Pad {
        let u16_at = |at: usize| u16::from_le_bytes([state[at], state[at + 1]]);
        Pad {
            slot,
            packet: u32::from_le_bytes([state[0], state[1], state[2], state[3]]),
            buttons: u16_at(4),
            left_trigger: state[6],
            right_trigger: state[7],
            left_x: u16_at(8) as i16,
            left_y: u16_at(10) as i16,
            right_x: u16_at(12) as i16,
            right_y: u16_at(14) as i16,
        }
    }

    /// The buttons, with the left stick leaning past [`STICK_THRESHOLD`]
    /// added as d-pad bits (XInput's Y axis points up), as SCR navigates.
    pub fn nav_buttons(&self) -> u16 {
        let mut bits = self.buttons;
        for (lean, bit) in [
            (self.left_y > STICK_THRESHOLD, DPAD_UP),
            (self.left_y < -STICK_THRESHOLD, DPAD_DOWN),
            (self.left_x < -STICK_THRESHOLD, DPAD_LEFT),
            (self.left_x > STICK_THRESHOLD, DPAD_RIGHT),
        ] {
            if lean {
                bits |= bit;
            }
        }
        bits
    }
}

/// Vendor and product ids from a 32-byte `XINPUT_CAPABILITIES_EX` (bytes
/// 20 and 22); `None` when either is zero (not reported).
pub fn ids_from_capabilities_ex(caps: &[u8; 32]) -> Option<(u16, u16)> {
    let vendor = u16::from_le_bytes([caps[20], caps[21]]);
    let product = u16::from_le_bytes([caps[22], caps[23]]);
    (vendor != 0 && product != 0).then_some((vendor, product))
}

/// Motor speed for a 0–100 % strength (clamped).
pub fn motor_speed(percent: u32) -> u16 {
    (percent.min(100) * 65535 / 100) as u16
}

/// One navigation step, from one button press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    /// A.
    Press,
    /// B (or Back/Select).
    Back,
    PreviousTab,
    NextTab,
    /// Start.
    Play,
}

const MAP: [(u16, Nav); 10] = [
    (START, Nav::Play),
    (DPAD_UP, Nav::Up),
    (DPAD_DOWN, Nav::Down),
    (DPAD_LEFT, Nav::Left),
    (DPAD_RIGHT, Nav::Right),
    (A, Nav::Press),
    (B, Nav::Back),
    (BACK, Nav::Back),
    (LEFT_SHOULDER, Nav::PreviousTab),
    (RIGHT_SHOULDER, Nav::NextTab),
];

/// The steps for buttons down in `now` but not in `before`, in a fixed
/// order (Play first). B and Back together give one `Back`.
pub fn pressed(before: u16, now: u16) -> Vec<Nav> {
    let down = now & !before;
    let mut navs: Vec<Nav> = MAP
        .iter()
        .filter(|(bit, _)| down & bit != 0)
        .map(|&(_, nav)| nav)
        .collect();
    navs.dedup();
    navs
}

/// Remembers the held buttons between frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadNav {
    held: u16,
}

impl PadNav {
    /// The steps pressed since the last call.
    pub fn update(&mut self, now: u16) -> Vec<Nav> {
        let navs = pressed(self.held, now);
        self.held = now;
        navs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_bytes_parse_into_a_pad() {
        let mut state = [0u8; 16];
        state[0..4].copy_from_slice(&7u32.to_le_bytes());
        state[4..6].copy_from_slice(&(A | DPAD_DOWN).to_le_bytes());
        state[6] = 10;
        state[7] = 255;
        state[8..10].copy_from_slice(&(-32768i16).to_le_bytes());
        state[14..16].copy_from_slice(&123i16.to_le_bytes());
        let pad = Pad::from_state(2, &state);
        assert_eq!(pad.slot, 2);
        assert_eq!(pad.packet, 7);
        assert_eq!(pad.buttons, A | DPAD_DOWN);
        assert_eq!((pad.left_trigger, pad.right_trigger), (10, 255));
        assert_eq!((pad.left_x, pad.left_y, pad.right_y), (-32768, 0, 123));
        assert_eq!(pad.nav_buttons(), A | DPAD_DOWN | DPAD_LEFT);
    }

    #[test]
    fn the_stick_navigates_past_half_way() {
        let lean = |left_x, left_y| {
            Pad {
                left_x,
                left_y,
                ..Pad::default()
            }
            .nav_buttons()
        };
        assert_eq!(lean(0, 0), 0);
        assert_eq!(lean(0, 8000), 0, "a resting thumb does nothing");
        assert_eq!(lean(0, 32767), DPAD_UP);
        assert_eq!(lean(0, -32768), DPAD_DOWN);
        assert_eq!(lean(20000, 0), DPAD_RIGHT);
    }

    #[test]
    fn capabilities_ex_ids() {
        let mut caps = [0u8; 32];
        assert_eq!(ids_from_capabilities_ex(&caps), None);
        caps[20..22].copy_from_slice(&0x045eu16.to_le_bytes());
        caps[22..24].copy_from_slice(&0x0b12u16.to_le_bytes());
        assert_eq!(ids_from_capabilities_ex(&caps), Some((0x045e, 0x0b12)));
    }

    #[test]
    fn motor_speed_scales_and_clamps() {
        assert_eq!(motor_speed(0), 0);
        assert_eq!(motor_speed(50), 32767);
        assert_eq!(motor_speed(100), 65535);
        assert_eq!(motor_speed(250), 65535);
    }

    #[test]
    fn buttons_act_once_per_press() {
        assert_eq!(pressed(0, START), [Nav::Play]);
        assert_eq!(pressed(START, START), []);
        assert_eq!(pressed(0, DPAD_DOWN | A), [Nav::Down, Nav::Press]);
        assert_eq!(pressed(0, RIGHT_SHOULDER), [Nav::NextTab]);
        assert_eq!(pressed(0, LEFT_SHOULDER), [Nav::PreviousTab]);
        assert_eq!(pressed(0, B | BACK), [Nav::Back]);
        assert_eq!(pressed(A, 0), []);
        // Play comes first, so Start+A plays.
        assert_eq!(pressed(0, A | START)[0], Nav::Play);
    }

    #[test]
    fn pad_nav_tracks_held_buttons() {
        let mut pad = PadNav::default();
        assert_eq!(pad.update(DPAD_DOWN), [Nav::Down]);
        assert_eq!(pad.update(DPAD_DOWN), []);
        assert_eq!(pad.update(DPAD_DOWN | DPAD_RIGHT), [Nav::Right]);
        assert_eq!(pad.update(0), []);
        assert_eq!(pad.update(DPAD_DOWN), [Nav::Down]);
    }
}
