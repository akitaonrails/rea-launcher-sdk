//! The unpack window: a game's installer being unpacked on the first start
//! (`rea_launcher::installer`, `rea_launcher_win32::innoextract`), so a
//! player never faces a silent screen for minutes. CMR2005's window, with
//! the game's title, the installer's name and the photo as parameters.
//!
//! The page: the launcher's photo strip on the left (with its credit, when
//! given: [`Unpacking::with_credit`]); the game's title, one
//! line saying what is happening and from which installer, the progress
//! bar with sizes and time left, and a note that the installer is only
//! read; "Stop unpacking" in the footer. Closing the window, Escape and
//! the pad's B/Back stop it too; the pad's A presses the focused button.
//!
//! ```no_run
//! # use rea_launcher::installer::{UnpackJob, UnpackOutcome};
//! # use rea_launcher_ui::theme::{Metrics, Palette};
//! # use rea_launcher_ui::window::{SIZE, WindowFit};
//! use rea_launcher_ui::unpack::{self, Unpacking};
//! # fn run(job: &mut dyn UnpackJob) -> Result<UnpackOutcome, String> {
//! let fit = WindowFit::new(SIZE, false, Some((1920, 1080)), 1.0);
//! let state = Unpacking::new("Colin McRae Rally 2005", "GOG's installer", 3_564_462_813);
//! unpack::show(
//!     &Palette::default(),
//!     &Metrics::DEFAULT,
//!     unpack::Window { title: "CMR2005 Launcher", fit, hero_jpeg: None, photo_width: 280.0 },
//!     state,
//!     job,
//!     || 0, // XInput buttons: rea_launcher_win32::xinput on Windows
//! )
//! # }
//! ```

use eframe::egui::{self, Key, Rect, RichText, Role, TextureHandle, Ui, pos2, vec2};
use rea_launcher::installer::{UnpackJob, UnpackOutcome};

use crate::download;
use crate::pad::{self, Nav, PadNav};
use crate::theme::{self, Metrics, Palette, body, condensed, size};
use crate::widgets::{self, Crop};
use crate::window::{Layout, WindowFit};

/// The footer button's text.
pub const STOP: &str = "Stop unpacking";

/// What the window shows, and whether the player stopped it.
pub struct Unpacking {
    /// The game's title, the page's heading.
    pub game: String,
    /// The installer, as the player knows it ("GOG's installer").
    pub installer: String,
    /// Bytes written so far, and the unpacked game's size.
    pub done: u64,
    pub total: u64,
    /// Seconds since the unpack began, for the time left.
    pub elapsed: f64,
    /// The launcher's photo for the strip (none: only its fades).
    pub hero: Option<TextureHandle>,
    /// The photo strip's width (the launcher's own `PHOTO_W`).
    pub photo_width: f32,
    /// The photo's credit (`Photo: name, CC BY 2.0`), shown small under
    /// it when the licence asks for one ([`Unpacking::with_credit`]).
    pub credit: Option<String>,
    /// Stop was pressed (the caller stops the unpack).
    pub cancel: bool,
}

impl Unpacking {
    pub fn new(game: &str, installer: &str, total: u64) -> Unpacking {
        Unpacking {
            game: game.to_owned(),
            installer: installer.to_owned(),
            done: 0,
            total,
            elapsed: 0.0,
            hero: None,
            photo_width: 280.0,
            credit: None,
            cancel: false,
        }
    }

    /// Shows `credit` (`Photo: name, CC BY 2.0`) in the photo strip's
    /// lower left, for a photo whose licence asks for one.
    pub fn with_credit(mut self, credit: &str) -> Unpacking {
        self.credit = Some(credit.to_owned());
        self
    }

    /// The bar's fraction, never past full.
    pub fn fraction(&self) -> f32 {
        (self.done as f64 / self.total.max(1) as f64).min(1.0) as f32
    }

    /// "1.2 GB of 3.6 GB, about 2 min left" (the time once there is a rate).
    pub fn status(&self) -> String {
        let sizes = format!(
            "{} of {}",
            download::size_text(self.done.min(self.total)),
            download::size_text(self.total)
        );
        if self.elapsed < 3.0 || self.done == 0 {
            return sizes;
        }
        let rate = self.done as f64 / self.elapsed;
        let left = self.total.saturating_sub(self.done) as f64 / rate;
        format!("{sizes}, {}", download::time_left(left))
    }

    /// The line under the title.
    pub fn summary(&self) -> String {
        format!(
            "Unpacking the game from {}. This happens once, on the first start.",
            self.installer
        )
    }
}

/// The note under the progress.
pub const NOTE: &str = "The launcher copies the game out of your installer files into a \
     folder of its own. The installer itself is not run, and its files are only read. \
     The game starts as soon as this is done.";

/// What the stop button does, for screen readers.
pub const STOP_DESCRIPTION: &str =
    "Stops unpacking and removes what was copied so far. Your installer files stay as they are.";

/// Draws the unpack page filling `ui`. Accessibility: the title is a
/// heading, the progress bar is named "Unpacking progress" with the sizes
/// as its description, the stop button says what it does, and a photo
/// credit is a label. Escape stops the unpack.
pub fn draw_unpack(ui: &mut Ui, u: &mut Unpacking) {
    let p = theme::palette(ui.ctx());
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape)) {
        u.cancel = true;
    }
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, p.base);
    let layout = Layout::new(full, u.photo_width);
    widgets::hero_image(
        ui.painter(),
        layout.photo,
        u.hero.as_ref(),
        Crop::Start,
        p.base,
    );
    if let Some(credit) = &u.credit {
        widgets::photo_credit(ui, layout.photo, credit);
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(layout.body), |ui| {
        let title = ui.add(
            egui::Label::new(RichText::new(&u.game).font(condensed(38.0)).color(p.ink)).wrap(),
        );
        title.widget_info(|| egui::WidgetInfo::labeled(Role::Heading, true, &u.game));
        ui.add_space(4.0);
        ui.label(
            RichText::new(u.summary())
                .font(body(size::LABEL))
                .color(p.ink),
        );
        ui.add_space(24.0);
        let status = u.status();
        let bar = widgets::progress_bar(ui, u.fraction());
        widgets::name_control(&bar, Some("Unpacking progress"), Some(&status));
        ui.add_space(6.0);
        widgets::note(ui, &status);
        ui.add_space(14.0);
        widgets::note(ui, NOTE);
    });
    let button = Rect::from_min_size(
        pos2(
            layout.footer.right() - 190.0,
            layout.footer.center().y - 20.0,
        ),
        vec2(190.0, 40.0),
    );
    let stop = ui
        .scope_builder(egui::UiBuilder::new().max_rect(button), |ui| {
            widgets::outline_button(ui, STOP)
        })
        .inner;
    widgets::name_control(&stop, None, Some(STOP_DESCRIPTION));
    if stop.clicked() {
        u.cancel = true;
    }
}

/// Whether a pad step stops the unpack (B/Back; A presses the focused
/// button through [`pad::inject`]).
pub fn pad_cancels(nav: Nav) -> bool {
    nav == Nav::Back
}

/// The window around [`draw_unpack`].
pub struct Window<'a> {
    /// The window's title (the launcher's, never the game's).
    pub title: &'a str,
    /// Its size and DPI fit: the settings window's `WindowFit`.
    pub fit: WindowFit,
    /// The photo for the strip, decoded at start.
    pub hero_jpeg: Option<&'static [u8]>,
    pub photo_width: f32,
}

/// Shows the unpack window over `state` until `job` ends or the player
/// stops it (the stop button, Escape, the pad's B, or closing the window,
/// which all call `job.cancel()`). `buttons` gives the pad's XInput
/// buttons each frame (`|| 0` for none).
pub fn show(
    palette: &Palette,
    metrics: &Metrics,
    window: Window<'_>,
    mut state: Unpacking,
    job: &mut dyn UnpackJob,
    buttons: impl FnMut() -> u16,
) -> Result<UnpackOutcome, String> {
    let (palette, metrics) = (*palette, *metrics);
    let options = window.fit.native_options(window.title);
    let mut outcome = None;
    state.photo_width = window.photo_width;
    eframe::run_native(
        window.title,
        options,
        Box::new(|cc| {
            theme::install_with(&cc.egui_ctx, &palette, &metrics);
            state.hero = window
                .hero_jpeg
                .and_then(|jpeg| widgets::load_jpeg(&cc.egui_ctx, "hero", jpeg));
            Ok(Box::new(UnpackApp {
                state,
                fit: window.fit,
                job,
                started: std::time::Instant::now(),
                outcome: &mut outcome,
                pad: PadNav::default(),
                buttons: Box::new(buttons),
                pending: Vec::new(),
            }))
        }),
    )
    .map_err(|error| error.to_string())?;
    // Closed some other way: the app stopped the unpack too.
    Ok(outcome.unwrap_or(UnpackOutcome::Cancelled))
}

struct UnpackApp<'a> {
    state: Unpacking,
    fit: WindowFit,
    job: &'a mut dyn UnpackJob,
    started: std::time::Instant,
    outcome: &'a mut Option<UnpackOutcome>,
    pad: PadNav,
    buttons: Box<dyn FnMut() -> u16 + 'a>,
    pending: Vec<Nav>,
}

impl eframe::App for UnpackApp<'_> {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let navs = self.pad.update((self.buttons)());
        pad::inject(ctx, raw, &navs);
        self.pending.extend(navs);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.fit.first_frame(&ctx);
        if self.outcome.is_none() {
            if let Some(result) = self.job.finished() {
                *self.outcome = Some(UnpackOutcome::Finished(result));
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.state.done = self.job.done();
                self.state.elapsed = self.started.elapsed().as_secs_f64();
            }
        }
        let closing = ctx.input(|input| input.viewport().close_requested());
        self.state.cancel |= std::mem::take(&mut self.pending)
            .into_iter()
            .any(pad_cancels);
        draw_unpack(ui, &mut self.state);
        if self.outcome.is_none() && (self.state.cancel || closing) {
            self.job.cancel();
            *self.outcome = Some(UnpackOutcome::Cancelled);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Progress, and a pad sends no window events: look again soon.
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, Queryable};
    use crate::theme::Palette;
    use egui::accesskit::Role;

    fn open(u: Unpacking) -> testing::Harness<'static, Unpacking> {
        testing::harness(&Palette::default(), &Metrics::DEFAULT, u, draw_unpack)
    }

    #[test]
    fn the_window_shows_progress_and_time_left() {
        let mut u = Unpacking::new("Colin McRae Rally 2005", "GOG's installer", 3_600_000_000);
        assert_eq!(u.status(), "0 MB of 3.6 GB");
        u.done = 1_200_000_000;
        u.elapsed = 60.0;
        assert_eq!(u.status(), "1.2 GB of 3.6 GB, about 2 min left");
        assert!((u.fraction() - 1.0 / 3.0).abs() < 1e-6);
        u.done = 4_000_000_000;
        assert_eq!(u.fraction(), 1.0, "never past full");
        assert!(u.status().starts_with("3.6 GB of 3.6 GB"));
        assert_eq!(Unpacking::new("g", "i", 0).fraction(), 0.0);
    }

    #[test]
    fn the_player_reads_what_happens_and_from_which_installer() {
        let mut u = Unpacking::new("Colin McRae Rally 04", "the Magipack repack", 2_000_000_000);
        u.done = 500_000_000;
        let ui = open(u);
        ui.get_by_role_and_label(Role::Heading, "Colin McRae Rally 04");
        ui.get_by_label(
            "Unpacking the game from the Magipack repack. This happens once, on the first start.",
        );
        ui.get_by_label(NOTE);
        let bar = ui.get_by_role_and_label(Role::ProgressIndicator, "Unpacking progress");
        assert_eq!(
            testing::description(&bar).as_deref(),
            Some("500 MB of 2.0 GB")
        );
        let stop = ui.get_by_role_and_label(Role::Button, STOP);
        assert_eq!(
            testing::description(&stop).as_deref(),
            Some(STOP_DESCRIPTION)
        );
    }

    #[test]
    fn a_photo_credit_is_shown_under_the_photo_when_given() {
        let credit = "Photo: Carolyn Williams, CC BY 2.0";
        let u = Unpacking::new("Game", "the installer", 100).with_credit(credit);
        assert_eq!(u.credit.as_deref(), Some(credit));
        let ui = open(u);
        let label = ui.get_by_role_and_label(Role::Label, credit);
        let bounds = label.rect();
        assert!(
            bounds.max.x <= 280.0 + 1.0,
            "inside the photo strip: {bounds:?}"
        );
        let ui = open(Unpacking::new("Game", "the installer", 100));
        assert!(
            ui.query_by_label_contains("Photo:").is_none(),
            "none by default"
        );
    }

    #[test]
    fn the_player_stops_it_with_the_button_escape_or_the_pad() {
        let mut ui = open(Unpacking::new("Game", "the installer", 100));
        ui.get_by_role_and_label(Role::Button, STOP).click();
        ui.run();
        assert!(ui.state().cancel);

        let mut ui = open(Unpacking::new("Game", "the installer", 100));
        ui.key_press(Key::Escape);
        ui.run();
        assert!(ui.state().cancel);

        // Keyboard (and the pad's A, injected as Enter/Space): focus the
        // button, press it.
        let mut ui = open(Unpacking::new("Game", "the installer", 100));
        assert!(!ui.state().cancel);
        ui.key_press(Key::Tab);
        ui.run();
        ui.key_press(Key::Enter);
        ui.run();
        assert!(
            ui.state().cancel,
            "Tab reaches the button, Enter presses it"
        );

        assert!(pad_cancels(Nav::Back));
        assert!(!pad_cancels(Nav::Down));
    }
}
