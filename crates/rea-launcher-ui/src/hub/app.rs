//! The hub window's state and what it does with each input: scans and art
//! on their own threads, the settings form, the setup and extra pages, the
//! game's session on its own thread, messages. [`super::pages`] draws it.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui::{self, TextureHandle};
use rea_launcher::cfg::OptionSpec;
use rea_launcher::hub::{
    Effect, Focus, HubConfig, Input, Model, Page, TileState, World, sentence, typed_path,
};
use rea_launcher::log::Log;

use super::backend::{
    ControlsView, ExtraPage, HubBackend, HubForm, Note, Progress, Reporter, Stage,
};
use super::paint::edit_text;
use super::{HubSpec, theme};

/// A game being set up or played.
pub(crate) struct Running {
    pub game: usize,
    pub stage: Option<Stage>,
    pub playing: bool,
    pub progress: Receiver<Progress>,
    /// Asks the session to stop the game.
    pub stop: Arc<AtomicBool>,
}

/// A message over the window, dismissed with OK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub body: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArtKind {
    Hero,
    Tile,
}

type Art = (usize, ArtKind, egui::ColorImage);

/// The hub window over a launcher's [`HubBackend`] (what [`super::run`]
/// runs, and what the headless tests drive through [`HubApp::frame`]).
pub struct HubApp<B: HubBackend> {
    pub(crate) spec: HubSpec,
    pub(crate) backend: B,
    pub(crate) config: HubConfig,
    pub(crate) log: Log,
    pub(crate) model: Model,
    /// The settings rows, from the backend's option table.
    pub(crate) rows: Vec<&'static OptionSpec>,
    /// The last scan; `None` while looking.
    pub(crate) library: Option<B::Library>,
    scan: Option<Receiver<(B::Library, B::Cache)>>,
    pub(crate) cache: Option<B::Cache>,
    /// A rescan asked for during a scan: started when it ends, with its cache.
    rescan: bool,
    pub(crate) form: Option<(usize, B::Form)>,
    /// The last settings save.
    pub(crate) note: Option<Note>,
    pub(crate) heroes: Vec<Option<TextureHandle>>,
    pub(crate) tiles: Vec<Option<TextureHandle>>,
    art: Receiver<Art>,
    /// The hero picture being faded from, and when the fade started.
    pub(crate) fade: Option<(usize, f64)>,
    pub(crate) running: Option<Running>,
    pub(crate) message: Option<Message>,
    /// Setup page: what is typed in each field (0: the library folder).
    pub(crate) typed: Vec<String>,
    /// Setup page: the line under each field.
    pub(crate) notes: Vec<Option<Note>>,
    pub(crate) extra: Option<ExtraPage>,
    pub(crate) extra_typed: String,
    pub(crate) extra_note: Option<Note>,
    /// The monitor's size in pixels, once known (for the looks).
    pub(crate) screen: Option<(u32, u32)>,
    /// The window was closed while a game ran: close once it has stopped.
    close_after_stop: bool,
    pub(crate) controls: ControlsView,
}

/// A mouse click, turned into the same model inputs the keyboard sends.
pub(crate) enum Click {
    Focus(Focus, Option<Input>),
    Tile(usize, bool),
    Dismiss,
    StopGame,
}

impl<B: HubBackend> HubApp<B> {
    /// The window over `backend`, reading `launcher.cfg` and starting the
    /// first scan and the art.
    pub fn new(spec: HubSpec, backend: B) -> HubApp<B> {
        let log = Log::new(spec.paths.log());
        let file = spec.paths.config_file();
        let (config, message) = match HubConfig::load(&file, spec.fields) {
            Ok(c) => (c, None),
            Err(e) => {
                let m = Message {
                    title: "Your launcher settings can't be read".into(),
                    body: format!(
                        "{}: {e}.\n\nThe launcher won't write over it. Check the file, \
                         then start the launcher again. Details are in {}.",
                        file.display(),
                        log.path().display()
                    ),
                };
                (HubConfig::new(spec.fields), Some(m))
            }
        };
        if let Some(m) = &message {
            log.error(&m.body);
        }
        let rows: Vec<&'static OptionSpec> = rea_launcher::cfg::spec::active(backend.options())
            .filter(|s| !s.launcher_only)
            .collect();
        let keys: Vec<&'static str> = rows.iter().map(|s| s.key).collect();
        let games = backend.games().len();
        let art = load_art(&spec, &backend);
        let mut app = HubApp {
            model: Model::new(
                games,
                &keys,
                spec.fields.len(),
                !config.libraries.is_empty(),
            ),
            rows,
            config,
            log,
            library: None,
            scan: None,
            cache: Some(B::Cache::default()),
            rescan: false,
            form: None,
            note: None,
            heroes: vec![None; games],
            tiles: vec![None; games],
            art,
            fade: None,
            running: None,
            message,
            typed: vec![String::new(); spec.fields.len() + 1],
            notes: vec![None; spec.fields.len() + 1],
            extra: None,
            extra_typed: String::new(),
            extra_note: None,
            screen: None,
            close_after_stop: false,
            controls: ControlsView::default(),
            spec,
            backend,
        };
        app.start_scan();
        app
    }

    pub fn model(&self) -> &Model {
        &self.model
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    pub fn config(&self) -> &HubConfig {
        &self.config
    }
    /// The last scan, `None` while looking.
    pub fn library(&self) -> Option<&B::Library> {
        self.library.as_ref()
    }
    /// Replaces the scan (tests: a library the scan can't make).
    pub fn set_library(&mut self, library: B::Library) {
        self.library = Some(library);
    }
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }
    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }
    /// The monitor size the looks are decided for (tests set it).
    pub fn set_screen(&mut self, screen: Option<(u32, u32)>) {
        self.screen = screen;
    }

    // ------------------------------------------------------------ library

    fn start_scan(&mut self) {
        if self.config.libraries.is_empty() {
            return;
        }
        let Some(mut cache) = self.cache.take() else {
            self.rescan = true;
            return;
        };
        let roots = self.config.libraries.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let found = B::scan(&roots, &mut cache);
                let _ = tx.send((found, cache));
            })
            .expect("spawn scan thread");
        self.library = None;
        self.scan = Some(rx);
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan else { return };
        match rx.try_recv() {
            Ok((found, cache)) => {
                for line in B::unknown(&found) {
                    self.log.line(&line);
                }
                self.library = Some(found);
                self.cache = Some(cache);
                self.scan = None;
                if std::mem::take(&mut self.rescan) {
                    self.start_scan();
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.log.error("the library scan stopped unexpectedly");
                self.cache = Some(B::Cache::default());
                self.scan = None;
            }
        }
    }

    fn poll_art(&mut self, ctx: &egui::Context) {
        for (i, kind, img) in self.art.try_iter() {
            let name = format!("{}-{}", self.backend.games()[i].slug, kind as u8);
            let tex = ctx.load_texture(name, img, egui::TextureOptions::LINEAR);
            match kind {
                ArtKind::Hero => self.heroes[i] = Some(tex),
                ArtKind::Tile => self.tiles[i] = Some(tex),
            }
        }
    }

    pub(crate) fn found(&self, i: usize) -> Option<super::Found> {
        B::found(self.library.as_ref()?, i)
    }

    pub(crate) fn found_count(&self) -> usize {
        (0..self.backend.games().len())
            .filter(|i| self.found(*i).is_some())
            .count()
    }

    pub(crate) fn state(&self, i: usize) -> TileState {
        TileState::of(
            self.backend.support(i),
            self.library.is_some(),
            self.found(i).is_some(),
            self.backend.need(i),
        )
    }

    fn world(&self) -> World {
        let state = self.state(self.model.selected);
        World {
            playable: state.playable() && self.running.is_none(),
            has_library: !self.config.libraries.is_empty(),
            needs: matches!(state, TileState::Needs(_)),
        }
    }

    // ------------------------------------------------------------ settings

    fn ensure_form(&mut self) {
        let i = self.model.selected;
        if self.form.as_ref().map(|(at, _)| *at) != Some(i) {
            self.form = Some((i, self.backend.form(i, self.screen)));
            self.note = None;
        }
    }

    /// Why a row can't change now: the form's reason, else the platform's.
    pub(crate) fn row_off(&self, form: &B::Form, spec: &OptionSpec) -> Option<String> {
        form.disabled(spec.key).or_else(|| {
            (spec.helper && cfg!(windows)).then(|| self.spec.words.not_here.to_string())
        })
    }

    fn adjust(&mut self, key: &str, d: i32, wrap: bool) {
        self.ensure_form();
        let Some(spec) = self.rows.iter().find(|s| s.key == key).copied() else {
            return;
        };
        let Some((i, mut form)) = self.form.take() else {
            return;
        };
        if self.row_off(&form, spec).is_none() && form.adjust(key, d, wrap) {
            let path = self.spec.paths.settings(self.backend.games()[i].slug);
            self.note = Some(match form.save(&path) {
                Ok(()) => Note::ok("Saved. The game uses it the next time you press Play."),
                Err(e) => self.logged(format!(
                    "Couldn't save to {}: {e}. Check that the folder can be written.",
                    path.display()
                )),
            });
        }
        self.form = Some((i, form));
    }

    /// An error note, also written to `launcher.log`.
    fn logged(&self, text: String) -> Note {
        self.log.error(&text);
        Note::error(text)
    }

    // ------------------------------------------------------------ setup

    fn save_config(&self) -> Result<(), Note> {
        self.config
            .save(&self.spec.paths.config_file(), self.spec.fields)
            .map_err(|e| {
                self.logged(format!(
                    "Couldn't save the choice ({e}). Check that {} can be written.",
                    self.spec.paths.data.display()
                ))
            })
    }

    fn submit_path(&mut self, n: usize) {
        self.notes[n] = Some(match n.checked_sub(1) {
            None => self.add_library(),
            Some(k) => self.use_path(k),
        });
    }

    fn add_library(&mut self) -> Note {
        let dir = match typed_path(
            &self.typed[0],
            rea_launcher::hub::PathKind::Folder,
            "Type the folder's path first.",
        ) {
            Ok(dir) => dir,
            Err(e) => return Note::error(e),
        };
        let before = self.config.clone();
        self.config.add_library(&dir);
        if let Err(note) = self.save_config() {
            self.config = before;
            return note;
        }
        self.log.line(&format!("library added: {}", dir.display()));
        self.typed[0].clear();
        self.start_scan();
        Note::ok(format!("Added {}.", dir.display()))
    }

    fn use_path(&mut self, k: usize) -> Note {
        let field = self.spec.fields[k];
        let path = match typed_path(&self.typed[k + 1], field.kind, field.empty) {
            Ok(p) => p,
            Err(e) => return Note::error(e),
        };
        let before = self.config.paths[k].replace(path);
        if let Err(note) = self.save_config() {
            self.config.paths[k] = before;
            return note;
        }
        Note::ok(field.saved)
    }

    fn opened_setup(&mut self) {
        self.typed[0].clear();
        for (k, p) in self.config.paths.iter().enumerate() {
            self.typed[k + 1] = p
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }
        self.notes.iter_mut().for_each(|n| *n = None);
    }

    // ------------------------------------------------------------ extra page

    fn opened_extra(&mut self) {
        self.extra = self.backend.extra(self.model.selected);
        self.extra_typed = self
            .extra
            .as_ref()
            .map(|e| e.start.clone())
            .unwrap_or_default();
        self.extra_note = None;
    }

    fn submit_extra(&mut self) {
        let i = self.model.selected;
        let note = self.backend.submit_extra(i, &self.extra_typed);
        let title = self.backend.games()[i].title;
        self.log.line(&format!("{title}: {}", note.text));
        self.extra_note = Some(note);
        self.extra = self.backend.extra(i);
        let w = self.world();
        self.model.settle(w);
    }

    // ------------------------------------------------------------ input

    fn gather(&mut self, ctx: &egui::Context) -> Vec<Input> {
        // drained every frame: a game's pad presses never reach the hub
        let pads = self.backend.pad();
        if self.running.is_some() {
            return vec![];
        }
        let typing = self.model.page.has_fields() && matches!(self.model.focus, Focus::Field(_));
        let mut out = pads;
        ctx.input(|i| {
            let keys: &[(egui::Key, Input)] = if typing {
                &[
                    (egui::Key::Enter, Input::Press),
                    (egui::Key::Escape, Input::Escape),
                    (egui::Key::ArrowUp, Input::Up),
                    (egui::Key::ArrowDown, Input::Down),
                ]
            } else {
                &[
                    (egui::Key::ArrowUp, Input::Up),
                    (egui::Key::ArrowDown, Input::Down),
                    (egui::Key::ArrowLeft, Input::Left),
                    (egui::Key::ArrowRight, Input::Right),
                    (egui::Key::Enter, Input::Press),
                    (egui::Key::Space, Input::Press),
                    (egui::Key::Escape, Input::Escape),
                    (egui::Key::Backspace, Input::Back),
                    (egui::Key::PageUp, Input::PreviousTab),
                    (egui::Key::PageDown, Input::NextTab),
                ]
            };
            for (k, inp) in keys {
                if i.key_pressed(*k) {
                    out.push(*inp);
                }
            }
        });
        out
    }

    fn handle(&mut self, input: Input, ctx: &egui::Context) {
        if self.message.is_some() {
            if matches!(
                input,
                Input::Press | Input::Back | Input::Escape | Input::Start
            ) {
                self.message = None;
            }
            return;
        }
        let before = self.model.selected;
        let effect = self.model.input(input, self.world());
        self.apply(effect, ctx);
        if self.model.selected != before {
            self.selected_changed(before, ctx);
        }
    }

    fn selected_changed(&mut self, from: usize, ctx: &egui::Context) {
        self.fade = Some((from, ctx.input(|i| i.time)));
        let w = self.world();
        self.model.settle(w);
    }

    fn apply(&mut self, effect: Effect, ctx: &egui::Context) {
        match effect {
            Effect::None => {}
            Effect::Play => self.play(),
            Effect::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Effect::Adjust(key, d, wrap) => self.adjust(key, d, wrap),
            Effect::Rescan => self.start_scan(),
            Effect::SubmitPath(n) => self.submit_path(n),
            Effect::Finish => {
                self.model.open(Page::Hub);
                if self.library.is_none() && self.scan.is_none() {
                    self.start_scan();
                }
            }
            Effect::OpenedSettings => self.ensure_form(),
            Effect::OpenedSetup => self.opened_setup(),
            Effect::OpenedExtra => self.opened_extra(),
            Effect::SubmitExtra => self.submit_extra(),
        }
    }

    pub(crate) fn click(&mut self, c: Click, ctx: &egui::Context) {
        if let Click::StopGame = c {
            if let Some(run) = &self.running {
                run.stop.store(true, Ordering::SeqCst);
            }
            return;
        }
        // while a game runs or a message is up, only its own buttons work
        if self.running.is_some() || (self.message.is_some() && !matches!(c, Click::Dismiss)) {
            return;
        }
        match c {
            Click::Dismiss => self.message = None,
            Click::StopGame => {}
            Click::Focus(f, input) => {
                self.model.focus = f;
                if let Some(i) = input {
                    self.handle(i, ctx);
                }
            }
            Click::Tile(i, double) => {
                let before = self.model.selected;
                self.model.select(i);
                self.model.focus = Focus::Tile;
                if before != i {
                    self.selected_changed(before, ctx);
                }
                if double {
                    self.handle(Input::Press, ctx);
                }
            }
        }
    }

    // ------------------------------------------------------------ a game

    fn play(&mut self) {
        let i = self.model.selected;
        if self.running.is_some() || !self.state(i).playable() {
            return;
        }
        let Some(library) = &self.library else { return };
        let job = match self.backend.session(i, library, &self.config, self.screen) {
            Ok(job) => job,
            Err(e) => return self.ended(i, false, Err(e)),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let reporter = Reporter::new(tx.clone(), Arc::clone(&stop));
        std::thread::Builder::new()
            .name("session".into())
            .spawn(move || {
                let r = job(&reporter);
                let _ = tx.send(Progress::Ended(r));
            })
            .expect("spawn session thread");
        self.running = Some(Running {
            game: i,
            stage: None,
            playing: false,
            progress: rx,
            stop,
        });
    }

    fn poll_running(&mut self, ctx: &egui::Context) {
        let Some(run) = self.running.as_mut() else {
            return;
        };
        if ctx.input(|i| i.viewport().close_requested()) {
            // closing the window must not leave the game behind: stop it,
            // then close
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            run.stop.store(true, Ordering::SeqCst);
            self.close_after_stop = true;
        }
        let ended = loop {
            match run.progress.try_recv() {
                Ok(Progress::SettingUp(stage)) => run.stage = Some(stage),
                Ok(Progress::Playing) => run.playing = true,
                Ok(Progress::Ended(r)) => break Some(r),
                Err(TryRecvError::Empty) => break None,
                Err(TryRecvError::Disconnected) => {
                    break Some(Err("the game ended unexpectedly".into()));
                }
            }
        };
        let Some(r) = ended else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let (game, started) = (run.game, run.playing);
        self.running = None;
        self.backend.ended();
        self.ended(game, started, r);
        if std::mem::take(&mut self.close_after_stop) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    /// A session's end: an error goes to `launcher.log` and a message
    /// that says where to look.
    fn ended(&mut self, game: usize, started: bool, r: Result<(), String>) {
        let Err(e) = r else { return };
        let title = self.backend.games()[game].title;
        self.log.error(&format!("{title}: {e}"));
        let launcher_log = format!("Details are in {}.", self.log.path().display());
        self.message = Some(if started {
            let next = match self.backend.game_log(game) {
                Some(log) => format!("the game's log is {}.", log.display()),
                None => launcher_log,
            };
            Message {
                title: format!("{title} stopped with an error"),
                body: format!(
                    "{}.\n\nTry again. If it happens again, {next}",
                    sentence(&e)
                ),
            }
        } else {
            Message {
                title: format!("{title} couldn't start"),
                body: format!(
                    "{}.\n\nCheck that the game folder is complete, then try again. {launcher_log}",
                    sentence(&e)
                ),
            }
        });
    }

    // ------------------------------------------------------------ frame

    /// One frame of the window (also what the headless tests drive).
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        zoom_to_design(&ctx);
        if self.screen.is_none() {
            self.screen = monitor_pixels(&ctx);
        }
        self.controls = self.backend.controls();
        self.poll_scan();
        self.poll_art(&ctx);
        self.poll_running(&ctx);
        if self.model.page == Page::Settings {
            self.ensure_form();
        }
        // typed text first, so Enter in the same frame submits all of it
        if self.model.page.has_fields() && self.message.is_none() && self.running.is_none() {
            self.type_text(&ctx);
        }
        for input in self.gather(&ctx) {
            self.handle(input, &ctx);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
            let full = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!full));
        }
        let clicks = self.draw(ui);
        for c in clicks {
            self.click(c, &ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn type_text(&mut self, ctx: &egui::Context) {
        let Focus::Field(n) = self.model.focus else {
            return;
        };
        let events = ctx.input(|i| i.events.clone());
        match self.model.page {
            Page::Setup if edit_text(&mut self.typed[n], &events) => self.notes[n] = None,
            Page::Extra if edit_text(&mut self.extra_typed, &events) => self.extra_note = None,
            _ => {}
        }
    }
}

/// Laid out for 720 lines, zoomed to the window.
fn zoom_to_design(ctx: &egui::Context) {
    let native = ctx.native_pixels_per_point().unwrap_or(1.0);
    let pixels = ctx.input(|i| i.viewport_rect().height() * i.pixels_per_point);
    let want = (pixels / native / theme::DESIGN_HEIGHT).clamp(0.75, 4.0);
    if (want - ctx.zoom_factor()).abs() > 0.001 {
        ctx.set_zoom_factor(want);
    }
}

/// The monitor's size in pixels, when the platform says.
fn monitor_pixels(ctx: &egui::Context) -> Option<(u32, u32)> {
    ctx.input(|i| {
        let px = i.viewport().native_pixels_per_point.unwrap_or(1.0);
        i.viewport()
            .monitor_size
            .map(|m| ((m.x * px).round() as u32, (m.y * px).round() as u32))
    })
}

/// Decodes the pictures off the window's thread: the tiles first, then the
/// big ones (`art/<slug>-tile.jpg`, `art/<slug>.jpg`).
fn load_art<B: HubBackend>(spec: &HubSpec, backend: &B) -> Receiver<Art> {
    let (tx, rx) = mpsc::channel();
    let games = backend.games();
    let jobs: Vec<(usize, ArtKind, PathBuf)> = games
        .iter()
        .enumerate()
        .map(|(i, g)| {
            (
                i,
                ArtKind::Tile,
                spec.paths.art(&format!("{}-tile.jpg", g.slug)),
            )
        })
        .chain(
            games
                .iter()
                .enumerate()
                .map(|(i, g)| (i, ArtKind::Hero, spec.paths.art(&format!("{}.jpg", g.slug)))),
        )
        .collect();
    std::thread::Builder::new()
        .name("art".into())
        .spawn(move || {
            for (i, kind, path) in jobs {
                let Some(img) = std::fs::read(&path)
                    .ok()
                    .and_then(|b| crate::widgets::decode_jpeg(&b))
                else {
                    continue;
                };
                if tx.send((i, kind, img)).is_err() {
                    return;
                }
            }
        })
        .expect("spawn art thread");
    rx
}
