//! One run of a session, with shell scripts standing in for the game: the
//! log, the feed, the watcher, the stop and its grace. Linux only (the
//! Windows half of the loop is `group`'s, tested in `group_windows`).
#![cfg(all(feature = "group", target_os = "linux"))]

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rea_launcher_native::group::{Group, Tag};
use rea_launcher_native::supervise::{self, Ended, Feed, Hold, Options, Outcome, Watcher};

static RUN: AtomicUsize = AtomicUsize::new(0);

/// A log the test reads back.
#[derive(Clone, Default)]
struct Text(Arc<Mutex<Vec<u8>>>);

impl Write for Text {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Text {
    fn get(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

/// Runs `script` (sh) as a session until it ends or `stop` is set.
fn run(
    script: &str,
    stdin: bool,
    feed: &mut dyn Feed,
    watcher: &mut dyn Watcher,
    stop: &AtomicBool,
    grace: Duration,
) -> (Outcome, String) {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", script])
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let tag = format!(
        "{}-{}",
        std::process::id(),
        RUN.fetch_add(1, Ordering::SeqCst)
    );
    let group = Group::spawn(&mut cmd, &Tag::new("REA_NATIVE_TEST_RUN", tag)).unwrap();
    let text = Text::default();
    let opts = Options {
        stop,
        log: Arc::new(Mutex::new(text.clone())),
        grace,
        sink: None,
    };
    let out = supervise::run(group, feed, watcher, opts).unwrap();
    (out, text.get())
}

/// Sets `flag` after `after`.
fn later(flag: &Arc<AtomicBool>, after: Duration) {
    let flag = Arc::clone(flag);
    std::thread::spawn(move || {
        std::thread::sleep(after);
        flag.store(true, Ordering::SeqCst);
    });
}

const LONG: Duration = Duration::from_secs(10);

#[test]
fn a_game_that_ends_by_itself_is_logged_with_its_end() {
    let stop = AtomicBool::new(false);
    let (out, log) = run(
        "echo hello; echo trouble >&2; exit 3",
        false,
        &mut Hold::default(),
        &mut (),
        &stop,
        LONG,
    );
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(out.ended, Ended::Itself);
    assert!(out.ended_early(Duration::from_secs(20)));
    assert!(!out.ended_early(Duration::ZERO));
    assert!(
        log.contains("hello\n") && log.contains("trouble\n"),
        "{log}"
    );
    assert!(
        log.contains("# exit: exit status: 3 after 0 s; stopped 0 leftover process(es)"),
        "{log}"
    );
}

#[test]
fn without_a_feed_the_input_stays_open_until_the_stop() {
    // a loader that quits when its input closes, and ignores SIGTERM
    let stop = Arc::new(AtomicBool::new(false));
    later(&stop, Duration::from_millis(700));
    let started = Instant::now();
    let (out, log) = run(
        "trap '' TERM; read line; echo \"input closed $?\"",
        true,
        &mut Hold::default(),
        &mut (),
        &stop,
        LONG,
    );
    assert!(started.elapsed() >= Duration::from_millis(700), "{log}");
    assert!(started.elapsed() < LONG, "it waited out the grace: {log}");
    assert_eq!(out.ended, Ended::Stopped);
    assert_eq!(out.status.code(), Some(0), "{log}");
    assert!(log.contains("input closed 1"), "{log}");
}

#[test]
fn with_its_own_sink_the_leaders_piped_input_still_stays_open_until_the_stop() {
    // Dolphin's pipe is the input; the loader in front still quits when
    // its own stdin closes
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "trap '' TERM; read line; echo \"input closed $?\""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let tag = format!("{}-sink", std::process::id());
    let group = Group::spawn(&mut cmd, &Tag::new("REA_NATIVE_TEST_RUN", tag)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    later(&stop, Duration::from_millis(500));
    let text = Text::default();
    let opts = Options {
        stop: &stop,
        log: Arc::new(Mutex::new(text.clone())),
        grace: LONG,
        sink: Some(Box::new(Text::default())),
    };
    let out = supervise::run(group, &mut Hold::default(), &mut (), opts).unwrap();
    let log = text.get();
    assert_eq!(out.ended, Ended::Stopped, "{log}");
    assert!(log.contains("input closed 1"), "{log}");
}

#[test]
fn a_game_that_ignores_the_stop_is_killed_after_the_grace() {
    use std::os::unix::process::ExitStatusExt;
    let stop = Arc::new(AtomicBool::new(false));
    later(&stop, Duration::from_millis(200));
    let started = Instant::now();
    let (out, _) = run(
        "trap '' TERM; while :; do sleep 0.05; done",
        false,
        &mut Hold::default(),
        &mut (),
        &stop,
        Duration::from_millis(400),
    );
    assert!(started.elapsed() >= Duration::from_millis(600));
    assert_eq!(out.status.signal(), Some(9));
    assert_eq!(out.ended, Ended::Stopped);
}

/// Records what the run gave it; force feedback lines are its own.
#[derive(Default)]
struct Recorder {
    sink: Option<Box<dyn Write + Send>>,
    taken: Arc<Mutex<Vec<String>>>,
    ticks: usize,
    detached: usize,
}

impl Feed for Recorder {
    fn attach(&mut self, sink: Option<Box<dyn Write + Send>>) {
        self.sink = sink;
        let _ = writeln!(self.sink.as_mut().unwrap(), "p1 0 0 0 0 0 0");
    }
    fn outputs(&mut self) -> Option<supervise::Outputs> {
        let taken = Arc::clone(&self.taken);
        Some(Box::new(move |line| {
            let ffb = line.starts_with("out ");
            if ffb {
                taken.lock().unwrap().push(line.to_string());
            }
            ffb
        }))
    }
    fn tick(&mut self, _ran: Duration) {
        self.ticks += 1;
    }
    fn detach(&mut self) {
        self.detached += 1;
        self.sink = None;
    }
}

#[test]
fn the_feed_gets_the_input_and_its_lines_the_log_the_rest() {
    let stop = AtomicBool::new(false);
    let mut feed = Recorder::default();
    let (out, log) = run(
        "read line; echo \"got $line\"; echo 'out 0 1 5000'; sleep 0.2",
        true,
        &mut feed,
        &mut (),
        &stop,
        LONG,
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(log.contains("got p1 0 0 0 0 0 0\n"), "{log}");
    assert!(!log.contains("out 0 1"), "{log}");
    assert_eq!(*feed.taken.lock().unwrap(), ["out 0 1 5000"]);
    assert!(feed.ticks > 5);
    assert_eq!(feed.detached, 1);
}

/// Stops the game when it says it hung; after a natural exit, waits for
/// the line that says it saved.
#[derive(Default)]
struct Watch {
    hung: bool,
    saved: bool,
    asked: bool,
    after_exit: bool,
}

impl Watcher for Watch {
    fn line(&mut self, line: &str) {
        self.hung |= line == "HANG";
        self.saved |= line == "saved";
    }
    fn check(&mut self, _g: &Group) -> Option<String> {
        self.hung.then(|| "the boot hung".to_string())
    }
    fn ask_to_stop(&mut self, g: &Group) {
        self.asked = true;
        g.terminate();
    }
    fn saving(&mut self, _g: &Group) -> bool {
        !self.saved
    }
    fn saves_after_exit(&self) -> bool {
        self.after_exit
    }
}

#[test]
fn the_watcher_stops_a_hung_game_and_ends_the_grace_once_it_saved() {
    let stop = AtomicBool::new(false);
    let mut w = Watch::default();
    let started = Instant::now();
    let (out, log) = run(
        "trap 'echo saved; sleep 5' TERM; echo HANG; while :; do sleep 0.05; done",
        false,
        &mut Hold::default(),
        &mut w,
        &stop,
        LONG,
    );
    assert_eq!(out.ended, Ended::Watcher("the boot hung".into()));
    assert!(w.asked && w.saved);
    assert!(started.elapsed() < Duration::from_secs(5), "{log}");
    assert!(log.contains("# the boot hung: stopping\n"), "{log}");
}

#[test]
fn what_is_left_after_a_natural_exit_gets_its_time_to_save() {
    let stop = AtomicBool::new(false);
    let script = "(trap '' TERM; sleep 1; echo saved >&2; sleep 30) & exit 0";
    let mut w = Watch {
        after_exit: true,
        ..Watch::default()
    };
    let (out, log) = run(script, false, &mut Hold::default(), &mut w, &stop, LONG);
    assert!(w.saved, "{log}");
    assert_eq!(out.ended, Ended::Itself);
    assert!(log.contains("saved\n"), "{log}");
    // without the wait, what is left is killed before it saved
    let mut w = Watch::default();
    let (_, log) = run(script, false, &mut Hold::default(), &mut w, &stop, LONG);
    assert!(!w.saved, "{log}");
}
