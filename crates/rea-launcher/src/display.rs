//! Display choices, from sizes the win32 crate measures.
//!
//! - [`game_monitor`] is CMR4's `display::choose` (as adopted by the win32
//!   crate): with a portrait primary (Wine lists a rotated monitor first)
//!   the game's 16:9 picture was squeezed into a strip, so the largest
//!   landscape monitor is chosen instead.
//! - [`landscape`] and [`STEAM_DECK_SCREEN`] are the CMR3/SRR/OR look
//!   helpers (SRR ed2a9b0: recommend the Steam Deck look on a Deck, in
//!   either orientation).
//! - [`largest_16_9`] is CMR4's `default_resolution`, generic.

/// One monitor in enumeration order, in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn landscape(self) -> bool {
        self.width >= self.height
    }

    pub fn size(self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// The primary monitor, or the first one when none is flagged.
pub fn primary(monitors: &[Monitor]) -> Option<Monitor> {
    monitors
        .iter()
        .find(|m| m.primary)
        .or(monitors.first())
        .copied()
}

/// Where a fullscreen game should land, and its index in `monitors`: the
/// primary when it is landscape, else the largest landscape monitor (ties
/// keep the first), else the primary anyway. CMR4 hands a non-primary
/// choice to dgVoodoo as `FullScreenOutput = index + 1`.
pub fn game_monitor(monitors: &[Monitor]) -> Option<(usize, Monitor)> {
    let primary_index = monitors.iter().position(|m| m.primary).unwrap_or(0);
    let primary = *monitors.get(primary_index)?;
    if primary.landscape() {
        return Some((primary_index, primary));
    }
    monitors
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, m)| m.landscape())
        .max_by_key(|(index, m)| (u64::from(m.width) * u64::from(m.height), usize::MAX - index))
        .or(Some((primary_index, primary)))
}

/// The Steam Deck's screen, landscape.
pub const STEAM_DECK_SCREEN: (u32, u32) = (1280, 800);

/// A screen size as a landscape resolution a game can use (even sizes,
/// 640×480 up to 7680×4320), or `None`.
pub fn landscape((a, b): (u32, u32)) -> Option<(u32, u32)> {
    let (w, h) = (a.max(b) & !1, a.min(b) & !1);
    (w >= 640 && h >= 480 && w <= 7680 && h <= 4320).then_some((w, h))
}

/// True for a Steam Deck's screen, in either orientation.
pub fn is_steam_deck(screen: Option<(u32, u32)>) -> bool {
    screen.and_then(landscape) == Some(STEAM_DECK_SCREEN)
}

/// True for an exact 16:9 size.
pub fn is_16_9((w, h): (u32, u32)) -> bool {
    h > 0 && u64::from(w) * 9 == u64::from(h) * 16
}

/// The largest exact 16:9 size that fits `screen` turned landscape, at
/// most `max` and at least 640×360; `fallback` for an unknown screen.
pub fn largest_16_9(
    screen: Option<(u32, u32)>,
    max: (u32, u32),
    fallback: (u32, u32),
) -> (u32, u32) {
    let Some((a, b)) = screen else {
        return fallback;
    };
    let (screen_w, screen_h) = (a.max(b).min(max.0), a.min(b).min(max.1));
    let mut height = screen_h;
    while height >= 360 {
        let width = height / 9 * 16;
        if height % 9 == 0 && width <= screen_w && width >= 640 {
            return (width, height);
        }
        height -= 1;
    }
    (640, 360)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(width: u32, height: u32, primary: bool) -> Monitor {
        Monitor {
            width,
            height,
            primary,
        }
    }

    #[test]
    fn landscape_primary_is_kept() {
        let monitors = [m(2160, 3840, false), m(3840, 2160, true)];
        assert_eq!(game_monitor(&monitors), Some((1, m(3840, 2160, true))));
        assert_eq!(primary(&monitors), Some(m(3840, 2160, true)));
    }

    #[test]
    fn portrait_primary_selects_the_landscape_monitor() {
        // Measured: Wine lists the rotated DP-2 first, as primary.
        let monitors = [m(2160, 3840, true), m(3840, 2160, false)];
        assert_eq!(game_monitor(&monitors), Some((1, m(3840, 2160, false))));
    }

    #[test]
    fn largest_landscape_wins_and_ties_keep_the_first() {
        let monitors = [
            m(1080, 1920, true),
            m(1920, 1080, false),
            m(2560, 1440, false),
            m(2560, 1440, false),
        ];
        assert_eq!(game_monitor(&monitors).map(|(i, _)| i), Some(2));
    }

    #[test]
    fn only_portrait_or_nothing_falls_back() {
        assert_eq!(
            game_monitor(&[m(1080, 1920, true)]),
            Some((0, m(1080, 1920, true)))
        );
        assert_eq!(game_monitor(&[]), None);
        assert_eq!(primary(&[]), None);
        // No flagged primary: the first monitor stands in.
        assert_eq!(primary(&[m(800, 600, false)]), Some(m(800, 600, false)));
    }

    #[test]
    fn landscape_sizes_and_the_steam_deck() {
        assert_eq!(landscape((800, 1280)), Some((1280, 800)));
        assert_eq!(landscape((1365, 767)), Some((1364, 766)));
        assert_eq!(landscape((640, 480)), Some((640, 480)));
        assert_eq!(landscape((320, 240)), None);
        assert!(is_steam_deck(Some((800, 1280))));
        assert!(!is_steam_deck(Some((1920, 1080))));
        assert!(!is_steam_deck(None));
    }

    #[test]
    fn largest_16_9_fits_the_screen() {
        let fit = |s| largest_16_9(s, (3840, 2160), (1920, 1080));
        for (screen, expected) in [
            (Some((3840, 2160)), (3840, 2160)),
            (Some((7680, 4320)), (3840, 2160)),
            (Some((1920, 1080)), (1920, 1080)),
            (Some((2560, 1080)), (1920, 1080)),
            (Some((1280, 800)), (1280, 720)),
            (Some((2160, 3840)), (3840, 2160)),
            (Some((1366, 768)), (1360, 765)),
            (Some((640, 480)), (640, 360)),
            (Some((320, 200)), (640, 360)),
            (None, (1920, 1080)),
        ] {
            assert_eq!(fit(screen), expected, "{screen:?}");
        }
        assert!(is_16_9((1360, 765)));
        assert!(!is_16_9((1280, 800)));
        assert!(!is_16_9((0, 0)));
    }
}
