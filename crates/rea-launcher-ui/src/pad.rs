//! Gamepad navigation of the settings window.
//!
//! The button mask is XInput's `wButtons` layout (`rea_launcher::pad`'s
//! constants; what `rea_launcher_win32::xinput::buttons()` returns, and
//! what a native Linux launcher maps its joystick into, as SCR does). Edge
//! detection ([`PadNav`], one step per press, never per frame held) lives
//! in `rea_launcher::pad` and is re-exported here. This module turns the
//! steps into egui keys: SCR's model expressed as keys, so
//! egui's own focus does the work and the focus stays visible:
//!
//! | Pad | egui |
//! |---|---|
//! | D-pad down / up | Tab / Shift+Tab (next / previous control) |
//! | D-pad left / right | ←/→: steps a focused chip row, else moves focus |
//! | A | Space: presses the focused control |
//! | Start, LB/RB, B | returned to the launcher: Play, tabs, back |
//!
//! ```no_run
//! # use eframe::egui;
//! # use rea_launcher_ui::pad::{self, Nav, PadNav};
//! # struct App { pad: PadNav, pending: Vec<Nav> }
//! # fn buttons() -> u16 { 0 }
//! # impl App { fn play(&mut self) {} fn step_tab(&mut self, _: isize) {}
//! fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
//!     let navs = self.pad.update(buttons());
//!     pad::inject(ctx, raw, &navs);
//!     self.pending.extend(navs);
//! }
//!
//! fn ui(&mut self, ui: &mut egui::Ui) {
//!     // Taken, not read: should egui run a second pass this frame, Play
//!     // must not fire twice.
//!     for nav in std::mem::take(&mut self.pending) {
//!         match nav {
//!             Nav::Play => self.play(),
//!             Nav::NextTab => self.step_tab(1),
//!             Nav::PreviousTab => self.step_tab(-1),
//!             _ => {}
//!         }
//!     }
//!     // A pad sends no window events: look again soon.
//!     ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
//! }
//! # }
//! ```

use eframe::egui::{self, Key, Modifiers};

pub use rea_launcher::pad::{Nav, PadNav};

/// The key a step becomes inside egui; `None` for the launcher's own
/// steps. `typing` (a text field has focus) keeps A from typing a space.
pub fn key_for(nav: Nav, typing: bool) -> Option<(Key, Modifiers)> {
    match nav {
        Nav::Down => Some((Key::Tab, Modifiers::NONE)),
        Nav::Up => Some((Key::Tab, Modifiers::SHIFT)),
        Nav::Left => Some((Key::ArrowLeft, Modifiers::NONE)),
        Nav::Right => Some((Key::ArrowRight, Modifiers::NONE)),
        Nav::Press if !typing => Some((Key::Space, Modifiers::NONE)),
        _ => None,
    }
}

/// Adds a key press and release to `raw` for each focus step in `navs`.
/// Call from `eframe::App::raw_input_hook`.
pub fn inject(ctx: &egui::Context, raw: &mut egui::RawInput, navs: &[Nav]) {
    let typing = ctx.egui_wants_keyboard_input();
    for &nav in navs {
        let Some((key, modifiers)) = key_for(nav, typing) else {
            continue;
        };
        for pressed in [true, false] {
            raw.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_become_focus_keys() {
        assert_eq!(key_for(Nav::Down, false), Some((Key::Tab, Modifiers::NONE)));
        assert_eq!(key_for(Nav::Up, false), Some((Key::Tab, Modifiers::SHIFT)));
        assert_eq!(
            key_for(Nav::Right, false),
            Some((Key::ArrowRight, Modifiers::NONE))
        );
        assert_eq!(
            key_for(Nav::Press, false),
            Some((Key::Space, Modifiers::NONE))
        );
        assert_eq!(key_for(Nav::Press, true), None, "A never types a space");
        assert_eq!(key_for(Nav::Play, false), None);
        assert_eq!(key_for(Nav::NextTab, false), None);
    }

    #[test]
    fn inject_adds_press_and_release() {
        let ctx = egui::Context::default();
        let mut raw = egui::RawInput::default();
        inject(&ctx, &mut raw, &[Nav::Down, Nav::Play]);
        assert_eq!(raw.events.len(), 2);
        assert!(matches!(
            raw.events[0],
            egui::Event::Key {
                key: Key::Tab,
                pressed: true,
                ..
            }
        ));
        assert!(matches!(
            raw.events[1],
            egui::Event::Key {
                key: Key::Tab,
                pressed: false,
                ..
            }
        ));
    }
}
