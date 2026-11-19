//! windowsarcadeloader's input pipe and outputs, and the ports it listens
//! on.
//!
//! windowsarcadeloader (`arcade-launcher run … --input-pipe`) reads one
//! virtual arcade stick per player from its stdin (its `wal_protocol`):
//! `p1 <buttons hex> <lx> <ly> <rx> <ry> <accel> <brake>`, buttons as the
//! bits of [`button`], axes −32767..32767, pedals 0..32767. It writes the
//! game's outputs to stdout as `out <player> <id> <value>`; [`Ffb`] keeps
//! the force feedback ones. Which bit is which cabinet button depends on
//! the system profile: that table stays in the launcher (game knowledge),
//! which maps its own buttons with [`buttons`]. From the Initial D,
//! Nintendo and Sega hub launchers (`input.rs`, `session.rs`), with
//! Initial D's free port block ([`free_port_block`]) for all three: Sega
//! used the loader's default port, which collides with Initial D's games,
//! and Nintendo a fixed one.

use crate::ffb::FfbOutput;
use crate::pad;
use crate::wheel::{Control, Controls};

/// The virtual stick's button bits (`wal_protocol::button`).
pub mod button {
    pub const UP: u32 = 1 << 0;
    pub const DOWN: u32 = 1 << 1;
    pub const LEFT: u32 = 1 << 2;
    pub const RIGHT: u32 = 1 << 3;
    pub const B1: u32 = 1 << 4;
    pub const B2: u32 = 1 << 5;
    pub const B3: u32 = 1 << 6;
    pub const B4: u32 = 1 << 7;
    pub const B5: u32 = 1 << 8;
    pub const B6: u32 = 1 << 9;
    pub const B7: u32 = 1 << 10;
    pub const B8: u32 = 1 << 11;
    pub const START: u32 = 1 << 12;
    pub const COIN: u32 = 1 << 13;
    pub const SERVICE: u32 = 1 << 14;
    pub const TEST: u32 = 1 << 15;
    pub const CARD: u32 = 1 << 16;
}

/// The stick bits for the pressed buttons in `pressed` ([`Controls::buttons`]),
/// each launcher button mapped by `bit` (0: the profile has none).
pub fn buttons<B: Control>(pressed: u32, bit: impl Fn(B) -> u32) -> u32 {
    B::ALL
        .iter()
        .filter(|b| pressed & b.bit() != 0)
        .fold(0, |acc, &b| acc | bit(b))
}

/// XInput d-pad bits ([`pad::DPAD_UP`]..[`pad::DPAD_RIGHT`]) as
/// windowsarcadeloader virtual stick direction bits (`button::UP`..`RIGHT`).
pub fn dpad(bits: u16) -> u32 {
    [
        (pad::DPAD_UP, button::UP),
        (pad::DPAD_DOWN, button::DOWN),
        (pad::DPAD_LEFT, button::LEFT),
        (pad::DPAD_RIGHT, button::RIGHT),
    ]
    .into_iter()
    .filter(|(in_bit, _)| bits & in_bit != 0)
    .fold(0, |acc, (_, out_bit)| acc | out_bit)
}

/// One player's virtual stick: buttons, sticks −1..1, pedals 0..1.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Stick {
    pub buttons: u32,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub accel: f32,
    pub brake: f32,
}

impl Stick {
    /// A driving cabinet: the wheel on the left stick's X, the pedals.
    pub fn driving(buttons: u32, c: &Controls) -> Stick {
        Stick {
            buttons,
            lx: c.steer,
            accel: c.accel,
            brake: c.brake,
            ..Stick::default()
        }
    }

    /// Player 1's line for the input pipe.
    pub fn line(&self) -> String {
        let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i32;
        let pedal = |v: f32| (v.clamp(0.0, 1.0) * 32767.0).round() as i32;
        format!(
            "p1 {:x} {} {} {} {} {} {}",
            self.buttons,
            axis(self.lx),
            axis(self.ly),
            axis(self.rx),
            axis(self.ry),
            pedal(self.accel),
            pedal(self.brake)
        )
    }
}

/// One output line: `out <player> <id> <value>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Out {
    pub player: u8,
    pub id: u16,
    pub value: i32,
}

impl Out {
    /// The output in `line`, or `None` when it is another line of the
    /// loader's (its log).
    pub fn parse(line: &str) -> Option<Out> {
        let mut w = line.split_whitespace();
        if w.next() != Some("out") {
            return None;
        }
        Some(Out {
            player: w.next()?.parse().ok()?,
            id: w.next()?.parse().ok()?,
            value: w.next()?.parse().ok()?,
        })
    }
}

/// The force feedback outputs' ids (`wal_protocol::output`), each ±10000.
pub mod output {
    pub const CONSTANT: u16 = 1;
    /// A centring spring: played as a damper (the SDK has no spring
    /// effect), which also resists the wheel.
    pub const SPRING: u16 = 2;
    pub const VIBRATION: u16 = 3;
}

/// The game's force feedback state, from its output lines.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ffb(pub FfbOutput);

impl Ffb {
    /// Applies one output line; `false` if it isn't a force feedback one.
    pub fn apply(&mut self, line: &str) -> bool {
        let Some(out) = Out::parse(line) else {
            return false;
        };
        let v = out.value as f32 / 10000.0;
        match out.id {
            output::CONSTANT => self.0.constant = v.clamp(-1.0, 1.0),
            output::SPRING => self.0.damper = v.clamp(0.0, 1.0),
            output::VIBRATION => self.0.rumble = v.clamp(0.0, 1.0),
            _ => return false,
        }
        true
    }

    /// The state scaled by the player's strength (`gain`, 0–2), each
    /// effect kept within its range.
    pub fn scaled(&self, gain: f32) -> FfbOutput {
        let o = self.0;
        FfbOutput {
            constant: (o.constant * gain).clamp(-1.0, 1.0),
            damper: (o.damper * gain).clamp(0.0, 1.0),
            rumble: (o.rumble * gain).clamp(0.0, 1.0),
            ..o
        }
    }
}

/// The ports above the first one the ALLS payloads' services use.
const SERVICES: std::ops::RangeInclusive<u16> = 102..=110;
/// How far apart the blocks tried are, and how many are tried.
const STEP: u16 = 200;
const TRIES: u16 = 20;

/// windowsarcadeloader's first port for this start (its input server; the
/// ALLS payloads' services on the first + 102 to 110), from `from` on: the
/// first block whose ports on 127.0.0.1 are all free. A port another
/// program holds (another loader, a sister launcher's game, or a range
/// Windows reserves for Hyper-V and WSL) stopped the game at its start.
/// `from` when no block is free.
pub fn free_port_block(from: u16) -> u16 {
    free_port_block_with(from, |p| {
        std::net::TcpListener::bind(("127.0.0.1", p)).is_ok()
    })
}

/// [`free_port_block`] with `free` saying which ports are free.
pub fn free_port_block_with(from: u16, free: impl Fn(u16) -> bool) -> u16 {
    (0..TRIES)
        .filter_map(|i| from.checked_add(i * STEP))
        .filter(|base| base.checked_add(*SERVICES.end()).is_some())
        .find(|&base| free(base) && SERVICES.clone().all(|o| free(base + o)))
        .unwrap_or(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum B {
        Up,
        View,
        Start,
    }

    impl Control for B {
        const ALL: &'static [Self] = &[B::Up, B::View, B::Start];
        fn key(self) -> &'static str {
            "b"
        }
        fn label(self) -> &'static str {
            "B"
        }
        fn bit(self) -> u32 {
            1 << Self::ALL.iter().position(|&b| b == self).unwrap()
        }
    }

    /// RingEdge: b1 shift up, b3 view; Start is Start.
    fn ringedge(b: B) -> u32 {
        match b {
            B::Up => button::B1,
            B::View => button::B3,
            B::Start => button::START,
        }
    }

    #[test]
    fn the_launchers_buttons_become_the_profiles_bits() {
        let pressed = B::View.bit() | B::Up.bit();
        assert_eq!(buttons(pressed, ringedge), 0x50);
        assert_eq!(
            buttons(pressed, |b: B| if b == B::View { 0 } else { ringedge(b) }),
            0x10
        );
        assert_eq!(buttons(0, ringedge), 0);
    }

    #[test]
    fn dpad_bits_become_virtual_stick_directions_only() {
        assert_eq!(dpad(0), 0);
        assert_eq!(dpad(pad::DPAD_UP), button::UP);
        assert_eq!(dpad(pad::DPAD_DOWN), button::DOWN);
        assert_eq!(dpad(pad::DPAD_LEFT), button::LEFT);
        assert_eq!(dpad(pad::DPAD_RIGHT), button::RIGHT);
        assert_eq!(
            dpad(pad::DPAD_UP | pad::DPAD_RIGHT | pad::A | pad::START),
            button::UP | button::RIGHT
        );
    }

    #[test]
    fn a_driving_line_carries_the_wheel_and_the_pedals() {
        let c = Controls {
            steer: -1.5,
            accel: 1.0,
            brake: 0.5,
            buttons: 0,
        };
        let line = Stick::driving(button::START, &c).line();
        assert_eq!(line, "p1 1000 -32767 0 0 0 32767 16384");
        assert_eq!(Stick::default().line(), "p1 0 0 0 0 0 0 0");
        // a gun: the crosshair on the left stick, no pedals
        let aim = Stick {
            buttons: button::B1,
            lx: 0.5,
            ly: -1.0,
            ..Stick::default()
        };
        assert_eq!(aim.line(), "p1 10 16384 -32767 0 0 0 0");
    }

    #[test]
    fn output_lines_parse_and_others_do_not() {
        assert_eq!(
            Out::parse("out 0 1 -5000"),
            Some(Out {
                player: 0,
                id: 1,
                value: -5000
            })
        );
        for other in ["wine: hello", "out", "out 0 x 1", "out 0 1", ""] {
            assert_eq!(Out::parse(other), None, "{other}");
        }
    }

    #[test]
    fn force_feedback_lines_update_the_state() {
        let mut f = Ffb::default();
        assert!(f.apply("out 0 1 -5000"));
        assert!(f.apply("out 0 2 10000"));
        assert!(f.apply("out 0 3 2500"));
        assert!(!f.apply("out 0 9 1"));
        assert!(!f.apply("wine: hello"));
        assert_eq!(f.0.constant, -0.5);
        assert_eq!(f.0.damper, 1.0);
        assert_eq!(f.0.rumble, 0.25);
        let half = f.scaled(0.5);
        assert_eq!(
            (half.constant, half.damper, half.rumble),
            (-0.25, 0.5, 0.125)
        );
        let off = f.scaled(0.0);
        assert_eq!((off.constant, off.damper, off.rumble), (0.0, 0.0, 0.0));
        assert_eq!(f.scaled(2.0).damper, 1.0, "kept within range");
        assert!(f.apply("out 0 1 -20000"));
        assert_eq!(f.0.constant, -1.0, "clamped");
    }

    #[test]
    fn the_first_block_with_every_port_free_is_taken() {
        // 33700 itself and a service port of the next block are held
        let held = [33700, 33900 + 105];
        let port = free_port_block_with(33700, |p| !held.contains(&p));
        assert_eq!(port, 34100);
        assert_eq!(free_port_block_with(33700, |_| true), 33700);
        assert_eq!(free_port_block_with(33700, |_| false), 33700, "none free");
        // never past the last port
        assert_eq!(free_port_block_with(65500, |_| true), 65500);
    }

    #[test]
    fn a_port_another_program_holds_is_skipped() {
        // a port the system gives out, low enough for a block above it
        let held =
            std::iter::repeat_with(|| std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap())
                .take(50)
                .find(|l| l.local_addr().unwrap().port() < 60000)
                .expect("a port below 60000");
        let taken = held.local_addr().unwrap().port();
        let port = free_port_block(taken);
        assert_ne!(port, taken);
        assert_eq!((port - taken) % STEP, 0);
        drop(held);
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}
