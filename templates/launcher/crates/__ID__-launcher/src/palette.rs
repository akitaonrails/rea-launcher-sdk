//! The settings window's colours and sizes, in the SDK's roles
//! (`rea_launcher_ui::theme`).
//!
//! PLACEHOLDER colours: a neutral charcoal with one amber accent. Choose
//! the game's own with a design skill (`frontend-design`, then a review
//! with `web-design-guidelines`) once the window's imagery is picked
//! (docs/STANDARDS.md "Give the window the game's identity"). The tests
//! below keep any palette within WCAG AA and the text-size rules.

use rea_launcher_ui::egui::Color32;
use rea_launcher_ui::theme::{Metrics, Palette, tint};

pub const BASE: Color32 = Color32::from_rgb(0x15, 0x17, 0x1c);
pub const PANEL: Color32 = Color32::from_rgb(0x21, 0x25, 0x2c);
pub const LINE: Color32 = Color32::from_rgb(0x76, 0x7d, 0x89);
pub const INK: Color32 = Color32::from_rgb(0xf1, 0xf3, 0xf6);
pub const MUTED: Color32 = Color32::from_rgb(0xaa, 0xb2, 0xbe);
pub const ACCENT: Color32 = Color32::from_rgb(0xf2, 0xb3, 0x3d);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x8f, 0x80);

pub const PALETTE: Palette = Palette {
    base: BASE,
    panel: PANEL,
    line: LINE,
    ink: INK,
    muted: MUTED,
    action: ACCENT,
    action_hover: Color32::from_rgb(0xff, 0xc9, 0x5c),
    on_action: BASE,
    select: ACCENT,
    on_select: BASE,
    error: ERROR,
    disabled_ink: Color32::from_rgb(0x6c, 0x73, 0x7e),
    disabled_line: Color32::from_rgb(0x2e, 0x33, 0x3b),
    focus: INK,
    selection: tint(ACCENT, 128),
};

pub const METRICS: Metrics = Metrics::DEFAULT;

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher_ui::theme;

    #[test]
    fn the_palette_meets_wcag_aa() {
        assert_eq!(
            theme::contrast_problems(&PALETTE, &METRICS),
            Vec::<String>::new()
        );
    }

    #[test]
    fn no_text_is_too_small() {
        assert_eq!(theme::text_size_problems(&METRICS), Vec::<String>::new());
    }
}
