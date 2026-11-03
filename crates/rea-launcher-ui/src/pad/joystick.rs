//! Pads for a native Linux settings window: the kernel joystick devices
//! (`/dev/input/js*`), read without blocking, as XInput button bits for
//! [`super::PadNav`] (DiRT Rally's reader, the first native launcher).
//!
//! The joystick interface numbers a standard-layout pad's buttons A, B, X,
//! Y, LB, RB, Back, Start, Guide, L3, R3 and its axes left stick X/Y, LT,
//! right stick X/Y, RT, then the D-pad hat X/Y (the order SDL uses).
//! Every pad counts, so whichever one the player picks up works.
//!
//! ```no_run
//! # use eframe::egui;
//! # use rea_launcher_ui::pad::{self, Nav, PadNav};
//! # struct App { pad: PadNav, pending: Vec<Nav>, #[cfg(target_os = "linux")] joysticks: pad::joystick::Joysticks }
//! # impl App {
//! fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
//!     #[cfg(target_os = "linux")]
//!     let buttons = self.joysticks.buttons();
//!     # #[cfg(not(target_os = "linux"))] let buttons = 0;
//!     let navs = self.pad.update(buttons);
//!     pad::inject(ctx, raw, &navs);
//!     self.pending.extend(navs);
//! }
//! # }
//! ```

use std::io::{ErrorKind, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use rea_launcher::pad as bits;

/// `js_event.type` bits.
pub const EVENT_BUTTON: u8 = 0x01;
pub const EVENT_AXIS: u8 = 0x02;
/// Set on the events the kernel sends on open with the current state.
pub const EVENT_INIT: u8 = 0x80;

/// How often [`Joysticks::buttons`] looks for pads again, so one switched
/// on after the window opened is found.
pub const RESCAN: Duration = Duration::from_secs(2);

/// Joystick button number → XInput bit (0: not used for navigation).
const BUTTONS: [u16; 11] = [
    bits::A,
    bits::B,
    bits::X,
    bits::Y,
    bits::LEFT_SHOULDER,
    bits::RIGHT_SHOULDER,
    bits::BACK,
    bits::START,
    0, // Guide: the system's
    bits::LEFT_THUMB,
    bits::RIGHT_THUMB,
];

/// One pad's buttons and axes, from its events.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    pub buttons: Vec<bool>,
    pub axes: Vec<i16>,
}

impl State {
    /// Applies one 8-byte `js_event` (time u32, value i16, type u8,
    /// number u8, little-endian).
    pub fn apply(&mut self, event: &[u8; 8]) {
        fn grow<T: Default + Clone>(list: &mut Vec<T>, n: usize) {
            if list.len() <= n {
                list.resize(n + 1, T::default());
            }
        }
        let value = i16::from_le_bytes([event[4], event[5]]);
        let (kind, number) = (event[6] & !EVENT_INIT, usize::from(event[7]));
        match kind {
            EVENT_BUTTON => {
                grow(&mut self.buttons, number);
                self.buttons[number] = value != 0;
            }
            EVENT_AXIS => {
                grow(&mut self.axes, number);
                self.axes[number] = value;
            }
            _ => {}
        }
    }

    /// The XInput bits: the buttons, and the D-pad hat (axes 6/7) or the
    /// left stick past [`bits::STICK_THRESHOLD`] as the D-pad.
    pub fn mask(&self) -> u16 {
        let mut mask = 0;
        for (number, &pressed) in self.buttons.iter().enumerate() {
            if pressed {
                mask |= BUTTONS.get(number).copied().unwrap_or(0);
            }
        }
        let axis = |n: usize| self.axes.get(n).copied().unwrap_or(0);
        let x = axis(6).saturating_add(axis(0));
        let y = axis(7).saturating_add(axis(1));
        let t = bits::STICK_THRESHOLD;
        for (on, bit) in [
            (x <= -t, bits::DPAD_LEFT),
            (x >= t, bits::DPAD_RIGHT),
            (y <= -t, bits::DPAD_UP),
            (y >= t, bits::DPAD_DOWN),
        ] {
            if on {
                mask |= bit;
            }
        }
        mask
    }
}

/// An open joystick: anything that reads `js_event`s without blocking
/// (a `/dev/input/js*` file opened `O_NONBLOCK`, or a test's fake).
pub type Device = Box<dyn Read + Send>;

type Scan = Box<dyn FnMut() -> Vec<Device> + Send>;

/// Every pad, polled without blocking, looked for again every
/// [`RESCAN`].
pub struct Joysticks {
    open: Vec<(Device, State)>,
    scan: Scan,
    every: Duration,
    scanned: Option<Instant>,
}

impl Default for Joysticks {
    fn default() -> Joysticks {
        Joysticks::new()
    }
}

impl std::fmt::Debug for Joysticks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Joysticks")
            .field("open", &self.open.len())
            .finish()
    }
}

impl Joysticks {
    /// The pads under `/dev/input`.
    pub fn new() -> Joysticks {
        Joysticks::with_scan(RESCAN, || devices(Path::new("/dev/input")))
    }

    /// Pads that `scan` opens, every `every` (tests pass fakes).
    pub fn with_scan(
        every: Duration,
        scan: impl FnMut() -> Vec<Device> + Send + 'static,
    ) -> Joysticks {
        Joysticks {
            open: Vec::new(),
            scan: Box::new(scan),
            every,
            scanned: None,
        }
    }

    /// The bits of every pad together, after reading what each one sent
    /// since the last call. A pad that fails or closes is dropped until
    /// the next scan. Call once per frame (`raw_input_hook`).
    pub fn buttons(&mut self) -> u16 {
        if self.scanned.is_none_or(|at| at.elapsed() >= self.every) {
            // Reopened pads report their current state first (EVENT_INIT).
            self.open = (self.scan)()
                .into_iter()
                .map(|device| (device, State::default()))
                .collect();
            self.scanned = Some(Instant::now());
        }
        let mut mask = 0;
        self.open.retain_mut(|(device, state)| {
            let alive = drain(device, state);
            mask |= state.mask();
            alive
        });
        mask
    }

    /// How many pads are open.
    pub fn count(&self) -> usize {
        self.open.len()
    }
}

/// Applies every whole event `device` has ready; false once it failed or
/// closed.
fn drain(device: &mut Device, state: &mut State) -> bool {
    let mut event = [0u8; 8];
    loop {
        match device.read(&mut event) {
            Ok(8) => state.apply(&event),
            Ok(_) => return false,
            Err(error) if error.kind() == ErrorKind::WouldBlock => return true,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// The `js*` devices in `dir` that open for reading, without blocking.
pub fn devices(dir: &Path) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("js"))
        .map(|entry| entry.path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(path)
                .ok()
        })
        .map(|file| Box::new(file) as Device)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::pad::{Nav, PadNav};

    fn event(kind: u8, number: u8, value: i16) -> [u8; 8] {
        let v = value.to_le_bytes();
        [0, 0, 0, 0, v[0], v[1], kind, number]
    }

    /// A fake joystick: queued events, then "would block"; closed (end of
    /// file) or unplugged (an error) on request.
    #[derive(Default)]
    struct Queue {
        events: VecDeque<[u8; 8]>,
        failure: Option<ErrorKind>,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Queue>>);

    impl Fake {
        fn send(&self, event: [u8; 8]) {
            self.0.lock().unwrap().events.push_back(event);
        }
        fn unplug(&self) {
            self.0.lock().unwrap().failure = Some(ErrorKind::NotFound);
        }
    }

    impl Read for Fake {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut inner = self.0.lock().unwrap();
            if let Some(kind) = inner.failure {
                return Err(kind.into());
            }
            match inner.events.pop_front() {
                Some(event) => {
                    buf[..8].copy_from_slice(&event);
                    Ok(8)
                }
                None => Err(ErrorKind::WouldBlock.into()),
            }
        }
    }

    fn joysticks(pads: &[Fake]) -> Joysticks {
        let pads = pads.to_vec();
        Joysticks::with_scan(Duration::from_secs(3600), move || {
            pads.iter()
                .map(|pad| Box::new(pad.clone()) as Device)
                .collect()
        })
    }

    #[test]
    fn a_pad_becomes_xinput_bits() {
        let mut state = State::default();
        state.apply(&event(EVENT_BUTTON | EVENT_INIT, 0, 1)); // A, initial state
        state.apply(&event(EVENT_BUTTON, 7, 1)); // Start
        state.apply(&event(EVENT_AXIS, 7, -32_767)); // hat up
        state.apply(&event(EVENT_AXIS, 0, 20_000)); // stick right
        assert_eq!(
            state.mask(),
            bits::A | bits::START | bits::DPAD_UP | bits::DPAD_RIGHT
        );
        state.apply(&event(EVENT_BUTTON, 0, 0));
        state.apply(&event(EVENT_AXIS, 0, 8_000)); // inside the dead zone
        assert_eq!(state.mask(), bits::START | bits::DPAD_UP);
        // Guide and unknown buttons do nothing.
        let mut other = State::default();
        other.apply(&event(EVENT_BUTTON, 8, 1));
        other.apply(&event(EVENT_BUTTON, 15, 1));
        assert_eq!(other.mask(), 0);
    }

    #[test]
    fn a_fake_event_stream_drives_pad_navigation() {
        let (one, two) = (Fake::default(), Fake::default());
        let mut pads = joysticks(&[one.clone(), two.clone()]);
        let mut nav = PadNav::default();
        assert!(nav.update(pads.buttons()).is_empty());
        assert_eq!(pads.count(), 2);

        // Hat down on one pad: one step per press, not per frame held.
        one.send(event(EVENT_AXIS, 7, 32_767));
        assert_eq!(nav.update(pads.buttons()), [Nav::Down]);
        assert!(nav.update(pads.buttons()).is_empty());
        one.send(event(EVENT_AXIS, 7, 0));
        assert!(nav.update(pads.buttons()).is_empty());

        // A on the other pad, then Start; LB/RB switch tabs.
        two.send(event(EVENT_BUTTON, 0, 1));
        assert_eq!(nav.update(pads.buttons()), [Nav::Press]);
        two.send(event(EVENT_BUTTON, 0, 0));
        two.send(event(EVENT_BUTTON, 7, 1));
        two.send(event(EVENT_BUTTON, 5, 1));
        assert_eq!(nav.update(pads.buttons()), [Nav::Play, Nav::NextTab]);

        // An unplugged pad is dropped; the other keeps working.
        one.unplug();
        two.send(event(EVENT_BUTTON, 7, 0));
        two.send(event(EVENT_BUTTON, 5, 0));
        two.send(event(EVENT_BUTTON, 1, 1));
        assert_eq!(nav.update(pads.buttons()), [Nav::Back]);
        assert_eq!(pads.count(), 1);
    }

    #[test]
    fn pads_are_looked_for_again() {
        let found = Arc::new(Mutex::new(Vec::<Fake>::new()));
        let shared = found.clone();
        let mut pads = Joysticks::with_scan(Duration::ZERO, move || {
            shared
                .lock()
                .unwrap()
                .iter()
                .map(|pad| Box::new(pad.clone()) as Device)
                .collect()
        });
        assert_eq!(pads.buttons(), 0);
        assert_eq!(pads.count(), 0);
        // Switched on later: found on the next scan, with its state.
        let late = Fake::default();
        late.send(event(EVENT_BUTTON | EVENT_INIT, 7, 1));
        found.lock().unwrap().push(late);
        assert_eq!(pads.buttons(), bits::START);
    }

    #[test]
    fn devices_lists_only_joysticks() {
        let dir = std::env::temp_dir().join(format!("rea-js-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["js0", "event3", "js1"] {
            std::fs::write(dir.join(name), event(EVENT_BUTTON, 0, 1)).unwrap();
        }
        let mut found = devices(&dir);
        assert_eq!(found.len(), 2);
        let mut state = State::default();
        assert!(!drain(&mut found[0], &mut state), "a file ends: dropped");
        assert_eq!(state.mask(), bits::A);
        assert!(devices(&dir.join("missing")).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
