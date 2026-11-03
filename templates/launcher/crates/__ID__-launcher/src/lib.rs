//! The @GAME@ launcher: everything that can be tested on a Linux host lives
//! in this library; `main.rs` only wires it to Windows.
//!
//! - [`edition`]: which builds of @GAME_EXE@ are known (SHA-256), and the
//!   refusal for any other build.
//! - [`config`]: the one option table behind `launcher.cfg`, the flags,
//!   `--help`, the template and the window rows.
//! - [`cli`]: what a command line asks for, and the `--check` report.
//! - [`form`]: the settings window's state as `launcher.cfg` text.
//! - [`runtime`]: the private runtime copy of the game folder.
//! - `palette`, `ui`: the settings window (Windows, and the host's tests).

/// The settings window's title. It must differ from the game's own window
/// title: a second start brings this window forward
/// (`rea_launcher_win32::instance::Window::Own`), never the game's.
pub const WINDOW_TITLE: &str = "@GAME@ Launcher";

/// The single-instance name: one launcher at a time per user session (a
/// named mutex on Windows, per prefix under Wine).
pub const INSTANCE: &str = "rea-@ID@-launcher";

pub mod cli;
pub mod config;
pub mod edition;
pub mod form;
pub mod runtime;

#[cfg(any(windows, test))]
pub mod palette;
#[cfg(any(windows, test))]
pub mod ui;
