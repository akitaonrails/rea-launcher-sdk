//! The input thread and the two feeds, with sinks the test reads back and
//! a sink that breaks like a pipe whose reader quit. The thread's device
//! loop is tested only on a machine without controllers (it would play
//! force feedback on a real wheel); its SDL-less loop always.
#![cfg(feature = "feeder")]

use std::io::{self, Write};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rea_launcher::wheel::{Control, PadMap, Script};
use rea_launcher_native::feeder::{
    Command, Feeding, Frame, InputService, Scripted, Snapshot, Tuning, Wire,
};
use rea_launcher_native::sdl::Sdl;
use rea_launcher_native::supervise::Feed;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Button {
    Start,
}

impl Control for Button {
    const ALL: &'static [Button] = &[Button::Start];
    fn key(self) -> &'static str {
        "start"
    }
    fn label(self) -> &'static str {
        "Start"
    }
    fn bit(self) -> u32 {
        1
    }
}

/// `p1 <buttons> <steer>` lines; `quit` at the end.
struct Line;

impl Wire for Line {
    fn encode(&mut self, f: &Frame) -> String {
        format!("p1 {} {}\n", f.controls.buttons, f.controls.steer)
    }
    fn quit(&self) -> Option<&'static str> {
        Some("quit")
    }
}

#[derive(Clone, Default)]
struct Text(Arc<Mutex<Vec<u8>>>);

impl Write for Text {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Text {
    fn get(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// A pipe whose reader has quit.
struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn until(what: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if what() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn attach(s: &InputService, sink: impl Write + Send + 'static) {
    s.sender()
        .send(Command::Attach {
            sink: Box::new(sink),
            wire: Box::new(Line),
            tuning: Tuning::default(),
        })
        .unwrap();
}

/// Feeds a game, lets it go, then feeds one whose input breaks.
fn feeds_and_lets_go(s: &InputService) {
    let text = Text::default();
    attach(s, text.clone());
    assert!(
        until(|| text.get().matches("p1 0 0\n").count() >= 2),
        "neutral input, refreshed"
    );
    assert!(s.snapshot().feeding);
    s.sender().send(Command::Detach).unwrap();
    assert!(until(|| !s.snapshot().feeding));
    attach(s, Broken);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!s.snapshot().feeding, "a broken input is let go");
}

#[test]
fn the_input_thread_feeds_with_and_without_sdl() {
    let snap = |s: &InputService| -> Snapshot { s.snapshot() };
    let service = InputService::start(PadMap::<Button>::default(), None);
    std::thread::sleep(Duration::from_millis(200));
    let first = snap(&service);
    if first.error.is_none() && first.devices.is_empty() {
        feeds_and_lets_go(&service);
    }
    drop(service);

    // SDL held elsewhere: the thread still feeds neutral input
    let _sdl = Sdl::init(&[]).unwrap();
    let service = InputService::start(PadMap::<Button>::default(), None);
    assert!(until(|| snap(&service).error.is_some()));
    feeds_and_lets_go(&service);
}

#[test]
fn feeding_hands_the_input_and_the_force_feedback_to_the_thread() {
    let (tx, rx) = mpsc::channel();
    let mut feed = Feeding::new(tx, Box::new(Line), Tuning::default());
    let mut outputs = feed.outputs().unwrap();
    feed.attach(Some(Box::new(Text::default())));
    assert!(matches!(rx.try_recv(), Ok(Command::Attach { .. })));
    assert!(outputs("out 0 1 5000"));
    assert!(!outputs("wine: fixme"));
    assert!(matches!(rx.try_recv(), Ok(Command::Output(l)) if l == "out 0 1 5000"));
    feed.detach();
    assert!(matches!(rx.try_recv(), Ok(Command::Detach)));
    // no input to feed: nothing attached
    let (tx, rx) = mpsc::channel();
    Feeding::new(tx, Box::new(Line), Tuning::default()).attach(None);
    assert!(rx.try_recv().is_err());
}

#[test]
fn a_script_writes_its_steps_then_quits() {
    let script = Script::parse::<Button>("0 -\n1 start steer=0.5\n").unwrap();
    let mut feed = Scripted::new(Box::new(Line), script);
    let text = Text::default();
    feed.attach(Some(Box::new(text.clone())));
    let mut outputs = feed.outputs().unwrap();
    assert!(
        outputs("out 0 1 5000"),
        "the game's force feedback is dropped"
    );
    assert!(!outputs("loaded"));
    for ms in [0, 10, 20, 1000, 1010] {
        feed.tick(Duration::from_millis(ms));
    }
    feed.detach();
    feed.detach();
    assert_eq!(text.get(), "p1 0 0\np1 1 0.5\nquit\n");
}

#[test]
fn dropping_the_service_ends_its_thread_while_senders_live() {
    let service = InputService::start(PadMap::<Button>::default(), None);
    let kept = service.sender();
    std::thread::sleep(Duration::from_millis(100));
    let (done, dropped) = mpsc::channel();
    std::thread::spawn(move || {
        drop(service);
        done.send(()).unwrap();
    });
    assert!(
        dropped.recv_timeout(Duration::from_secs(5)).is_ok(),
        "the drop waited for every sender"
    );
    drop(kept);
}
