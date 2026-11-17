//! The input thread of the TeknoParrot hubs: it owns SDL, reads wheels and
//! pads, merges them into the cabinet's controls and, while a game runs,
//! writes them to the game's input in the game's line format (a [`Wire`]:
//! windowsarcadeloader's stick lines, Dolphin's pipe commands) and plays
//! the force feedback the game sends back. Between games it only reports
//! what it sees ([`Snapshot`]). From sega's `input.rs`, the best of the
//! three copies, with nintendo's wire and pitch.
//!
//! The game knowledge stays in the launcher: its buttons and pad map, the
//! [`Wire`] (line format, aim, synthetic force feedback), the sink when it
//! isn't the loader's stdin (Dolphin's FIFO), and its settings turned into
//! a [`Tuning`]. A run hands the thread its input through [`Feeding`] (a
//! [`supervise::Feed`]) or, in a headless test, plays a [`Script`] through
//! [`Scripted`].

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rea_launcher::ffb::FfbOutput;
use rea_launcher::wal::{Ffb, Out};
use rea_launcher::wheel::{
    Control, Controls, Kind, PadMap, PadSettings, PedalAxis, Script, SteeringLock, WheelProfile,
    WheelSettings, dinput::NoButtons, read_pad,
};

use crate::policy::HapticPolicy;
use crate::sdl::{Devices, Sdl};
use crate::supervise::{self, Outputs};

/// How often devices are read and the force feedback refreshed.
pub const TICK: Duration = Duration::from_millis(4);
/// The game's input is written at least this often even without a change.
pub const REFRESH: Duration = Duration::from_millis(100);
/// A script's line is written at least this often (Dolphin's pipe drops
/// what came before it opened).
const SCRIPT_REFRESH: Duration = Duration::from_millis(200);

/// One look at the controls.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Frame {
    pub controls: Controls,
    /// The first pad's left stick, up/down (−1 up … 1 down), dead zone
    /// applied: Dolphin's handle pitch, a gun's crosshair height.
    pub pitch: f32,
    /// Seconds since the last frame.
    pub dt: f32,
}

/// A game's input format, and what it does with force feedback.
pub trait Wire: Send + 'static {
    /// The text to write for `frame`: whole lines, each ending in `\n`.
    fn encode(&mut self, frame: &Frame) -> String;
    /// One line the game wrote (`out …`), into its force feedback state;
    /// whether it was one. The default reads windowsarcadeloader's
    /// ([`Ffb::apply`]).
    fn output(&mut self, line: &str, ffb: &mut Ffb) -> bool {
        ffb.apply(line)
    }
    /// What the wheel plays this tick: by default the game's own force
    /// (`game`, already scaled by the player's strength).
    fn force(&mut self, game: FfbOutput, _frame: &Frame) -> FfbOutput {
        game
    }
    /// What a script writes before letting go of the input (`quit` for
    /// windowsarcadeloader).
    fn quit(&self) -> Option<&'static str> {
        None
    }
}

/// The player's wheel and pad options for one run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Stick dead zone, fraction of the travel (0–0.3).
    pub deadzone: f32,
    pub lock: SteeringLock,
    /// Force feedback strength (1.0 = the game's).
    pub ffb_gain: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            deadzone: 0.0,
            lock: SteeringLock::DEFAULT,
            ffb_gain: 1.0,
        }
    }
}

/// The settings' dead zone (0–0.3 of the stick's travel) in raw stick units.
pub fn stick_deadzone(fraction: f32) -> u16 {
    (fraction.clamp(0.0, 0.3) * 32767.0).round() as u16
}

/// A stick axis (raw −32768..32767) as −1..1, zero inside the dead zone
/// and rescaled outside it.
pub fn stick_axis(raw: i16, deadzone: u16) -> f32 {
    let v = f32::from(raw).max(-32767.0);
    let dz = f32::from(deadzone).min(32000.0);
    if v.abs() <= dz {
        0.0
    } else {
        (v.signum() * (v.abs() - dz) / (32767.0 - dz)).clamp(-1.0, 1.0)
    }
}

pub enum Command {
    /// Start feeding a game: `sink` is its input.
    Attach {
        sink: Box<dyn Write + Send>,
        wire: Box<dyn Wire>,
        tuning: Tuning,
    },
    /// A force feedback line from the game.
    Output(String),
    /// Stop feeding; the sink is closed and the force feedback released.
    Detach,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub kind: Kind,
    pub haptic: bool,
}

/// The wheel driving the car, as its profile reads it (a loader that reads
/// the wheel itself is told which one and how: Dolphin, lindbergh-loader).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelInfo {
    pub name: String,
    pub guid: String,
    pub steer_axis: usize,
    pub gas: Option<PedalAxis>,
    pub brake: Option<PedalAxis>,
}

impl WheelInfo {
    fn of<B: Control>(name: &str, guid: &str, p: &WheelProfile<B>) -> WheelInfo {
        WheelInfo {
            name: name.to_string(),
            guid: guid.to_string(),
            steer_axis: p.steer_axis,
            gas: p.gas.map(|(_, a)| a),
            brake: p.brake.map(|(_, a)| a),
        }
    }
}

/// What the window shows.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub devices: Vec<DeviceInfo>,
    /// The device driving the car (the first wheel, else the first pad),
    /// by index into `devices`.
    pub driver: Option<usize>,
    pub wheel: Option<WheelInfo>,
    pub controls: Controls,
    pub feeding: bool,
    pub error: Option<String>,
    /// The menu buttons down on any device, in XInput bits
    /// ([`rea_launcher::pad`]; a wheel's d-pad too), for the hub's pad
    /// navigation: `hub::Input::from_pad(&mut pad_nav, snapshot.nav)`.
    pub nav: u16,
}

/// The input thread.
pub struct InputService {
    tx: Sender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    /// Set on drop: the thread ends even while senders live.
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl InputService {
    /// Starts the thread with the launcher's pad layout and the buttons a
    /// wheel's shift paddles press (right, left).
    pub fn start<B: Control + Send>(pad_map: PadMap<B>, paddles: Option<(B, B)>) -> InputService {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let shared = Arc::clone(&snapshot);
        let quit = Arc::new(AtomicBool::new(false));
        let quitting = Arc::clone(&quit);
        let thread = std::thread::Builder::new()
            .name("input".into())
            .spawn(move || run(rx, shared, &quitting, pad_map, paddles))
            .expect("spawn input thread");
        InputService {
            tx,
            snapshot,
            quit,
            thread: Some(thread),
        }
    }

    pub fn sender(&self) -> Sender<Command> {
        self.tx.clone()
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for InputService {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// A game being fed.
struct Fed {
    sink: Box<dyn Write + Send>,
    wire: Box<dyn Wire>,
    gain: f32,
    ffb: Ffb,
    last: String,
    sent: Option<Instant>,
}

impl Fed {
    fn new(sink: Box<dyn Write + Send>, wire: Box<dyn Wire>, gain: f32) -> Fed {
        Fed {
            sink,
            wire,
            gain,
            ffb: Ffb::default(),
            last: String::new(),
            sent: None,
        }
    }

    /// Writes `frame` when it changed, or `refresh` after the last write.
    fn send(&mut self, frame: &Frame, refresh: Duration) -> io::Result<()> {
        let text = self.wire.encode(frame);
        if text != self.last || self.sent.is_none_or(|t| t.elapsed() >= refresh) {
            self.sink.write_all(text.as_bytes())?;
            self.sink.flush()?;
            self.last = text;
            self.sent = Some(Instant::now());
        }
        Ok(())
    }

    fn force(&mut self, frame: &Frame) -> FfbOutput {
        let game = self.ffb.scaled(self.gain);
        self.wire.force(game, frame)
    }
}

fn run<B: Control>(
    rx: Receiver<Command>,
    shared: Arc<Mutex<Snapshot>>,
    quit: &AtomicBool,
    pad_map: PadMap<B>,
    paddles: Option<(B, B)>,
) {
    let sdl = match Sdl::init(&[]) {
        Ok(s) => s,
        Err(e) => {
            if let Ok(mut s) = shared.lock() {
                s.error = Some(format!("controllers unavailable: {e}"));
            }
            // still feed a game (neutral input) so it never waits on us
            idle_without_sdl(rx, &shared, quit);
            return;
        }
    };
    let mut devices = Devices::new(&sdl, HapticPolicy::default());
    let mut pads = PadSettings::new(pad_map);
    let mut wheels = WheelSettings::default();
    let mut profile: Option<WheelProfile<B>> = None;
    let mut fed: Option<Fed> = None;
    let mut last = Instant::now();
    while !quit.load(Ordering::SeqCst) {
        loop {
            match rx.try_recv() {
                Ok(Command::Attach { sink, wire, tuning }) => {
                    pads.stick_deadzone = stick_deadzone(tuning.deadzone);
                    wheels.lock = tuning.lock;
                    fed = Some(Fed::new(sink, wire, tuning.ffb_gain));
                }
                Ok(Command::Output(line)) => {
                    if let Some(f) = fed.as_mut() {
                        f.wire.output(&line, &mut f.ffb);
                    }
                }
                Ok(Command::Detach) => {
                    fed = None;
                    devices.release_force_feedback();
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        devices.pump();
        let list = devices.list();
        let wheel = list.iter().position(|d| d.kind == Kind::Wheel);
        let mut controls = Controls::default();
        if let Some(d) = wheel.map(|w| &list[w]) {
            let fresh = profile
                .as_ref()
                .is_none_or(|p| p.wheel.vid != d.id.vid || p.wheel.pid != d.id.pid);
            if fresh {
                profile = Some(WheelProfile::guess(d.id.clone(), &d.initial, paddles));
            }
            if let Some(p) = &profile {
                controls = p.read(&wheels, &d.state, None);
            }
        }
        let mut pad_driver = None;
        let mut pitch = 0.0;
        for (i, d) in list.iter().enumerate() {
            if let Some(pad) = d.pad {
                controls = controls.merge(read_pad(&pads, &pad, None));
                if pad_driver.is_none() {
                    pitch = stick_axis(pad.left_y, pads.stick_deadzone);
                    pad_driver = Some(i);
                }
            }
        }
        let driver = wheel.or(pad_driver);
        let snapshot = Snapshot {
            devices: list
                .iter()
                .map(|d| DeviceInfo {
                    name: d.id.name.clone(),
                    kind: d.kind,
                    haptic: d.haptic,
                })
                .collect(),
            driver,
            wheel: wheel
                .zip(profile.as_ref())
                .map(|(w, p)| WheelInfo::of(&list[w].id.name, &list[w].id.guid, p)),
            controls,
            feeding: false,
            error: None,
            nav: list.iter().fold(0, |bits, d| bits | d.nav_bits()),
        };
        let frame = Frame {
            controls,
            pitch,
            dt: since(&mut last),
        };
        if let Some(f) = fed.as_mut() {
            if f.send(&frame, REFRESH).is_err() {
                // the game is gone: nothing may keep pulling the wheel
                fed = None;
                devices.release_force_feedback();
            } else if let Some(d) = driver {
                let out = f.force(&frame);
                devices.force_feedback(d, &out);
            }
        }
        if let Ok(mut s) = shared.lock() {
            *s = Snapshot {
                feeding: fed.is_some(),
                ..snapshot
            };
        }
        std::thread::sleep(TICK);
    }
}

/// Seconds since `last`, which becomes now.
fn since(last: &mut Instant) -> f32 {
    let now = Instant::now();
    let dt = now.duration_since(*last).as_secs_f32();
    *last = now;
    dt
}

/// Without SDL: a game still gets neutral input, so it never waits on us.
fn idle_without_sdl(rx: Receiver<Command>, shared: &Mutex<Snapshot>, quit: &AtomicBool) {
    let mut fed: Option<Fed> = None;
    let mut last = Instant::now();
    while !quit.load(Ordering::SeqCst) {
        match rx.recv_timeout(REFRESH) {
            Ok(Command::Attach { sink, wire, .. }) => fed = Some(Fed::new(sink, wire, 0.0)),
            Ok(Command::Detach) => fed = None,
            Ok(Command::Output(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let neutral = Frame {
            dt: since(&mut last),
            ..Frame::default()
        };
        if fed
            .as_mut()
            .is_some_and(|f| f.send(&neutral, REFRESH).is_err())
        {
            fed = None;
        }
        if let Ok(mut s) = shared.lock() {
            s.feeding = fed.is_some();
        }
    }
}

/// The first wheel SDL sees and its guessed profile: a short look before a
/// game that reads the wheel itself starts (SDL is closed again before the
/// game opens it). `None` while the input thread holds SDL.
pub fn find_wheel() -> Option<WheelInfo> {
    let sdl = Sdl::init(&[]).ok()?;
    let mut devices = Devices::new(&sdl, HapticPolicy::default());
    for _ in 0..5 {
        devices.pump();
        std::thread::sleep(Duration::from_millis(60));
    }
    let d = devices.list().iter().find(|d| d.kind == Kind::Wheel)?;
    let profile: WheelProfile<NoButtons> = WheelProfile::guess(d.id.clone(), &d.initial, None);
    Some(WheelInfo::of(&d.id.name, &d.id.guid, &profile))
}

/// A run fed by the input thread (a [`supervise::Feed`]): the game's input
/// goes to the thread, its force feedback lines too.
pub struct Feeding {
    tx: Sender<Command>,
    wire: Option<Box<dyn Wire>>,
    tuning: Tuning,
}

impl Feeding {
    pub fn new(tx: Sender<Command>, wire: Box<dyn Wire>, tuning: Tuning) -> Feeding {
        Feeding {
            tx,
            wire: Some(wire),
            tuning,
        }
    }
}

impl supervise::Feed for Feeding {
    fn attach(&mut self, sink: Option<Box<dyn Write + Send>>) {
        if let (Some(sink), Some(wire)) = (sink, self.wire.take()) {
            let tuning = self.tuning;
            let _ = self.tx.send(Command::Attach { sink, wire, tuning });
        }
    }

    fn outputs(&mut self) -> Option<Outputs> {
        let tx = self.tx.clone();
        Some(Box::new(move |line| {
            let ffb = Out::parse(line).is_some();
            if ffb {
                let _ = tx.send(Command::Output(line.to_string()));
            }
            ffb
        }))
    }

    fn detach(&mut self) {
        let _ = self.tx.send(Command::Detach);
    }
}

/// A run fed by a [`Script`] (headless tests): its line on every change
/// and every 200 ms, the wire's quit line at the end. The game's force
/// feedback lines are dropped.
pub struct Scripted {
    wire: Option<Box<dyn Wire>>,
    script: Script,
    fed: Option<Fed>,
    at: Duration,
}

impl Scripted {
    pub fn new(wire: Box<dyn Wire>, script: Script) -> Scripted {
        Scripted {
            wire: Some(wire),
            script,
            fed: None,
            at: Duration::ZERO,
        }
    }
}

impl supervise::Feed for Scripted {
    fn attach(&mut self, sink: Option<Box<dyn Write + Send>>) {
        if let (Some(sink), Some(wire)) = (sink, self.wire.take()) {
            self.fed = Some(Fed::new(sink, wire, 0.0));
        }
    }

    fn outputs(&mut self) -> Option<Outputs> {
        Some(Box::new(|line| Out::parse(line).is_some()))
    }

    fn tick(&mut self, ran: Duration) {
        let (controls, pitch) = self.script.at(ran.as_secs_f64());
        let frame = Frame {
            controls,
            pitch,
            dt: (ran - self.at).as_secs_f32(),
        };
        self.at = ran;
        if self
            .fed
            .as_mut()
            .is_some_and(|f| f.send(&frame, SCRIPT_REFRESH).is_err())
        {
            self.fed = None;
        }
    }

    fn detach(&mut self) {
        if let Some(mut f) = self.fed.take()
            && let Some(quit) = f.wire.quit()
        {
            let _ = writeln!(f.sink, "{quit}").and_then(|_| f.sink.flush());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dead_zone_converts_to_raw_units() {
        assert_eq!(stick_deadzone(0.0), 0);
        assert_eq!(stick_deadzone(0.05), 1638);
        assert_eq!(stick_deadzone(0.9), 9830);
    }

    #[test]
    fn a_stick_axis_has_a_dead_zone_and_full_travel() {
        assert_eq!(stick_axis(1000, 1638), 0.0);
        assert_eq!(stick_axis(32767, 1638), 1.0);
        assert_eq!(stick_axis(-32768, 1638), -1.0);
        assert!(stick_axis(17000, 1638) > 0.4);
    }
}
