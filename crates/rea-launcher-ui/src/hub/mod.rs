//! The hub: the SDK's third sanctioned look (docs/STANDARDS.md), one
//! launcher for a series of arcade games. Full screen, pad first, a hero
//! picture and a row of tiles, laid out for 720 lines and zoomed to the
//! window (it runs on 4K TVs). Pages: the hub, Controls, Settings (rows
//! from the launcher's option table), the setup page (library folders and
//! the launcher's path fields) and an optional extra page.
//!
//! The launcher supplies [`HubSpec`] (theme, layout, words, path fields,
//! folders, version) and a [`HubBackend`]; [`run`] owns the window, the
//! scan and art threads, keyboard, mouse and pad input, the session
//! thread, and the messages. The pure model is `rea_launcher::hub`.
//!
//! - Pad navigation comes from [`HubBackend::pad`] and is dropped while a
//!   game runs, so the game's presses never move the hub.
//! - Closing the window while a game runs stops the game first, then
//!   closes; the running overlay has a Stop button.
//! - Every error (a session's, a save's) is also written to
//!   `launcher.log`, and the message names the log to read.
//! - Settings rows the Linux helper owns are shown off on Windows, with
//!   the reason, rather than hidden.

mod app;
mod backend;
mod pages;
mod paint;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;
mod theme;

pub use app::{HubApp, Message};
pub use backend::{
    ButtonView, Check, ControlsView, Device, ExtraPage, Found, HubBackend, HubForm, HubGame,
    HubWords, Mark, Note, Reporter, Scanner, Session, Stage,
};
pub use paint::{Semantics, cover_uv, edit_text};
pub use theme::{
    DESIGN_HEIGHT, HubLayout, HubTheme, alpha, body, display, display_semi, install, over,
    semibold, size,
};

use eframe::egui;
use rea_launcher::hub::{HubPaths, PathField};
use rea_launcher::log::Log;

use crate::report::{self, Link, NativeAudience};

/// What a hub launcher is, besides its backend.
#[derive(Debug, Clone)]
pub struct HubSpec {
    pub theme: HubTheme,
    pub layout: HubLayout,
    pub words: HubWords,
    /// The launcher's paths besides the library folders (a BIOS file…).
    pub fields: &'static [PathField],
    pub paths: HubPaths,
    /// The footer's "Version …": `env!("CARGO_PKG_VERSION")`.
    pub version: &'static str,
}

/// Opens the hub full screen and runs it until it is closed.
pub fn run<B: HubBackend>(spec: HubSpec, backend: B) -> Result<(), String> {
    let t = spec.theme;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(t.title)
            .with_app_id(t.app_id)
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([640.0, 360.0])
            .with_fullscreen(true),
        ..Default::default()
    };
    eframe::run_native(
        t.title,
        options,
        Box::new(move |cc| {
            install(&cc.egui_ctx, &spec.theme);
            Ok(Box::new(Window(HubApp::new(spec, backend))))
        }),
    )
    .map_err(|e| format!("the window could not open: {e}"))
}

/// A message before (or instead of) the hub: a refused command, another
/// launcher already open, a game that can't start from `--play`. Written to
/// `log`, printed to stderr, and shown in a window in the hub's colours
/// when someone may be watching ([`report::fatal`]); `link` is the issue
/// form, when it helps.
pub fn fatal(
    theme: &HubTheme,
    log: &Log,
    text: &str,
    link: Option<Link<'_>>,
    mode: report::Mode,
) -> NativeAudience {
    log.line(&format!("error: {text}"));
    let message = report::Message {
        title: theme.title,
        text,
        link,
        details: None,
    };
    report::fatal(
        &theme.palette(),
        &crate::theme::Metrics::DEFAULT,
        &message,
        mode,
    )
}

struct Window<B: HubBackend>(HubApp<B>);

impl<B: HubBackend> eframe::App for Window<B> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.frame(ui);
    }
}
