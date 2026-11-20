//! What a hub launcher plugs into [`super::run`]: its games, how it scans,
//! its settings form, its play session, its controllers and, optionally,
//! an extra page. Everything here is data or a trait the launcher
//! implements; the window owns the threads, the input and the drawing.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

use rea_launcher::catalog::Support;
use rea_launcher::cfg::OptionSpec;
use rea_launcher::hub::{HubConfig, Input, Need};
use rea_launcher::wheel::{Kind, PadInput};

/// One game of the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubGame {
    /// The id: `--play <slug>`, `settings/<slug>.cfg`, `art/<slug>.jpg`
    /// and `art/<slug>-tile.jpg`.
    pub slug: &'static str,
    pub title: &'static str,
    /// The tile's shield: `ID6`.
    pub short: &'static str,
    /// The arcade board: `Lindbergh`.
    pub board: &'static str,
    pub year: u16,
}

/// A game the scan found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub dir: PathBuf,
    /// The edition's name, when the launcher tells them apart.
    pub edition: Option<String>,
}

/// A line of text for the player, and whether it reports a problem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub text: String,
    pub error: bool,
}

impl Note {
    pub fn ok(text: impl Into<String>) -> Note {
        Note {
            text: text.into(),
            error: false,
        }
    }
    pub fn error(text: impl Into<String>) -> Note {
        Note {
            text: text.into(),
            error: true,
        }
    }
}

/// What the running overlay says while a first start sets something up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    /// "Building lindbergh-loader…"
    pub title: String,
    /// What it is and how long it takes.
    pub body: String,
}

/// What the session thread tells the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Progress {
    SettingUp(Stage),
    Playing,
    Ended(Result<(), String>),
}

/// The session thread's line to the window: setup stages, "playing", and
/// the stop request (the Stop button, or the window being closed).
#[derive(Debug, Clone)]
pub struct Reporter {
    tx: Sender<Progress>,
    stop: Arc<AtomicBool>,
}

impl Reporter {
    pub(crate) fn new(tx: Sender<Progress>, stop: Arc<AtomicBool>) -> Reporter {
        Reporter { tx, stop }
    }

    /// A first start sets something up before the game (shown until
    /// [`Reporter::playing`]).
    pub fn setting_up(&self, title: impl Into<String>, body: impl Into<String>) {
        let _ = self.tx.send(Progress::SettingUp(Stage {
            title: title.into(),
            body: body.into(),
        }));
    }

    /// The game is starting: the overlay says so and offers Stop.
    pub fn playing(&self) {
        let _ = self.tx.send(Progress::Playing);
    }

    /// Set when the player asks the game to stop; hand it to the process
    /// supervisor (`rea_launcher_native::supervise`).
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }

    /// Whether the player asked the game to stop.
    pub fn stop_requested(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// A game's session, prepared on the window's thread and run on its own:
/// set up, start the game, supervise it, return how it ended. An `Err`
/// before [`Reporter::playing`] reads "<game> couldn't start", after it
/// "<game> stopped with an error"; both are written to `launcher.log`.
/// Detect a game that ended at once by itself (Windows refusing a program)
/// inside the session and return it as an `Err`.
pub type Session = Box<dyn FnOnce(&Reporter) -> Result<(), String> + Send>;

/// A scan to run on the scan thread, given the library folders.
pub type Scanner<L> = Box<dyn FnOnce(&[PathBuf]) -> L + Send>;

/// A controller, for the top bar and the Controls page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub kind: Kind,
}

/// A button on the Controls page's live check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonView {
    /// A stable id (`start`), for the accessibility tree.
    pub key: &'static str,
    pub label: String,
    pub pressed: bool,
}

/// What the Controls page and the top bar show, read each frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlsView {
    /// Why the controllers can't be read.
    pub error: Option<String>,
    pub devices: Vec<Device>,
    /// The device that drives the car.
    pub driver: Option<usize>,
    /// The pad layout: `(what, keys)`, e.g. `("Gas", "RT")`.
    pub layout: Vec<(String, String)>,
    /// The line under the layout.
    pub layout_note: String,
    /// -1…1.
    pub steer: f32,
    /// 0…1.
    pub accel: f32,
    pub brake: f32,
    pub buttons: Vec<ButtonView>,
    /// A game is running and reads these controls.
    pub feeding: bool,
    /// The game's own readings under the bars, `(label, value)`, e.g.
    /// `("Gear", "3")`; read as "Gear: 3".
    pub lines: Vec<(String, String)>,
}

/// How a checklist line on the extra page reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// "In": a filled sign chip.
    Good,
    /// "Missing": an outlined chip.
    Missing,
    /// "Wrong file": an outlined chip in the error colour.
    Bad,
}

/// One line of the extra page's checklist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub mark: Mark,
    /// The chip's word: "In", "Missing", "Wrong file".
    pub word: &'static str,
    /// "ic5.bin · 2 MB".
    pub text: String,
}

/// The extra page's content for a game (Wangan's board files): read when
/// the page opens and after each submit, never per frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtraPage {
    pub title: String,
    pub intro: String,
    pub checklist: Vec<Check>,
    /// The field's accessible name.
    pub label: &'static str,
    pub placeholder: &'static str,
    /// The line under the field.
    pub hint: &'static str,
    /// The button: "Look for the files".
    pub button: &'static str,
    /// What the field holds when the page opens (`~/Downloads`).
    pub start: String,
}

/// The settings of one game, as the settings page edits them. Rows are
/// the launcher's option table's (labels and descriptions come from it).
pub trait HubForm {
    /// The row's value as shown: `1920 × 1080 (1080p)`.
    fn value(&self, key: &str) -> String;
    /// Why the row can't change now, if it can't (a sentence). Rows the
    /// Linux helper owns are already off on Windows ([`HubWords::not_here`]).
    fn disabled(&self, key: &str) -> Option<String>;
    /// Moves a row's value `d` steps (Press: +1 with `wrap`). Returns
    /// whether anything changed.
    fn adjust(&mut self, key: &str, d: i32, wrap: bool) -> bool;
    /// Writes the settings to `file`, keeping the player's comments and
    /// unknown keys: re-read it with `rea_launcher::cfg::read_file`, refuse
    /// (return the error) when it can't be read, write with
    /// `rea_launcher::fsx::write_atomic`, and change only the keys the
    /// player changed (a value the launcher couldn't read stays as the
    /// player wrote it, never replaced by the default).
    fn save(&self, file: &Path) -> io::Result<()>;
}

/// A hub launcher, as [`super::run`] drives it. Calls come from the
/// window's thread, once per frame for the cheap ones ([`HubBackend::support`],
/// [`HubBackend::need`], [`HubBackend::note`], [`HubBackend::controls`],
/// [`HubBackend::pad`]): keep them to lookups.
pub trait HubBackend: 'static {
    /// What a scan returns (`rea_launcher::library::Scan`).
    type Library: Send + 'static;
    type Form: HubForm;

    fn games(&self) -> &[HubGame];
    /// The option table the settings rows come from (rows: every active,
    /// non-`launcher_only` option, in table order).
    fn options(&self) -> &'static [OptionSpec];
    /// How far a game is from playable on this computer.
    fn support(&self, game: usize) -> Support;
    /// What a game found in the library still needs (opens the extra page).
    fn need(&self, _game: usize) -> Option<Need> {
        None
    }
    /// A line under a playable game's folder: "Needs the NAOMI 2 BIOS: add
    /// it in Settings." (error) or "Test menu off: …" (not).
    fn note(&self, _game: usize, _config: &HubConfig) -> Option<Note> {
        None
    }
    /// Short context or trivia about `game` (release context, what sets
    /// this entry apart), drawn in a card over the top-right quarter of
    /// the hero art on the Games page.
    fn blurb(&self, _game: usize) -> Option<&str> {
        None
    }

    /// A scan of `roots` for the scan thread. Keep the digests between
    /// scans in it (`rea_launcher::library::Library::with_cache` over one
    /// `HashCache`: its clones share it), so only new or changed files are
    /// hashed again; a rescan asked for during a scan runs after it.
    fn scanner(&self) -> Scanner<Self::Library>;
    fn found(library: &Self::Library, game: usize) -> Option<Found>;
    /// One `launcher.log` line per folder holding a build the launcher
    /// doesn't know (its digest).
    fn unknown(library: &Self::Library) -> Vec<String>;

    /// A game's settings, loaded (and, the first time, the recommended
    /// look decided for `screen`, the monitor in pixels when known).
    fn form(&mut self, game: usize, screen: Option<(u32, u32)>) -> Self::Form;
    /// Prepares a game's session; an `Err` is shown as "couldn't start".
    fn session(
        &mut self,
        game: usize,
        library: &Self::Library,
        config: &HubConfig,
        screen: Option<(u32, u32)>,
    ) -> Result<Session, String>;
    /// The game's own log, named when it stops with an error.
    fn game_log(&self, _game: usize) -> Option<PathBuf> {
        None
    }
    /// A session ended (detach the input feeder).
    fn ended(&mut self) {}

    fn controls(&self) -> ControlsView;
    /// The pad's menu steps since the last frame (`Input::from_nav`, and
    /// Y as `Input::Settings`). Drained every frame; dropped while a game
    /// runs, so the game's pad presses never move the hub.
    fn pad(&mut self) -> Vec<Input> {
        Vec::new()
    }

    /// The pad now, for the Controls page: the menu buttons down on any
    /// pad (`feeder::Snapshot::nav`) and the first input held
    /// (`Snapshot::held`). With it, the Controls page gives the pad to its
    /// live check (Back and Start together give it back to the menu) and
    /// can wait for a press to rebind. `None` (the default): the pad moves
    /// the menu on every page.
    fn pad_now(&self) -> Option<(u16, Option<PadInput>)> {
        None
    }

    /// The rows of [`ControlsView::layout`] the player can bind again, in
    /// order (`rea_launcher::hub::bindings::Bindings::rebindable`); none by
    /// default.
    fn rebindable(&self) -> Vec<usize> {
        Vec::new()
    }

    /// Binds layout row `row` to `input` alone, saves the layout and hands
    /// it to the input thread; the note says what changed or why not.
    fn rebind(&mut self, _row: usize, _input: PadInput) -> Note {
        Note::error("This launcher's buttons can't be changed.")
    }

    /// Back to the launcher's pad layout, saved and handed to the input
    /// thread.
    fn reset_bindings(&mut self) -> Note {
        Note::error("This launcher's buttons can't be changed.")
    }

    /// The extra page for a game that [`HubBackend::need`]s something.
    fn extra(&self, _game: usize) -> Option<ExtraPage> {
        None
    }
    /// The player pressed the extra page's button with `typed` in its field.
    fn submit_extra(&mut self, _game: usize, _typed: &str) -> Note {
        Note::error("Nothing to add here.")
    }
}

/// The words that differ between hubs (every other line is the SDK's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubWords {
    /// The setup page's first paragraph.
    pub setup_intro: &'static str,
    /// The settings page's button that opens the setup page: "Folders and BIOS".
    pub folders_button: &'static str,
    /// Why a row the Linux helper owns is off on Windows.
    pub not_here: &'static str,
}

impl HubWords {
    pub const DEFAULT: HubWords = HubWords {
        setup_intro: "Type the path of a folder that holds your game folders and add it. \
             The launcher looks inside for each game and never changes what's there.",
        folders_button: "Folders and files",
        not_here: "On Windows the game sets its own screen mode; this option is for \
             gamescope on Linux.",
    };
}

impl Default for HubWords {
    fn default() -> HubWords {
        HubWords::DEFAULT
    }
}
