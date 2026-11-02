//! The settings-window kit shared by the rea launchers (eframe/egui).
//!
//! - [`theme`]: a [`theme::Palette`] of colour *roles* (the values are each
//!   game's), the bundled Barlow fonts, and [`theme::install`].
//! - [`widgets`]: chips, toggles, look cards, buttons, folder field, tabs,
//!   the Play plate and footer, gradients, decal stripes, hero image.
//! - [`window`]: window sizing that survives XWayland under Wine and fits
//!   the DPI on Windows, plus the common page layout.
//! - [`pad`]: XInput buttons → egui focus navigation.
//!
//! Widgets read the palette that [`theme::install`] stored in the egui
//! context, so calls stay short: `widgets::toggle(ui, &mut on, "VSync", None)`.

pub mod pad;
pub mod theme;
pub mod widgets;
pub mod window;
