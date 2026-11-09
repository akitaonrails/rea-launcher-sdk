//! The settings-window kit shared by the rea launchers (eframe/egui).
//!
//! - [`theme`]: a [`theme::Palette`] of colour *roles* and
//!   [`theme::Metrics`] (the values are each game's), the bundled Barlow
//!   fonts, and [`theme::install_with`].
//! - [`widgets`]: chips, toggles, look cards, buttons, folder field, tabs,
//!   the Play plate and footer, gradients, decal stripes, hero image.
//! - [`window`]: window sizing that survives XWayland under Wine (and, opted
//!   in, fits the DPI on Windows), plus the common page layout.
//! - [`pad`]: XInput buttons → egui focus navigation.
//! - [`report`]: a native launcher's message window and who sees a
//!   message (terminal, window or log only).
//! - [`download`]: a download row (progress, Download/Cancel/Remove).
//! - [`unpack`]: the progress window while a game's installer is unpacked
//!   (`rea_launcher::installer`).
//! - [`spec`]: window rows from the core option table, with a parity check.
//! - `testing` (feature `testing`): headless GUI tests through AccessKit.
//!
//! `eframe` and `egui` are re-exported, so a launcher needs no matching
//! eframe dependency of its own: `use rea_launcher_ui::egui;`.
//!
//! Widgets read the palette that [`theme::install`] stored in the egui
//! context, so calls stay short: `widgets::toggle(ui, &mut on, "VSync", None)`.

pub use eframe;
pub use eframe::egui;

pub mod download;
pub mod pad;
pub mod report;
pub mod spec;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
pub mod theme;
pub mod unpack;
pub mod widgets;
pub mod window;
