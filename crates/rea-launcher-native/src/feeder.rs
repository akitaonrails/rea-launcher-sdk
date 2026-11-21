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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rea_launcher::ffb::FfbOutput;
use rea_launcher::wal::{Ffb, Out};
use rea_launcher::wheel::{
    Action, Binding, Control, Controls, DeviceId, DeviceState, Kind, PadInput, PadMap, PadSettings,
    PedalAxis, Script, Source, SteeringLock, WheelButtons, WheelCandidate, WheelProfile,
    WheelSettings, dinput::NoButtons, guess_pedals, pick_wheel, read_pad,
};

use crate::policy::HapticPolicy;
use crate::sdl::{Device, Devices, Sdl};
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
    /// The d-pad or hat switch on any connected pad or wheel (never the
    /// left stick), in XInput bits ([`rea_launcher::pad::DPAD_UP`]..
    /// [`rea_launcher::pad::DPAD_RIGHT`]), for cabinets with menu buttons.
    pub dpad: u16,
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
    /// A new pad layout: the `PadMap<B>` of the buttons the thread was
    /// started with ([`InputService::set_pad_map`]); another type is
    /// ignored.
    PadMap(Box<dyn std::any::Any + Send>),
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
    pub vid: u16,
    pub pid: u16,
    pub steer_axis: usize,
    pub gas: Option<PedalAxis>,
    pub brake: Option<PedalAxis>,
    pub buttons: WheelButtons,
}

impl WheelInfo {
    fn of_with_buttons<B: Control>(
        name: &str,
        guid: &str,
        p: &WheelProfile<B>,
        buttons: WheelButtons,
    ) -> WheelInfo {
        WheelInfo {
            name: name.to_string(),
            guid: guid.to_string(),
            vid: p.wheel.vid,
            pid: p.wheel.pid,
            steer_axis: p.steer_axis,
            gas: p.gas.map(|(_, a)| a),
            brake: p.brake.map(|(_, a)| a),
            buttons,
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
    /// The first pad input held now on any pad (a trigger past half way
    /// counts), for the hub's rebinding (`PadCapture::tick_held`).
    pub held: Option<PadInput>,
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
        Self::start_with_wheel_file(pad_map, paddles, None)
    }

    /// Starts the thread, preferring a saved `wheel.ini` profile at
    /// `wheel_file` when present and matching the connected wheel's
    /// `(vid, pid)`.
    pub fn start_with_wheel_file<B: Control + Send>(
        pad_map: PadMap<B>,
        paddles: Option<(B, B)>,
        wheel_file: Option<PathBuf>,
    ) -> InputService {
        let (tx, rx) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let shared = Arc::clone(&snapshot);
        let quit = Arc::new(AtomicBool::new(false));
        let quitting = Arc::clone(&quit);
        let thread = std::thread::Builder::new()
            .name("input".into())
            .spawn(move || run(rx, shared, &quitting, pad_map, paddles, wheel_file))
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

    /// Reads pads with `map` from now on (the player changed the layout);
    /// `B` must be the buttons the thread was started with.
    pub fn set_pad_map<B: Control + Send>(&self, map: PadMap<B>) {
        let _ = self.tx.send(Command::PadMap(Box::new(map)));
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
    wheel_file: Option<PathBuf>,
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
    let saved_cfg: Option<(WheelProfile<B>, String)> =
        wheel_file.as_deref().and_then(load_saved_profile);
    let saved_id = saved_cfg.as_ref().map(|(p, _)| &p.wheel);
    let mut devices = Devices::new(&sdl, HapticPolicy::default());
    let mut pads = PadSettings::new(pad_map);
    let mut wheels = WheelSettings::default();
    let mut profile: Option<WheelProfile<B>> = None;
    let mut wheel_buttons = WheelButtons::GENERIC;
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
                Ok(Command::PadMap(map)) => {
                    if let Ok(map) = map.downcast::<PadMap<B>>() {
                        pads.map = *map;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        devices.pump();
        let list = devices.list();
        let wheel = pick_wheel_index(list, saved_id);
        let mut controls = Controls::default();
        if let Some(d) = wheel.map(|w| &list[w]) {
            let fresh = profile
                .as_ref()
                .is_none_or(|p| p.wheel.vid != d.id.vid || p.wheel.pid != d.id.pid);
            let missing_pedals = profile
                .as_ref()
                .is_some_and(|p| p.gas.is_none() && p.brake.is_none())
                && d.state
                    .axes
                    .iter()
                    .skip(1)
                    .any(|v| v.unsigned_abs() > 24_000);
            if fresh || missing_pedals {
                let (mut resolved, buttons) =
                    resolve_wheel_profile(&d.id, &d.initial, &d.state, paddles, saved_cfg.as_ref());
                add_wheel_buttons(&mut resolved, &pads.map);
                wheel_buttons = buttons;
                profile = Some(resolved);
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
            wheel: wheel.zip(profile.as_ref()).map(|(w, p)| {
                WheelInfo::of_with_buttons(&list[w].id.name, &list[w].id.guid, p, wheel_buttons)
            }),
            controls,
            feeding: false,
            error: None,
            nav: list.iter().fold(0, |bits, d| bits | d.nav_bits()),
            held: list
                .iter()
                .filter_map(|d| d.pad)
                .find_map(|p| PadInput::held(&p)),
        };
        let frame = Frame {
            controls,
            pitch,
            dpad: list.iter().fold(0, |bits, d| bits | d.dpad_bits()),
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

/// Adds default wheel button bindings (A/B/X/Y/Start/Back from
/// [`WheelButtons::for_device`]) for any cabinet button bound to those pad
/// inputs in `pad_map` that is not already bound on the wheel (e.g. paddles).
fn add_wheel_buttons<B: Control>(profile: &mut WheelProfile<B>, pad_map: &PadMap<B>) {
    let wb = WheelButtons::for_device(profile.wheel.vid, profile.wheel.pid);
    for &b in B::ALL {
        if profile.buttons.iter().any(|(existing, _)| *existing == b) {
            continue;
        }
        let wheel_btn = pad_map.inputs(Action::Press(b)).find_map(|inp| match inp {
            PadInput::A => Some(wb.a),
            PadInput::B => Some(wb.b),
            PadInput::X => Some(wb.x),
            PadInput::Y => Some(wb.y),
            PadInput::Start => Some(wb.start),
            PadInput::Back => Some(wb.back),
            _ => None,
        });
        if let Some(btn) = wheel_btn {
            profile.buttons.push((
                b,
                Binding {
                    source: Source::Wheel,
                    button: usize::from(btn),
                },
            ));
        }
    }
    profile.buttons.sort_by_key(|(b, _)| *b);
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
            Ok(Command::Output(_) | Command::PadMap(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
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
    find_wheel_with_profile(None)
}

/// Like [`find_wheel`], but prefers a saved `wheel.ini` profile at
/// `wheel_file` when present and matching the connected wheel's `(vid, pid)`.
pub fn find_wheel_with_profile(wheel_file: Option<&Path>) -> Option<WheelInfo> {
    let saved_cfg: Option<(WheelProfile<NoButtons>, String)> =
        wheel_file.and_then(load_saved_profile);
    let sdl = Sdl::init(&[]).ok()?;
    let mut devices = Devices::new(&sdl, HapticPolicy::default());
    for _ in 0..5 {
        devices.pump();
        std::thread::sleep(Duration::from_millis(60));
    }
    let list = devices.list();
    let idx = pick_wheel_index(list, saved_cfg.as_ref().map(|(p, _)| &p.wheel))?;
    let d = &list[idx];
    let (profile, buttons) =
        resolve_wheel_profile(&d.id, &d.initial, &d.state, None, saved_cfg.as_ref());
    Some(WheelInfo::of_with_buttons(
        &d.id.name, &d.id.guid, &profile, buttons,
    ))
}

fn load_saved_profile<B: Control>(path: &Path) -> Option<(WheelProfile<B>, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let profile = WheelProfile::<B>::from_cfg(&text)?;
    Some((profile, text))
}

/// Picks the best wheel in `list` via [`pick_wheel`] (preferring the haptic
/// HID collection on multi-collection wheels like Fanatec CSL DD on Windows).
fn pick_wheel_index(list: &[Device], saved: Option<&DeviceId>) -> Option<usize> {
    let wheels: Vec<(usize, WheelCandidate<'_>)> = list
        .iter()
        .enumerate()
        .filter(|(_, d)| d.kind == Kind::Wheel)
        .map(|(i, d)| {
            (
                i,
                WheelCandidate {
                    vid: d.id.vid,
                    pid: d.id.pid,
                    path: &d.path,
                    haptic: d.haptic,
                },
            )
        })
        .collect();
    let candidates: Vec<WheelCandidate<'_>> = wheels.iter().map(|(_, c)| *c).collect();
    pick_wheel(&candidates, saved, "").map(|w| wheels[w].0)
}

/// Picks the axis sample (`state` after pumping vs `initial` at device open)
/// that actually carries resting pedal values (Windows DirectInput often
/// reports zeros at open before the first poll).
fn best_pedal_sample<'a>(initial: &'a DeviceState, state: &'a DeviceState) -> &'a DeviceState {
    let (sg, sb) = guess_pedals(state);
    if sg.is_some() || sb.is_some() {
        return state;
    }
    let (ig, ib) = guess_pedals(initial);
    if ig.is_some() || ib.is_some() {
        return initial;
    }
    state
}

/// Builds the wheel profile and button table for `id`: prefers a matching
/// saved `wheel.ini` (keeping its calibrated steer/gas/brake axes and any
/// `btn_*` overrides), falling back to guessing from the live/initial state.
fn resolve_wheel_profile<B: Control>(
    id: &DeviceId,
    initial: &DeviceState,
    state: &DeviceState,
    paddles: Option<(B, B)>,
    saved: Option<&(WheelProfile<B>, String)>,
) -> (WheelProfile<B>, WheelButtons) {
    if let Some((saved_profile, saved_text)) = saved
        && saved_profile.wheel.vid == id.vid
        && saved_profile.wheel.pid == id.pid
    {
        let mut profile = saved_profile.clone();
        profile.wheel = id.clone();
        if profile.gas.is_none() && profile.brake.is_none() {
            let (gas, brake) = guess_pedals(best_pedal_sample(initial, state));
            profile.gas = gas.map(|p| (Source::Wheel, p));
            profile.brake = brake.map(|p| (Source::Wheel, p));
        }
        let buttons = WheelButtons::from_cfg(id.vid, id.pid, saved_text);
        return (profile, buttons);
    }
    let sample = best_pedal_sample(initial, state);
    let profile = WheelProfile::guess(id.clone(), sample, paddles);
    let buttons = WheelButtons::for_device(id.vid, id.pid);
    (profile, buttons)
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
            dpad: 0,
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

    #[test]
    fn best_pedal_sample_prefers_live_state_over_all_zero_initial() {
        let initial = DeviceState {
            axes: vec![0; 8],
            buttons: vec![false; 108],
        };
        let state = DeviceState {
            axes: vec![0, 32767, 0, 0, 32767, 0, 0, 0],
            buttons: vec![false; 108],
        };
        let chosen = best_pedal_sample(&initial, &state);
        let (gas, brake) = guess_pedals(chosen);
        assert_eq!(gas.map(|p| p.axis), Some(1));
        assert_eq!(brake.map(|p| p.axis), Some(4));
    }

    #[test]
    fn resolve_wheel_profile_prefers_saved_wheel_ini_even_when_axes_are_zero() {
        let ini = "\
wheel_name=FANATEC Wheel
wheel_vid=0eb7
wheel_pid=0020
wheel_guid=03001464b70e00002000000000000000
steer_axis=0
rotation=1080
gas_device=wheel
gas_axis=1
gas_rest=32767
gas_peak=-21488
brake_device=wheel
brake_axis=4
brake_rest=32767
brake_peak=-32768
btn_start=wheel:9
btn_exit=wheel:8
";
        let saved = (
            WheelProfile::<NoButtons>::from_cfg(ini).unwrap(),
            ini.to_string(),
        );
        let id = DeviceId {
            name: "FANATEC Wheel".into(),
            vid: 0x0eb7,
            pid: 0x0020,
            guid: "03001464b70e00002000000000000000".into(),
        };
        let zeros = DeviceState {
            axes: vec![0; 8],
            buttons: vec![false; 108],
        };
        let (profile, buttons) = resolve_wheel_profile(&id, &zeros, &zeros, None, Some(&saved));
        assert_eq!(
            profile.gas,
            Some((
                Source::Wheel,
                PedalAxis {
                    axis: 1,
                    rest: 32767,
                    peak: -21488,
                }
            ))
        );
        assert_eq!(
            profile.brake,
            Some((
                Source::Wheel,
                PedalAxis {
                    axis: 4,
                    rest: 32767,
                    peak: -32768,
                }
            ))
        );
        assert_eq!(buttons.start, 9);
        assert_eq!(buttons.back, 8);
    }
}
