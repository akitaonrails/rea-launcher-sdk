//! The hub window against a fake launcher: fake games found as folders
//! named after their slug, a fake settings form, sessions the test
//! scripts. Ported from the four TeknoParrot launchers' window tests.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::accesskit::Role;
use eframe::egui::{self, ViewportCommand};
use rea_launcher::catalog::Support;
use rea_launcher::cfg::{self, Kind, OptionSpec};
use rea_launcher::hub::{
    Focus, HubConfig, HubPaths, Input, Need, Page, PathField, PathKind, Tab, display_path,
};

use super::testing::{self, SIZE_720, SIZE_800, says};
use super::*;
use crate::testing::{Harness, NodeT, Queryable};

// ------------------------------------------------------------ the fake launcher

const GAMES: [HubGame; 5] = [
    HubGame {
        slug: "ar1",
        title: "Arcade Racer",
        short: "AR1",
        board: "NAOMI 2",
        year: 2001,
    },
    HubGame {
        slug: "ar2",
        title: "Arcade Racer 2",
        short: "AR2",
        board: "Lindbergh",
        year: 2006,
    },
    HubGame {
        slug: "ar3",
        title: "Arcade Racer 3 Extreme Edition",
        short: "AR3",
        board: "Lindbergh",
        year: 2007,
    },
    HubGame {
        slug: "ar4",
        title: "Arcade Racer 4",
        short: "AR4",
        board: "RingEdge",
        year: 2010,
    },
    HubGame {
        slug: "ar5",
        title: "Arcade Racer 5",
        short: "AR5",
        board: "N2",
        year: 2012,
    },
];

/// The game that needs the BIOS path (field 0) to be set.
const BIOS_GAME: usize = 0;
/// The game that needs board files (the extra page).
const FILES_GAME: usize = 4;

/// Longer than a row has room for.
const SHARPNESS_OFF: &str = "Only FSR 1 and NVIDIA Image Scaling use sharpness; \
     choose one of them as the upscaler above to change how crisp the edges are.";

const UPSCALERS: &[(&str, &str)] = &[
    ("fsr", "FSR 1"),
    ("nis", "NVIDIA Image Scaling"),
    ("linear", "Smooth"),
];
const RENDERS: &[(&str, &str)] = &[("native", "Native"), ("1080p", "1920 × 1080 (1080p)")];

static OPTIONS: &[OptionSpec] = &[
    OptionSpec::new(
        "render",
        Kind::Choice(RENDERS),
        "Game render size",
        "The size the game draws at.",
    ),
    OptionSpec::new(
        "upscaler",
        Kind::Choice(UPSCALERS),
        "Upscaler",
        "How the picture is stretched.",
    ),
    OptionSpec::new(
        "sharpness",
        Kind::Range { min: 0, max: 20 },
        "Sharpness",
        "How crisp the upscaler makes edges.",
    ),
    OptionSpec::new(
        "fullscreen",
        Kind::Toggle,
        "Fullscreen",
        "Fill the whole screen.",
    )
    .helper_owned(),
    OptionSpec::new(
        "deadzone",
        Kind::Range { min: 0, max: 30 },
        "Dead zone",
        "How far a stick moves before the car steers.",
    ),
    OptionSpec::new("crt", Kind::Toggle, "CRT", "Scanlines.").launcher_only(),
];

const BIOS: PathField = PathField {
    key: "naomi2_bios",
    aliases: &["bios_file"],
    kind: PathKind::File,
    flag: "--naomi2-bios",
    flag_aliases: &[],
    help: "where your naomi2.zip is",
    heading: "NAOMI 2 BIOS (naomi2.zip), needed for Arcade Racer",
    label: "NAOMI 2 BIOS file",
    placeholder: "/path/to/naomi2.zip",
    button: "Use BIOS",
    empty: "Type the path of naomi2.zip first.",
    saved: "NAOMI 2 BIOS saved.",
};
static FIELDS: [PathField; 1] = [BIOS];

#[derive(Default)]
struct Library {
    found: Vec<Option<Found>>,
    unknown: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct Form {
    render: &'static str,
    upscaler: &'static str,
    sharpness: u32,
    fullscreen: bool,
    deadzone: u32,
}

impl Default for Form {
    fn default() -> Form {
        Form {
            render: "native",
            upscaler: "fsr",
            sharpness: 5,
            fullscreen: true,
            deadzone: 5,
        }
    }
}

fn choice(table: &[(&'static str, &'static str)], cur: &str, d: i32, wrap: bool) -> &'static str {
    let all: Vec<&'static str> = table.iter().map(|(v, _)| *v).collect();
    let cur = all.iter().copied().find(|v| *v == cur).unwrap_or(all[0]);
    rea_launcher::hub::pick(&all, cur, d, wrap)
}

fn label_of(table: &[(&str, &'static str)], v: &str) -> String {
    table
        .iter()
        .find(|(k, _)| *k == v)
        .map_or(v.into(), |(_, l)| (*l).into())
}

impl Form {
    fn load(file: &Path) -> Form {
        let mut f = Form::default();
        let text = cfg::read_file(file).ok().flatten().unwrap_or_default();
        for e in cfg::entries(&text) {
            let Some(spec) = cfg::spec::find(OPTIONS, e.key) else {
                continue;
            };
            match (e.key, spec.parse(e.value)) {
                ("render", Ok(cfg::Value::Choice(v))) => f.render = v,
                ("upscaler", Ok(cfg::Value::Choice(v))) => f.upscaler = v,
                ("sharpness", Ok(cfg::Value::Number(n))) => f.sharpness = n,
                ("fullscreen", Ok(cfg::Value::Toggle(b))) => f.fullscreen = b,
                ("deadzone", Ok(cfg::Value::Number(n))) => f.deadzone = n,
                _ => {}
            }
        }
        f
    }
}

impl HubForm for Form {
    fn value(&self, key: &str) -> String {
        match key {
            "render" => label_of(RENDERS, self.render),
            "upscaler" => label_of(UPSCALERS, self.upscaler),
            "sharpness" => self.sharpness.to_string(),
            "fullscreen" => if self.fullscreen { "On" } else { "Off" }.into(),
            "deadzone" => format!("{}%", self.deadzone),
            _ => String::new(),
        }
    }

    fn disabled(&self, key: &str) -> Option<String> {
        (key == "sharpness" && self.upscaler == "linear").then(|| SHARPNESS_OFF.into())
    }

    fn adjust(&mut self, key: &str, d: i32, wrap: bool) -> bool {
        let before = self.clone();
        match key {
            "render" => self.render = choice(RENDERS, self.render, d, wrap),
            "upscaler" => self.upscaler = choice(UPSCALERS, self.upscaler, d, wrap),
            "sharpness" => self.sharpness = (self.sharpness as i32 + d).clamp(0, 20) as u32,
            "fullscreen" => self.fullscreen = !self.fullscreen,
            "deadzone" => self.deadzone = (self.deadzone as i32 + d).clamp(0, 30) as u32,
            _ => {}
        }
        *self != before
    }

    fn save(&self, file: &Path) -> io::Result<()> {
        let mut text = cfg::read_file(file)?.unwrap_or_default();
        for (k, v) in [
            ("render", self.render.to_string()),
            ("upscaler", self.upscaler.to_string()),
            ("sharpness", self.sharpness.to_string()),
            ("fullscreen", cfg::on_off(self.fullscreen).to_string()),
            ("deadzone", self.deadzone.to_string()),
        ] {
            text = cfg::set_key(&text, k, &v);
        }
        rea_launcher::fsx::write_atomic(file, text)
    }
}

struct Fake {
    paths: HubPaths,
    supports: Vec<Support>,
    /// Board files present (the files game needs them).
    files_in: bool,
    /// The next Play's session; none: it can't start.
    next: Option<Session>,
    controls: ControlsView,
    pads: Vec<Input>,
    extra_reads: std::cell::Cell<usize>,
    ended: usize,
    /// Scans started, shared with the scan threads (the launcher's hash cache).
    scans: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Fake {
    fn new(paths: HubPaths) -> Fake {
        Fake {
            paths,
            supports: vec![Support::Ready; GAMES.len()],
            files_in: false,
            next: None,
            controls: ControlsView::default(),
            pads: Vec::new(),
            extra_reads: std::cell::Cell::new(0),
            ended: 0,
            scans: Default::default(),
        }
    }
}

/// The fake scan: a folder named after a game's slug is that game; one
/// named `mystery` holds an unknown build.
fn scan(roots: &[PathBuf]) -> Library {
    std::thread::sleep(std::time::Duration::from_millis(30));
    let mut lib = Library {
        found: vec![None; GAMES.len()],
        unknown: Vec::new(),
    };
    for root in roots {
        for (i, g) in GAMES.iter().enumerate() {
            let dir = root.join(g.slug);
            if dir.is_dir() {
                lib.found[i] = Some(Found {
                    dir,
                    edition: Some("Export".into()),
                });
            }
        }
        if root.join("mystery").is_dir() {
            lib.unknown
                .push(format!("unknown build game.exe 00ff in {}", root.display()));
        }
    }
    lib
}

impl HubBackend for Fake {
    type Library = Library;
    type Form = Form;

    fn games(&self) -> &[HubGame] {
        &GAMES
    }
    fn options(&self) -> &'static [OptionSpec] {
        OPTIONS
    }
    fn support(&self, game: usize) -> Support {
        self.supports[game]
    }
    fn need(&self, game: usize) -> Option<Need> {
        (game == FILES_GAME && !self.files_in).then(|| Need {
            chip: "Needs board files",
            badge: "Add files",
            action: "Add files",
            reason: "Also needs the board's files.".into(),
        })
    }
    fn note(&self, game: usize, config: &HubConfig) -> Option<Note> {
        (game == BIOS_GAME && config.paths[0].is_none())
            .then(|| Note::error("Needs the NAOMI 2 BIOS: add it in Settings."))
    }

    fn scanner(&self) -> Scanner<Library> {
        let scans = std::sync::Arc::clone(&self.scans);
        Box::new(move |roots| {
            scans.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            scan(roots)
        })
    }
    fn found(library: &Library, game: usize) -> Option<Found> {
        library.found[game].clone()
    }
    fn unknown(library: &Library) -> Vec<String> {
        library.unknown.clone()
    }

    fn form(&mut self, game: usize, _screen: Option<(u32, u32)>) -> Form {
        Form::load(&self.paths.settings(GAMES[game].slug))
    }
    fn session(
        &mut self,
        _game: usize,
        _library: &Library,
        _config: &HubConfig,
        _screen: Option<(u32, u32)>,
    ) -> Result<Session, String> {
        self.next
            .take()
            .ok_or_else(|| "the game needs launcher components that are missing".into())
    }
    fn game_log(&self, game: usize) -> Option<PathBuf> {
        Some(self.paths.runtime(GAMES[game].slug).join("session.log"))
    }
    fn ended(&mut self) {
        self.ended += 1;
    }
    fn controls(&self) -> ControlsView {
        self.controls.clone()
    }
    fn pad(&mut self) -> Vec<Input> {
        std::mem::take(&mut self.pads)
    }
    fn extra(&self, game: usize) -> Option<ExtraPage> {
        // counted: the window must not ask every frame
        self.extra_reads.set(self.extra_reads.get() + 1);
        (game == FILES_GAME).then(|| ExtraPage {
            title: "Add the board files".into(),
            intro: "Arcade Racer 5 needs these files from the original board.".into(),
            checklist: vec![Check {
                mark: if self.files_in {
                    Mark::Good
                } else {
                    Mark::Missing
                },
                word: if self.files_in { "In" } else { "Missing" },
                text: "ic5.bin · 2 MB".into(),
            }],
            label: "Folder with the files",
            placeholder: "/path/to/your/files",
            hint: "A folder or a zip. Press Enter to look.",
            button: "Look for the files",
            start: "~/Downloads".into(),
        })
    }
    fn submit_extra(&mut self, _game: usize, typed: &str) -> Note {
        if Path::new(typed.trim()).join("ic5.bin").is_file() {
            self.files_in = true;
            Note::ok("All 1 files are in.")
        } else {
            Note::error(format!("None of the missing files were found in {typed}."))
        }
    }
}

// ------------------------------------------------------------ helpers

const THEME: HubTheme = super::theme::tests::MOUNTAIN;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rea-hub-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn paths_in(t: &Path) -> HubPaths {
    HubPaths {
        data: t.join("data"),
        components: t.join("components"),
        dev_assets: None,
    }
}

fn spec(t: &Path, layout: HubLayout) -> HubSpec {
    HubSpec {
        theme: THEME,
        layout,
        words: HubWords {
            folders_button: "Folders and BIOS",
            ..HubWords::DEFAULT
        },
        fields: &FIELDS,
        paths: paths_in(t),
        version: "1.2.3",
    }
}

type H = Harness<'static, HubApp<Fake>>;

/// A window over a launcher with `libraries` saved, at `size`.
fn window_sized(t: &Path, libraries: &[PathBuf], size: egui::Vec2) -> H {
    let paths = paths_in(t);
    let mut config = HubConfig::new(&FIELDS);
    for l in libraries {
        config.add_library(l);
    }
    if !libraries.is_empty() {
        config.save(&paths.config_file(), &FIELDS).unwrap();
    }
    testing::harness(spec(t, HubLayout::DEFAULT), Fake::new(paths), size)
}

fn window(t: &Path, libraries: &[PathBuf]) -> H {
    window_sized(t, libraries, SIZE_720)
}

/// A window over a library holding `found` games, once its scan is done.
fn hub_with(name: &str, found: &[usize]) -> (PathBuf, H) {
    let t = tmp(name);
    let lib = t.join("games");
    std::fs::create_dir_all(&lib).unwrap();
    for i in found {
        std::fs::create_dir_all(lib.join(GAMES[*i].slug)).unwrap();
    }
    let mut h = window(&t, &[lib]);
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    (t, h)
}

fn hub(name: &str) -> (PathBuf, H) {
    hub_with(name, &[])
}

fn tile<'h>(h: &'h H, i: usize) -> crate::testing::Node<'h> {
    h.get_by_role_and_label(Role::RadioButton, GAMES[i].title)
}

fn select(h: &mut H, i: usize) {
    tile(h, i).click();
    h.run_steps(3);
    assert_eq!(h.state().model().selected, i);
}

fn click_button(h: &mut H, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run_steps(3);
}

fn open_tab(h: &mut H, tab: Tab) {
    h.get_by_role_and_label(Role::Tab, tab.label()).click();
    h.run_steps(3);
}

fn press(h: &mut H, key: egui::Key) {
    h.key_press(key);
    h.run_steps(2);
}

fn log_of(t: &Path) -> String {
    std::fs::read_to_string(t.join("data/launcher.log")).unwrap_or_default()
}

fn settings_text(t: &Path, i: usize) -> String {
    std::fs::read_to_string(paths_in(t).settings(GAMES[i].slug)).unwrap_or_default()
}

/// Runs frames until some label or value contains `words`.
#[track_caller]
fn wait_says(h: &mut H, words: &str) {
    for _ in 0..300 {
        if says(h, words) {
            return;
        }
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("never said {words:?}");
}

/// A session the test drives: it reports what arrives on the channel and
/// ends with what the last message says.
enum Step {
    Stage(&'static str, &'static str),
    Playing,
    End(Result<(), String>),
}

fn scripted(h: &mut H) -> (mpsc::Sender<Step>, mpsc::Receiver<bool>) {
    let (tx, rx) = mpsc::channel::<Step>();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let session: Session = Box::new(move |r: &Reporter| {
        for step in rx {
            match step {
                Step::Stage(a, b) => r.setting_up(a, b),
                Step::Playing => r.playing(),
                Step::End(e) => {
                    let _ = stopped_tx.send(r.stop_requested());
                    return e;
                }
            }
        }
        Ok(())
    });
    h.state_mut().backend_mut().next = Some(session);
    (tx, stopped_rx)
}

// ------------------------------------------------------------ hub

#[test]
fn the_hub_has_a_tile_for_each_game() {
    let (_t, h) = hub("tiles");
    assert_eq!(h.get_all_by_role(Role::RadioButton).count(), GAMES.len());
    for g in GAMES {
        let node = h.get_by_role_and_label(Role::RadioButton, g.title);
        let about = crate::testing::description(&node).unwrap();
        assert_eq!(
            about,
            format!("{}, {}, Not in your library", g.year, g.board)
        );
    }
    assert!(says(&h, "Your games · 0 of 5 found"));
    assert!(crate::testing::checked(&tile(&h, 0)));
    assert!(!crate::testing::checked(&tile(&h, 1)));
    assert!(says(&h, "Controller: No controller"));
}

#[test]
fn selecting_a_tile_shows_that_games_page() {
    let (t, mut h) = hub_with("select", &[1]);
    select(&mut h, 1);
    assert!(
        h.query_by_role_and_label(Role::Label, GAMES[1].title)
            .is_some()
    );
    assert!(says(&h, "Lindbergh · 2006 · Export"));
    assert!(says(&h, "Ready"));
    let dir = t.join("games").join("ar2");
    assert!(says(&h, &format!("Folder: {}", dir.display())));
    let play = h.get_by_role_and_label(Role::Button, "Play");
    assert!(!crate::testing::disabled(&play));
    assert!(says(&h, "Your games · 1 of 5 found"));
}

#[test]
fn a_blocked_game_cannot_be_played_and_says_why() {
    let (_t, mut h) = hub_with("blocked", &[1]);
    let why = "Not yet: it crashes.";
    h.state_mut().backend_mut().supports[1] = Support::Blocked(why);
    select(&mut h, 1);
    let play = h.get_by_role_and_label(Role::Button, "Play");
    assert!(crate::testing::disabled(&play));
    assert_eq!(crate::testing::description(&play).as_deref(), Some(why));
    assert!(says(&h, why), "the reason is on the page");
    assert!(says(&h, "Not supported yet"));
    click_button(&mut h, "Play");
    press(&mut h, egui::Key::Enter);
    assert!(!h.state().is_running() && h.state().message().is_none());
}

#[test]
fn an_unverified_game_shows_its_note_and_can_be_played() {
    let (_t, mut h) = hub_with("unverified", &[2]);
    let note = "Boots; no race verified.";
    h.state_mut().backend_mut().supports[2] = Support::Unverified(note);
    select(&mut h, 2);
    assert!(says(&h, note));
    assert!(says(&h, "Starts, not verified"));
    assert!(!crate::testing::disabled(
        &h.get_by_role_and_label(Role::Button, "Play")
    ));
    assert_eq!(
        crate::testing::description(&tile(&h, 2)).unwrap(),
        "2007, Lindbergh, Starts, not verified"
    );
}

#[test]
fn a_game_missing_from_the_library_cannot_be_played() {
    let (_t, mut h) = hub("missing");
    select(&mut h, 1);
    let play = h.get_by_role_and_label(Role::Button, "Play");
    assert!(crate::testing::disabled(&play));
    assert!(says(&h, "Not in your library"));
    assert!(says(&h, "Add its folder in Settings"));
    assert!(
        crate::testing::description(&play)
            .unwrap()
            .contains("Not found")
    );
}

#[test]
fn a_game_that_wont_start_says_why_and_the_log_has_it() {
    let (t, mut h) = hub_with("refused", &[1]);
    select(&mut h, 1);
    click_button(&mut h, "Play");
    assert!(!h.state().is_running(), "the window is free again");
    assert!(says(&h, "Arcade Racer 2 couldn't start"));
    assert!(says(
        &h,
        "The game needs launcher components that are missing."
    ));
    let log = paths_in(&t).log();
    assert!(says(&h, &format!("Details are in {}.", display_path(&log))));
    assert!(
        log_of(&t).contains("Arcade Racer 2: the game needs launcher components that are missing"),
        "{}",
        log_of(&t)
    );
    click_button(&mut h, "OK");
    assert!(h.state().message().is_none());
    assert!(h.query_by_label_contains("couldn't start").is_none());
}

#[test]
fn a_running_game_shows_its_stages_and_stop_asks_the_session_to_end() {
    let (t, mut h) = hub_with("running", &[1]);
    select(&mut h, 1);
    let (tx, stopped) = scripted(&mut h);
    click_button(&mut h, "Play");
    assert!(h.state().is_running());
    assert!(says(&h, "Getting Arcade Racer 2 ready…"));
    assert!(h.query_by_role_and_label(Role::Button, "Stop").is_none());
    tx.send(Step::Stage(
        "Building the loader…",
        "This takes a few minutes.",
    ))
    .unwrap();
    wait_says(&mut h, "Building the loader…");
    assert!(says(&h, "This takes a few minutes."));
    assert!(h.query_by_role_and_label(Role::Button, "Stop").is_none());
    tx.send(Step::Playing).unwrap();
    wait_says(&mut h, "Arcade Racer 2 is running");
    // nothing on the hub reacts while the game runs
    press(&mut h, egui::Key::ArrowRight);
    assert_eq!(h.state().model().selected, 1);
    click_button(&mut h, "Stop");
    assert!(crate::testing::disabled(
        &h.get_by_role_and_label(Role::Button, "Stop")
    ));
    tx.send(Step::End(Ok(()))).unwrap();
    assert!(stopped.recv().unwrap(), "the session saw the stop");
    testing::wait(&mut h, "the end", |a| !a.is_running());
    assert!(h.query_by_label_contains("is running").is_none());
    assert!(h.state().message().is_none());
    assert_eq!(h.state().backend().ended, 1);
    assert!(log_of(&t).is_empty(), "a clean end logs nothing");
}

#[test]
fn a_game_that_stops_with_an_error_points_at_its_log() {
    let (t, mut h) = hub_with("crashed", &[1]);
    select(&mut h, 1);
    let (tx, _stopped) = scripted(&mut h);
    click_button(&mut h, "Play");
    tx.send(Step::Playing).unwrap();
    tx.send(Step::End(Err(
        "the game ended at once (exit code 0xc0000135)".into(),
    )))
    .unwrap();
    testing::wait(&mut h, "the end", |a| !a.is_running());
    assert!(says(&h, "Arcade Racer 2 stopped with an error"));
    let game_log = paths_in(&t).runtime("ar2").join("session.log");
    assert!(says(
        &h,
        &format!(
            "The game ended at once (exit code 0xc0000135).\n\nTry again. If it happens again, the game's log is {}.",
            display_path(&game_log)
        )
    ));
    assert!(log_of(&t).contains("Arcade Racer 2: the game ended at once"));
}

#[test]
fn closing_the_window_stops_the_game_first_then_closes() {
    let (_t, mut h) = hub_with("close", &[1]);
    select(&mut h, 1);
    let (tx, stopped) = scripted(&mut h);
    click_button(&mut h, "Play");
    tx.send(Step::Playing).unwrap();
    wait_says(&mut h, "is running");
    testing::request_close(&mut h);
    h.run_steps(1);
    assert!(
        testing::commands(&h).contains(&ViewportCommand::CancelClose),
        "the window stays while the game runs"
    );
    tx.send(Step::End(Ok(()))).unwrap();
    assert!(stopped.recv().unwrap(), "the game was asked to stop");
    let mut closed = false;
    for _ in 0..100 {
        h.run_steps(1);
        if testing::commands(&h).contains(&ViewportCommand::Close) {
            closed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(closed, "then it closes");
}

#[test]
fn the_keyboard_moves_between_tiles_and_pages() {
    let (_t, mut h) = hub("keys");
    press(&mut h, egui::Key::ArrowRight);
    press(&mut h, egui::Key::ArrowRight);
    assert_eq!(h.state().model().selected, 2);
    assert!(crate::testing::checked(&tile(&h, 2)));
    press(&mut h, egui::Key::PageDown);
    assert_eq!(h.state().model().page, Page::Controls);
    press(&mut h, egui::Key::PageDown);
    assert_eq!(h.state().model().page, Page::Settings);
    assert!(says(&h, "Arcade Racer 3 Extreme Edition settings"));
    press(&mut h, egui::Key::Escape);
    assert_eq!(h.state().model().page, Page::Hub);
}

#[test]
fn the_pad_moves_the_hub_and_y_opens_settings() {
    let (_t, mut h) = hub("pad");
    h.state_mut().backend_mut().pads = vec![Input::Right, Input::Right, Input::Left];
    h.run_steps(2);
    assert_eq!(h.state().model().selected, 1);
    h.state_mut().backend_mut().pads = vec![Input::Settings];
    h.run_steps(3);
    assert_eq!(h.state().model().page, Page::Settings);
    assert!(says(&h, "Arcade Racer 2 settings"));
    h.state_mut().backend_mut().pads = vec![Input::Start];
    h.run_steps(2);
    assert_eq!(h.state().model().page, Page::Hub);
}

#[test]
fn the_pad_is_paused_while_a_game_runs() {
    let (_t, mut h) = hub_with("pad-paused", &[0]);
    let (tx, _stopped) = scripted(&mut h);
    click_button(&mut h, "Play");
    tx.send(Step::Playing).unwrap();
    h.state_mut().backend_mut().pads = vec![Input::Right, Input::Escape];
    h.run_steps(3);
    assert_eq!(h.state().model().selected, 0);
    assert!(
        h.state().backend().pads.is_empty(),
        "drained, not kept for later"
    );
    tx.send(Step::End(Ok(()))).unwrap();
    testing::wait(&mut h, "the end", |a| !a.is_running());
    assert_eq!(
        h.state().model().selected,
        0,
        "nothing queued moves it afterwards"
    );
    assert!(!testing::commands(&h).contains(&ViewportCommand::Close));
}

// ------------------------------------------------------------ settings

#[test]
fn every_option_in_the_settings_window_reaches_the_file() {
    let (t, mut h) = hub("settings");
    let file = paths_in(&t).settings("ar1");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "# my notes\nfuture_key = 7\ncrt = on\n").unwrap();
    open_tab(&mut h, Tab::Settings);
    assert!(says(&h, "Arcade Racer settings"));
    for spec in OPTIONS.iter().filter(|s| !s.launcher_only) {
        assert!(
            h.query_by_role_and_label(Role::Button, spec.label)
                .is_some(),
            "{}",
            spec.key
        );
    }
    assert!(h.query_by_label_contains("CRT").is_none(), "launcher-only");
    for (label, way, key, value) in [
        ("Game render size", "next", "render", "1080p"),
        ("Upscaler", "next", "upscaler", "nis"),
        ("Sharpness", "next", "sharpness", "6"),
        ("Fullscreen", "next", "fullscreen", "off"),
        ("Dead zone", "previous", "deadzone", "4"),
    ] {
        if cfg!(windows) && key == "fullscreen" {
            // the Linux helper's row: off on Windows (tested below)
            continue;
        }
        h.get_by_role_and_label(Role::Button, &format!("{label}: {way}"))
            .click();
        h.run_steps(3);
        let text = settings_text(&t, 0);
        assert!(text.contains(&format!("{key} = {value}")), "{key}:\n{text}");
        assert!(
            text.starts_with("# my notes\nfuture_key = 7\ncrt = on\n"),
            "notes and unknown keys stay:\n{text}"
        );
        assert!(says(
            &h,
            "Saved. The game uses it the next time you press Play."
        ));
    }
    let shown = |h: &H, label: &str| {
        crate::testing::value(&h.get_by_role_and_label(Role::Button, label)).unwrap()
    };
    assert_eq!(shown(&h, "Game render size"), "1920 × 1080 (1080p)");
    assert_eq!(shown(&h, "Upscaler"), "NVIDIA Image Scaling");
    assert_eq!(shown(&h, "Sharpness"), "6");
    let fullscreen = if cfg!(windows) { "On" } else { "Off" };
    assert_eq!(shown(&h, "Fullscreen"), fullscreen);
    assert_eq!(shown(&h, "Dead zone"), "4%");
}

#[test]
fn the_pad_and_keyboard_change_the_focused_row() {
    let (t, mut h) = hub("settings-keys");
    open_tab(&mut h, Tab::Settings);
    press(&mut h, egui::Key::ArrowRight);
    assert!(settings_text(&t, 0).contains("render = 1080p"));
    press(&mut h, egui::Key::Enter);
    assert!(
        settings_text(&t, 0).contains("render = native"),
        "Enter wraps"
    );
    h.state_mut().backend_mut().pads = vec![Input::Down, Input::Right];
    h.run_steps(3);
    assert!(settings_text(&t, 0).contains("upscaler = nis"));
}

#[test]
fn a_row_that_cant_change_says_why_and_ignores_input() {
    let (t, mut h) = hub("row-off");
    open_tab(&mut h, Tab::Settings);
    for _ in 0..2 {
        h.get_by_role_and_label(Role::Button, "Upscaler").click();
        h.run_steps(3);
    }
    assert!(settings_text(&t, 0).contains("upscaler = linear"));
    let sharp = h.get_by_role_and_label(Role::Button, "Sharpness");
    assert!(crate::testing::disabled(&sharp));
    assert!(
        crate::testing::description(&sharp)
            .unwrap()
            .contains("FSR 1")
    );
    assert!(h.query_by_label_contains("Sharpness: next").is_none());
    // its whole reason is on the page once the row has the focus
    assert!(
        h.query_by_role_and_label(Role::Label, SHARPNESS_OFF)
            .is_none()
    );
    h.state_mut().model.focus = Focus::Item(2);
    h.run_steps(2);
    assert!(
        h.query_by_role_and_label(Role::Label, SHARPNESS_OFF)
            .is_some()
    );
    h.state_mut().backend_mut().pads = vec![Input::Down, Input::Down, Input::Right];
    h.run_steps(3);
    assert!(settings_text(&t, 0).contains("sharpness = 5"), "unchanged");
    h.get_by_role_and_label(Role::Button, "Upscaler").click();
    h.run_steps(3);
    assert!(!crate::testing::disabled(
        &h.get_by_role_and_label(Role::Button, "Sharpness")
    ));
}

#[test]
fn rows_the_linux_helper_owns_are_shown_on_every_system() {
    let (t, mut h) = hub("helper-rows");
    open_tab(&mut h, Tab::Settings);
    let row = h.get_by_role_and_label(Role::Button, "Fullscreen");
    assert_eq!(crate::testing::disabled(&row), cfg!(windows));
    if cfg!(windows) {
        assert_eq!(
            crate::testing::description(&row).as_deref(),
            Some(HubWords::DEFAULT.not_here)
        );
        h.get_by_role_and_label(Role::Button, "Fullscreen").click();
        h.run_steps(3);
        assert!(!paths_in(&t).settings("ar1").exists(), "nothing changed");
    }
}

#[test]
fn settings_belong_to_the_selected_game() {
    let (t, mut h) = hub("per-game");
    select(&mut h, 3);
    open_tab(&mut h, Tab::Settings);
    assert!(says(&h, "Arcade Racer 4 settings"));
    h.get_by_role_and_label(Role::Button, "Dead zone: next")
        .click();
    h.run_steps(3);
    assert!(settings_text(&t, 3).contains("deadzone = 6"));
    assert!(!paths_in(&t).settings("ar1").exists());
}

#[test]
fn a_settings_file_that_cant_be_saved_says_so_and_the_log_has_it() {
    let (t, mut h) = hub("save-fails");
    // a folder where the settings file goes
    std::fs::create_dir_all(paths_in(&t).settings("ar1")).unwrap();
    open_tab(&mut h, Tab::Settings);
    h.get_by_role_and_label(Role::Button, "Dead zone: next")
        .click();
    h.run_steps(3);
    assert!(says(&h, "Couldn't save to"));
    assert!(says(&h, "Check that the folder can be written."));
    assert!(log_of(&t).contains("Couldn't save to"), "{}", log_of(&t));
}

#[test]
fn rescan_looks_again_and_finds_a_new_game() {
    let (t, mut h) = hub("rescan");
    open_tab(&mut h, Tab::Settings);
    assert!(says(&h, "0 of 5 games found."));
    std::fs::create_dir_all(t.join("games/ar3")).unwrap();
    std::fs::create_dir_all(t.join("games/mystery")).unwrap();
    click_button(&mut h, "Rescan");
    testing::wait(&mut h, "the rescan", |a| a.library().is_some());
    assert!(says(&h, "1 of 5 games found. 1 folder holds a build"));
    assert!(says(&h, &t.join("games").display().to_string()));
    assert!(log_of(&t).contains("unknown build game.exe 00ff"));
}

#[test]
fn a_rescan_during_a_scan_runs_after_it() {
    let (_t, mut h) = hub("rescan-twice");
    open_tab(&mut h, Tab::Settings);
    click_button(&mut h, "Rescan");
    assert!(h.state().scanning());
    // the button is off while looking, but the pad can still press it
    let n = h.state().model().items.len();
    h.state_mut().model.focus = Focus::Item(n - 2);
    h.state_mut().backend_mut().pads = vec![Input::Press];
    h.run_steps(1);
    testing::wait(&mut h, "both scans", |a| {
        !a.scanning() && a.library().is_some()
    });
    // three scans, one after the other: the first, the button's, the pad's
    let scans = h
        .state()
        .backend()
        .scans
        .load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(scans, 3);
}

// ------------------------------------------------------------ setup

#[test]
fn the_first_run_adds_a_library_folder_and_opens_the_hub() {
    let t = tmp("first-run");
    let mut h = window(&t, &[]);
    assert_eq!(h.state().model().page, Page::Setup);
    assert!(says(&h, "Choose your game folders"));
    assert!(says(&h, "No folder yet."));
    assert!(h.query_by_role_and_label(Role::Tab, "Games").is_none());
    let done = h.get_by_role_and_label(Role::Button, "Done");
    assert!(crate::testing::disabled(&done));
    assert!(
        crate::testing::description(&done)
            .unwrap()
            .contains("Add a game folder")
    );

    click_button(&mut h, "Add folder");
    assert!(says(&h, "Type the folder's path first."));
    let field = h.get_by_role_and_label(Role::TextInput, "Game folder");
    field.click();
    field.type_text("/definitely/not/here");
    h.run_steps(3);
    click_button(&mut h, "Add folder");
    assert!(says(&h, "isn't a folder this computer can open"));
    assert!(!paths_in(&t).config_file().exists());

    let lib = t.join("my games");
    std::fs::create_dir_all(&lib).unwrap();
    h.state_mut().typed[0].clear();
    let field = h.get_by_role_and_label(Role::TextInput, "Game folder");
    field.click();
    field.type_text(&lib.display().to_string());
    h.run_steps(3);
    assert_eq!(
        crate::testing::value(&h.get_by_role_and_label(Role::TextInput, "Game folder")).unwrap(),
        lib.display().to_string()
    );
    click_button(&mut h, "Add folder");
    let config = HubConfig::load(&paths_in(&t).config_file(), &FIELDS).unwrap();
    assert_eq!(config.libraries, std::slice::from_ref(&lib));
    assert!(says(&h, &format!("Added {}.", lib.display())));
    assert!(log_of(&t).contains("library added"));
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    click_button(&mut h, "Done");
    assert_eq!(h.state().model().page, Page::Hub);
    assert!(says(&h, "Your games · 0 of 5 found"));
}

#[test]
fn enter_adds_the_typed_folder_and_a_second_folder_is_kept() {
    let t = tmp("two-folders");
    let first = t.join("a");
    let second = t.join("b");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let mut h = window(&t, std::slice::from_ref(&first));
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    open_tab(&mut h, Tab::Settings);
    click_button(&mut h, "Folders and BIOS");
    assert_eq!(h.state().model().page, Page::Setup);
    assert!(says(&h, "Your game folders"));
    for _ in 0..2 {
        h.get_by_role_and_label(Role::TextInput, "Game folder")
            .type_text(&second.display().to_string());
        h.run_steps(2);
        press(&mut h, egui::Key::Enter);
    }
    let config = HubConfig::load(&paths_in(&t).config_file(), &FIELDS).unwrap();
    assert_eq!(config.libraries, [first.clone(), second.clone()], "once");
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    press(&mut h, egui::Key::Escape);
    assert_eq!(h.state().model().page, Page::Settings);
    assert!(says(&h, &first.display().to_string()));
    assert!(says(&h, &second.display().to_string()));
}

#[test]
fn the_setup_page_takes_a_path_field_of_the_right_kind() {
    let (t, mut h) = hub("bios");
    open_tab(&mut h, Tab::Settings);
    click_button(&mut h, "Folders and BIOS");
    assert!(says(&h, BIOS.heading));
    click_button(&mut h, "Use BIOS");
    assert!(says(&h, "Type the path of naomi2.zip first."));
    h.state_mut().typed[1] = t.display().to_string();
    click_button(&mut h, "Use BIOS");
    assert!(
        says(&h, "isn't a file this computer can open"),
        "a folder is no file"
    );
    let bios = t.join("naomi2.zip");
    std::fs::write(&bios, b"zip").unwrap();
    h.state_mut().typed[1].clear();
    let field = h.get_by_role_and_label(Role::TextInput, "NAOMI 2 BIOS file");
    field.click();
    field.type_text(&bios.display().to_string());
    h.run_steps(3);
    click_button(&mut h, "Use BIOS");
    assert!(says(&h, "NAOMI 2 BIOS saved."));
    let config = HubConfig::load(&paths_in(&t).config_file(), &FIELDS).unwrap();
    assert_eq!(config.paths[0], Some(bios.clone()));
    assert_eq!(config.libraries.len(), 1, "the folders are kept next to it");
    // shown when the page opens again
    press(&mut h, egui::Key::Escape);
    click_button(&mut h, "Folders and BIOS");
    assert!(says(&h, &format!("In use: {}", bios.display())));
}

#[test]
fn a_playable_game_shows_the_launchers_note() {
    let (t, mut h) = hub_with("note", &[0, 1]);
    assert!(says(&h, "Needs the NAOMI 2 BIOS: add it in Settings."));
    select(&mut h, 1);
    assert!(
        h.query_by_label_contains("Needs the NAOMI 2 BIOS")
            .is_none()
    );
    select(&mut h, 0);
    h.state_mut().backend_mut().supports[0] = Support::Blocked("no");
    h.run_steps(2);
    assert!(
        h.query_by_label_contains("Needs the NAOMI 2 BIOS")
            .is_none(),
        "only under a playable game"
    );
    h.state_mut().backend_mut().supports[0] = Support::Ready;
    let bios = t.join("naomi2.zip");
    std::fs::write(&bios, b"zip").unwrap();
    h.state_mut().typed[1] = bios.display().to_string();
    h.state_mut().model.open(Page::Setup);
    h.state_mut().model.focus = Focus::Submit(1);
    press(&mut h, egui::Key::Enter);
    press(&mut h, egui::Key::Escape);
    press(&mut h, egui::Key::Escape);
    assert_eq!(h.state().model().page, Page::Hub);
    assert!(
        h.query_by_label_contains("Needs the NAOMI 2 BIOS")
            .is_none()
    );
}

#[test]
fn an_unreadable_launcher_cfg_is_explained_and_never_overwritten() {
    let t = tmp("cfg-unreadable");
    let file = paths_in(&t).config_file();
    std::fs::create_dir_all(&file).unwrap();
    let mut h = window(&t, &[]);
    assert!(says(&h, "Your launcher settings can't be read"));
    assert!(log_of(&t).contains(&file.display().to_string()));
    click_button(&mut h, "OK");
    let lib = t.join("games");
    std::fs::create_dir_all(&lib).unwrap();
    h.state_mut().typed[0] = lib.display().to_string();
    click_button(&mut h, "Add folder");
    assert!(says(&h, "Couldn't save the choice"));
    assert!(file.is_dir(), "left alone");
    assert!(
        h.state().config().libraries.is_empty(),
        "not kept half-saved"
    );
}

// ------------------------------------------------------------ extra page

#[test]
fn a_game_that_needs_files_opens_the_extra_page_and_comes_back_playable() {
    let (t, mut h) = hub_with("extra", &[FILES_GAME]);
    select(&mut h, FILES_GAME);
    assert!(says(&h, "Needs board files"));
    assert!(says(&h, "Also needs the board's files."));
    assert_eq!(
        crate::testing::description(&tile(&h, FILES_GAME)).unwrap(),
        "2012, N2, Needs board files"
    );
    click_button(&mut h, "Add files");
    assert_eq!(h.state().model().page, Page::Extra);
    assert!(says(&h, "Add the board files"));
    assert!(says(&h, "ic5.bin · 2 MB: Missing"));
    assert_eq!(
        crate::testing::value(&h.get_by_role_and_label(Role::TextInput, "Folder with the files"))
            .unwrap(),
        "~/Downloads"
    );
    let reads = h.state().backend().extra_reads.get();
    h.run_steps(10);
    assert_eq!(
        h.state().backend().extra_reads.get(),
        reads,
        "not read every frame"
    );
    click_button(&mut h, "Look for the files");
    assert!(says(&h, "None of the missing files were found"));
    let files = t.join("board");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("ic5.bin"), b"x").unwrap();
    h.state_mut().extra_typed = files.display().to_string();
    press(&mut h, egui::Key::Enter);
    assert!(says(&h, "All 1 files are in."));
    assert!(says(&h, "ic5.bin · 2 MB: In"));
    assert!(log_of(&t).contains("Arcade Racer 5: All 1 files are in."));
    click_button(&mut h, "Back");
    assert_eq!(
        (h.state().model().page, h.state().model().focus),
        (Page::Hub, Focus::Play)
    );
    assert!(!crate::testing::disabled(
        &h.get_by_role_and_label(Role::Button, "Play")
    ));
}

// ------------------------------------------------------------ controls

#[test]
fn the_controls_page_shows_the_devices_and_live_readings() {
    let (_t, mut h) = hub("controls");
    open_tab(&mut h, Tab::Controls);
    assert!(says(&h, "No controller found."));
    h.state_mut().backend_mut().controls = ControlsView {
        devices: vec![
            Device {
                name: "Test Pad".into(),
                kind: rea_launcher::wheel::Kind::Pad,
            },
            Device {
                name: "Test Wheel".into(),
                kind: rea_launcher::wheel::Kind::Wheel,
            },
        ],
        driver: Some(1),
        layout: vec![("Gas".into(), "RT".into()), ("Brake".into(), "LT".into())],
        layout_note: "A wheel steers with its wheel and pedals.".into(),
        steer: -0.5,
        accel: 0.25,
        brake: 0.75,
        buttons: vec![
            ButtonView {
                key: "start",
                label: "Start".into(),
                pressed: true,
            },
            ButtonView {
                key: "coin",
                label: "Insert coin".into(),
                pressed: false,
            },
        ],
        feeding: true,
        error: None,
        lines: vec![("Gear".into(), "3".into())],
    };
    h.run_steps(3);
    assert!(says(&h, "A game is running: it reads these controls."));
    // a game's own reading (Wangan's gear), with its accessible name
    assert!(h.query_by_label("Gear: 3").is_some());
    assert!(says(&h, "Wheel: Test Wheel (drives the car)"));
    assert!(says(&h, "Pad: Test Pad"));
    assert!(h.query_by_label_contains("Test Pad (drives").is_none());
    assert!(says(&h, "Controller: Test Wheel"));
    let level = |h: &H, name: &str| {
        let m = h.get_by_role_and_label(Role::Meter, name);
        (
            m.accesskit_node().numeric_value().unwrap(),
            crate::testing::value(&m).unwrap(),
        )
    };
    assert_eq!(level(&h, "Steering"), (-50.0, "-50%".to_string()));
    assert_eq!(level(&h, "Gas"), (25.0, "25%".to_string()));
    assert_eq!(level(&h, "Brake"), (75.0, "75%".to_string()));
    assert!(says(&h, "Start: pressed"));
    assert!(says(&h, "Insert coin: released"));
    assert!(says(&h, "Gas: RT"));
}

#[test]
fn a_controller_error_is_explained() {
    let (_t, mut h) = hub("controls-error");
    h.state_mut().backend_mut().controls.error = Some("no SDL".into());
    open_tab(&mut h, Tab::Controls);
    assert!(says(&h, "Controllers can't be read: no SDL"));
    assert!(says(&h, "Controller: Controllers unavailable"));
}

// ------------------------------------------------------------ layout

/// Every page and overlay at `size`, in both row layouts.
fn every_control_fits(size: egui::Vec2, layout: HubLayout) {
    let t = tmp(&format!("fits-{}-{}", size.y, layout.row_height));
    let lib = t.join("games");
    std::fs::create_dir_all(lib.join("ar3")).unwrap();
    std::fs::create_dir_all(lib.join(GAMES[FILES_GAME].slug)).unwrap();
    let mut config = HubConfig::new(&FIELDS);
    config.add_library(&lib);
    config.save(&paths_in(&t).config_file(), &FIELDS).unwrap();
    let mut h = testing::harness(spec(&t, layout), Fake::new(paths_in(&t)), size);
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    let check = |h: &mut H, page: &str| {
        h.run_steps(3);
        let misfits = testing::misfits(h);
        assert!(misfits.is_empty(), "{page} at {size:?}: {misfits:#?}");
    };
    select(&mut h, 2);
    check(&mut h, "hub, a long title");
    open_tab(&mut h, Tab::Controls);
    check(&mut h, "controls");
    h.state_mut().backend_mut().controls = ControlsView {
        buttons: ["Start", "Coin", "View", "Shift up", "Shift down", "Test"]
            .map(|label| ButtonView {
                key: label,
                label: label.into(),
                pressed: false,
            })
            .into(),
        feeding: true,
        lines: vec![("Gear".into(), "3".into())],
        ..ControlsView::default()
    };
    check(&mut h, "controls with a game's reading");
    open_tab(&mut h, Tab::Settings);
    check(&mut h, "settings");
    click_button(&mut h, "Folders and BIOS");
    h.state_mut().notes[0] = Some(Note::error("x ".repeat(200)));
    check(&mut h, "setup");
    press(&mut h, egui::Key::Escape);
    press(&mut h, egui::Key::Escape);
    select(&mut h, FILES_GAME);
    click_button(&mut h, "Add files");
    check(&mut h, "extra page");
    press(&mut h, egui::Key::Escape);
    let (tx, _s) = scripted(&mut h);
    h.state_mut().backend_mut().files_in = true;
    h.run_steps(2);
    click_button(&mut h, "Play");
    tx.send(Step::Playing).unwrap();
    check(&mut h, "running");
    tx.send(Step::End(Err("it broke ".repeat(40)))).unwrap();
    testing::wait(&mut h, "the end", |a| a.message().is_some());
    check(&mut h, "message");
}

#[test]
fn every_control_fits_the_window_at_720_lines() {
    every_control_fits(SIZE_720, HubLayout::DEFAULT);
    every_control_fits(SIZE_720, HubLayout::COMPACT_ROWS);
}

#[test]
fn every_control_fits_the_window_at_16_by_10() {
    every_control_fits(SIZE_800, HubLayout::DEFAULT);
    every_control_fits(SIZE_800, HubLayout::COMPACT_ROWS);
}

#[test]
fn the_dump_lists_the_window_as_accesskit_sees_it() {
    let (_t, mut h) = hub_with("dump", &[1]);
    let dump = testing::dump(&h);
    assert!(
        dump.contains("RadioButton \"Arcade Racer\" (2001, NAOMI 2, Not in your library) [on] @"),
        "{dump}"
    );
    assert!(
        dump.contains(
            "Button \"Play\" (Not found in your game folders.) [disabled] @56,472 170x50"
        )
    );
    assert!(dump.contains("Tab \"Games\" [on]"));
    assert!(dump.contains("Label = \"Your games · 1 of 5 found\""));
    h.run_steps(3);
    assert_eq!(testing::dump(&h), dump, "stable between frames");
}

#[test]
fn the_window_is_not_named_like_a_game() {
    assert!(GAMES.iter().all(|g| g.title != THEME.title));
}

#[test]
fn a_window_too_small_for_the_layout_has_misfits() {
    let t = tmp("too-small");
    let lib = t.join("games");
    std::fs::create_dir_all(&lib).unwrap();
    // 360 lines: the zoom stops at 0.75, so the tile row falls off
    let mut h = window_sized(&t, &[lib], egui::vec2(640.0, 360.0));
    testing::wait(&mut h, "the scan", |a| a.library().is_some());
    let misfits = testing::misfits(&h);
    assert!(
        misfits
            .iter()
            .any(|m| m.starts_with("RadioButton \"Arcade Racer\"")),
        "{misfits:#?}"
    );
}

#[test]
fn a_message_before_the_window_reaches_the_log() {
    let t = tmp("fatal");
    let log = rea_launcher::log::Log::new(t.join("launcher.log"));
    let mode = crate::report::Mode::Command { interactive: false };
    let who = super::fatal(
        &THEME,
        &log,
        "Arcade Racer 9 is not a game here.",
        None,
        mode,
    );
    assert_ne!(
        who,
        crate::report::NativeAudience::Window,
        "a script sees no window"
    );
    assert_eq!(
        std::fs::read_to_string(log.path()).unwrap(),
        "error: Arcade Racer 9 is not a game here.\n"
    );
}

#[test]
fn a_click_on_another_tile_only_selects_it() {
    let (_t, mut h) = hub_with("double", &[0, 1]);
    select(&mut h, 0);
    tile(&h, 1).click();
    h.run_steps(2);
    assert_eq!(h.state().model().selected, 1);
    assert!(h.state().message().is_none(), "nothing started");
}
