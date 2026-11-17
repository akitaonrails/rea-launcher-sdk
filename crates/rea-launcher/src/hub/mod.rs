//! The pure side of a hub launcher (one launcher for a series of arcade
//! games: full screen, pad first, a tile per game; `rea_launcher_ui::hub`
//! draws it): the window's model ([`Model`]: pages, focus, what each input
//! does), what a tile says ([`TileState`]), `launcher.cfg` ([`HubConfig`]
//! with the launcher's [`PathField`]s), the folders ([`HubPaths`]), the
//! command line ([`cli`]), and small helpers for the settings rows.

pub mod cli;
mod config;
mod dirs;
mod model;

pub use config::{
    HubConfig, LIBRARY, PathField, PathKind, expand_home, expand_home_in, typed_path,
};
pub use dirs::{HubDirs, HubPaths};
pub use model::{
    Effect, Focus, Input, Item, Model, Need, Page, Support, Tab, TileState, World, settings_items,
};

/// An error message as a sentence: capital first letter, no final stop
/// (the caller adds one).
pub fn sentence(e: &str) -> String {
    let e = e.trim().trim_end_matches('.');
    let mut c = e.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Something went wrong".into(),
    }
}

/// The item `d` steps from `cur` in `all`, clamped at the ends (a value
/// not in `all` counts as the first).
pub fn step<T: Copy + PartialEq>(all: &[T], cur: T, d: i32) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    all[(i + d).clamp(0, all.len() as i32 - 1) as usize]
}

/// [`step`], wrapping around at the ends when `wrap` (Press on a row:
/// the next choice, then the first again).
pub fn pick<T: Copy + PartialEq>(all: &[T], cur: T, d: i32, wrap: bool) -> T {
    let next = step(all, cur, d);
    match (wrap && next == cur, d > 0) {
        (false, _) => next,
        (true, true) => all[0],
        (true, false) => all[all.len() - 1],
    }
}

/// A screen size as the rows show it: `1920 × 1080 (1080p)`; only
/// 1080p, 1440p and 4K are named.
pub fn size_name(w: u32, h: u32) -> String {
    let tag = match (w, h) {
        (1920, 1080) => " (1080p)",
        (2560, 1440) => " (1440p)",
        (3840, 2160) => " (4K)",
        _ => "",
    };
    format!("{w} × {h}{tag}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_as_sentences() {
        assert_eq!(
            sentence("starting gamescope (is it installed?)."),
            "Starting gamescope (is it installed?)"
        );
        assert_eq!(sentence("  "), "Something went wrong");
        assert_eq!(sentence("économie"), "Économie");
    }

    #[test]
    fn choices_step_clamp_and_wrap_on_press() {
        let all = [1, 2, 3];
        assert_eq!(step(&all, 2, 1), 3);
        assert_eq!(step(&all, 3, 1), 3);
        assert_eq!(step(&all, 1, -9), 1);
        assert_eq!(step(&all, 7, 1), 2, "an unknown value counts as the first");
        assert_eq!(pick(&all, 3, 1, false), 3);
        assert_eq!(pick(&all, 3, 1, true), 1);
        assert_eq!(pick(&all, 1, -1, true), 3);
        assert_eq!(pick(&all, 1, 1, true), 2);
    }

    #[test]
    fn sizes_have_their_common_names() {
        assert_eq!(size_name(1920, 1080), "1920 × 1080 (1080p)");
        assert_eq!(size_name(3840, 2160), "3840 × 2160 (4K)");
        assert_eq!(size_name(1360, 768), "1360 × 768");
        assert_eq!(size_name(1280, 720), "1280 × 720");
    }
}
