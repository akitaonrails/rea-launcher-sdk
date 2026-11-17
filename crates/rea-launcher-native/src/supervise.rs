//! One run of a game session, from start to nothing left: the loop the
//! TeknoParrot hubs' `session::run` shared (sega's, the most complete).
//!
//! It polls every 10 ms. The leader's output lines go to the session log
//! and the [`Watcher`] (stdout lines the [`Feed`] takes, windowsarcadeloader's
//! force feedback, go to it instead). A stop request (the flag, or the
//! watcher) detaches the feed (closing the loader's input asks it to end),
//! asks the game politely ([`Watcher::ask_to_stop`]), gives it `grace` to
//! save, then kills the session. After a natural exit what is left gets
//! the grace too when the watcher says a save may still be under way.
//! Without a feed, [`Hold`] keeps the loader's input open until the stop:
//! a closed input tells windowsarcadeloader to quit at once.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::group::Group;

/// The session log, shared with whatever else writes to it.
pub type Log = Arc<Mutex<dyn Write + Send>>;

/// Takes the stdout lines meant for a [`Feed`] (true: taken, not logged).
pub type Outputs = Box<dyn FnMut(&str) -> bool + Send>;

/// What drives the game's input.
pub trait Feed {
    /// The game started; `sink` is its input, when it has one.
    fn attach(&mut self, sink: Option<Box<dyn Write + Send>>);
    /// What takes the stdout lines meant for the feed; asked once, run on
    /// the thread that reads the output.
    fn outputs(&mut self) -> Option<Outputs> {
        None
    }
    /// Every poll, `ran` after the start.
    fn tick(&mut self, _ran: Duration) {}
    /// The game is stopping or has ended: let go of its input (once).
    fn detach(&mut self);
}

/// No feed: the game's input is held open until the stop.
#[derive(Default)]
pub struct Hold(Option<Box<dyn Write + Send>>);

impl Feed for Hold {
    fn attach(&mut self, sink: Option<Box<dyn Write + Send>>) {
        self.0 = sink;
    }

    fn detach(&mut self) {
        self.0 = None;
    }
}

/// The game knowledge a run needs (F355's boards, a boot hang, a first
/// start's restart, the line that says a save is done). Every method has
/// the plain default.
pub trait Watcher {
    /// A line the game wrote (stdout the feed didn't take, and stderr).
    fn line(&mut self, _line: &str) {}
    /// Every poll while the game runs: why it must stop now, if it must.
    fn check(&mut self, _group: &Group) -> Option<String> {
        None
    }
    /// Asks the game to stop: [`Group::terminate`] by default.
    fn ask_to_stop(&mut self, group: &Group) {
        group.terminate();
    }
    /// Whether the stopping game may still be saving (the grace goes on
    /// while it is, at most `grace`).
    fn saving(&mut self, _group: &Group) -> bool {
        true
    }
    /// Whether what is left after a natural exit gets the grace to save.
    fn saves_after_exit(&self) -> bool {
        false
    }
}

/// No game knowledge.
impl Watcher for () {}

pub struct Options<'a> {
    /// Set (by the window's Stop, a closed window, a signal) to stop.
    pub stop: &'a AtomicBool,
    pub log: Log,
    /// How long a stopping game gets to save and quit.
    pub grace: Duration,
    /// The game's input when it is not the leader's stdin (Dolphin's
    /// pipe); `None` uses the leader's stdin, if it was piped.
    pub sink: Option<Box<dyn Write + Send>>,
}

/// Why a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The game quit (or was closed) by itself.
    Itself,
    /// The stop flag.
    Stopped,
    /// The watcher's reason.
    Watcher(String),
}

#[derive(Debug)]
pub struct Outcome {
    pub status: ExitStatus,
    /// From the start to the leader's end.
    pub ran: Duration,
    pub ended: Ended,
    /// The session's processes still running at the end, killed.
    pub leftovers: usize,
}

impl Outcome {
    /// Whether the game ended by itself within `limit` of its start: it
    /// didn't start (a missing runtime, a refused program).
    pub fn ended_early(&self, limit: Duration) -> bool {
        self.ended == Ended::Itself && self.ran < limit
    }
}

const POLL: Duration = Duration::from_millis(10);
const GRACE_POLL: Duration = Duration::from_millis(100);
/// What is left after a natural exit gets this long before it is killed.
const SETTLE: Duration = Duration::from_millis(500);

/// Runs the session `group` until it ends or is stopped, then ends
/// everything it left. The `# exit:` line goes to the log.
pub fn run(
    mut group: Group,
    feed: &mut dyn Feed,
    watcher: &mut dyn Watcher,
    opts: Options,
) -> std::io::Result<Outcome> {
    let start = Instant::now();
    let pipes = group.take_pipes();
    if let Some(w) = group.warning() {
        log(&opts.log, &format!("# {w}"));
    }
    let lines = Lines::read(pipes.stdout, pipes.stderr, feed.outputs());
    let stdin = pipes.stdin.map(|s| Box::new(s) as Box<dyn Write + Send>);
    feed.attach(opts.sink.or(stdin));

    let see = |watcher: &mut dyn Watcher| lines.drain(&opts.log, watcher);
    let mut ended = Ended::Itself;
    let mut detached = false;
    let status = loop {
        see(watcher);
        if let Some(st) = group.exited()? {
            break st;
        }
        let why = if opts.stop.load(Ordering::SeqCst) {
            Some(Ended::Stopped)
        } else {
            watcher.check(&group).map(Ended::Watcher)
        };
        if let Some(why) = why {
            if let Ended::Watcher(reason) = &why {
                log(&opts.log, &format!("# {reason}: stopping"));
            }
            ended = why;
            feed.detach();
            detached = true;
            watcher.ask_to_stop(&group);
            let deadline = Instant::now() + opts.grace;
            while Instant::now() < deadline && group.exited()?.is_none() && watcher.saving(&group) {
                std::thread::sleep(GRACE_POLL);
                see(watcher);
            }
            group.kill();
            break group.wait_exit()?;
        }
        feed.tick(start.elapsed());
        std::thread::sleep(POLL);
    };
    let ran = start.elapsed();
    if !detached {
        feed.detach();
    }
    group.terminate();
    if ended == Ended::Itself && watcher.saves_after_exit() {
        let deadline = Instant::now() + opts.grace;
        while Instant::now() < deadline && watcher.saving(&group) && group.any_left() {
            std::thread::sleep(GRACE_POLL);
            see(watcher);
        }
    } else {
        std::thread::sleep(SETTLE);
    }
    group.kill();
    see(watcher);
    let leftovers = group.finish()?.leftovers;
    log(
        &opts.log,
        &format!(
            "# exit: {status} after {} s; stopped {leftovers} leftover process(es)",
            ran.as_secs()
        ),
    );
    Ok(Outcome {
        status,
        ran,
        ended,
        leftovers,
    })
}

fn log(log: &Log, line: &str) {
    if let Ok(mut f) = log.lock() {
        let _ = writeln!(f, "{line}");
    }
}

/// The leader's output lines, read on their own threads.
struct Lines(Receiver<String>);

impl Lines {
    fn read(
        stdout: Option<impl Read + Send + 'static>,
        stderr: Option<impl Read + Send + 'static>,
        mut outputs: Option<Outputs>,
    ) -> Lines {
        let (tx, rx) = mpsc::channel();
        if let Some(out) = stdout {
            let tx = tx.clone();
            forward(out, tx, move |line| {
                outputs.as_mut().is_some_and(|take| take(line))
            });
        }
        if let Some(err) = stderr {
            forward(err, tx, |_| false);
        }
        Lines(rx)
    }

    /// The lines read so far, to the log and the watcher.
    fn drain(&self, log_to: &Log, watcher: &mut dyn Watcher) {
        for line in self.0.try_iter() {
            watcher.line(&line);
            log(log_to, &line);
        }
    }
}

/// Sends `from`'s lines that `taken` doesn't take to `tx`, until it ends.
fn forward(
    from: impl Read + Send + 'static,
    tx: Sender<String>,
    mut taken: impl FnMut(&str) -> bool + Send + 'static,
) {
    std::thread::spawn(move || {
        for line in BufReader::new(from).lines().map_while(Result::ok) {
            if !taken(&line) && tx.send(line).is_err() {
                return;
            }
        }
    });
}
