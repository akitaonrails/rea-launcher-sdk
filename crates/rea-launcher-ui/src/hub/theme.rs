//! The hub's look: a launcher's colours, brand and type sizes
//! ([`HubTheme`]) and the sizes that differ between hubs ([`HubLayout`]).

use eframe::egui::{self, Color32, FontId};

use crate::theme::{self, Palette, contrast, tint};

/// Text sizes, in points, at the 720-line design height.
pub mod size {
    pub const TITLE: f32 = 58.0;
    pub const PAGE_TITLE: f32 = 36.0;
    pub const BRAND: f32 = 24.0;
    pub const NAV: f32 = 18.0;
    pub const BUTTON: f32 = 22.0;
    pub const BODY: f32 = 15.0;
    pub const LABEL: f32 = 17.0;
    pub const NOTE: f32 = 13.0;
    pub const SHIELD: f32 = 14.0;
    pub const BADGE: f32 = 11.0;
    pub const ALL: [f32; 10] = [
        TITLE, PAGE_TITLE, BRAND, NAV, BUTTON, BODY, LABEL, NOTE, SHIELD, BADGE,
    ];
}

/// The design canvas: everything is laid out for 720 lines and zoomed to
/// the window.
pub const DESIGN_HEIGHT: f32 = 720.0;

/// A hub launcher's colours and names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HubTheme {
    /// The page (asphalt at night).
    pub base: Color32,
    /// Cards, text fields, tiles without art.
    pub panel: Color32,
    /// Control borders and dividers (3:1 on base and panel).
    pub line: Color32,
    /// The accent: focus ring, Play, the active underline.
    pub lamp: Color32,
    pub lamp_hover: Color32,
    /// Sign plates: the top bar, card headings, shields, "Ready".
    pub sign: Color32,
    /// Text.
    pub ink: Color32,
    /// Secondary text.
    pub muted: Color32,
    /// The "Not supported yet" chip (ink on it).
    pub signal: Color32,
    /// Error text on the page.
    pub error: Color32,
    /// Text and outlines of controls that can't change now.
    pub disabled: Color32,
    /// Behind the focused settings row.
    pub focus_fill: Color32,
    /// How opaque the page is behind text over the hero picture.
    pub scrim: u8,
    /// The shield's text in the top bar: "ID".
    pub brand: &'static str,
    /// The window's title and the top bar's name: "Initial D Launcher"
    /// (never a game's title: STANDARDS "One instance").
    pub title: &'static str,
    /// The window's app id: "rea-initiald".
    pub app_id: &'static str,
}

impl HubTheme {
    /// The SDK palette of these colours (its contrast test, and the
    /// message window before the hub opens).
    pub fn palette(&self) -> Palette {
        Palette {
            base: self.base,
            panel: self.panel,
            line: self.line,
            ink: self.ink,
            muted: self.muted,
            action: self.lamp,
            action_hover: self.lamp_hover,
            on_action: self.base,
            select: self.lamp,
            on_select: self.base,
            error: self.error,
            disabled_ink: self.disabled,
            disabled_line: self.disabled,
            focus: self.lamp,
            selection: tint(self.lamp, 128),
        }
    }

    /// Every pair the hub draws that misses WCAG AA, as sentences: the
    /// SDK palette's pairs, sign plates, chips, and text over the hero
    /// picture (worst case: a white picture under the scrim). Empty when
    /// all is well; keep a test that asserts it.
    pub fn contrast_problems(&self) -> Vec<String> {
        let photo = over(alpha(self.base, self.scrim), Color32::WHITE);
        let checks = [
            ("ink on the sign plate", self.ink, self.sign, 4.5),
            ("focus ring on the sign plate", self.lamp, self.sign, 3.0),
            ("ink on the signal chip", self.ink, self.signal, 4.5),
            ("dark text on Play", self.base, self.lamp, 4.5),
            ("ink over the picture", self.ink, photo, 4.5),
            ("muted over the picture", self.muted, photo, 4.5),
            ("focus ring over the picture", self.lamp, photo, 3.0),
            ("error over the picture", self.error, photo, 4.5),
            ("muted on a card", self.muted, self.panel, 4.5),
            ("ink on the focused row", self.ink, self.focus_fill, 4.5),
            ("disabled on a card", self.disabled, self.panel, 2.0),
        ];
        let mut problems = self.palette().contrast_problems();
        problems.extend(
            checks
                .iter()
                .map(|(what, fg, bg, least)| (what, contrast(*fg, *bg), least))
                .filter(|(_, r, least)| r < *least)
                .map(|(what, r, least)| format!("{what}: {r:.2} < {least}")),
        );
        problems
    }
}

/// The sizes that differ between hubs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HubLayout {
    /// A tile's widest: `None` spreads the tiles over the whole width
    /// (eleven games), `Some(208.0)` keeps three or four games to the left.
    pub tile_max_width: Option<f32>,
    /// A settings row's height: 58, or 50 so nine rows fit.
    pub row_height: f32,
    /// Where a row's label and its note start below its top.
    pub row_label_top: f32,
    pub row_note_top: f32,
}

impl HubLayout {
    /// Initial D's: tiles over the whole width, 58-point rows.
    pub const DEFAULT: HubLayout = HubLayout {
        tile_max_width: None,
        row_height: 58.0,
        row_label_top: 9.0,
        row_note_top: 32.0,
    };
    /// Sega's 50-point rows (nine rows fit the 720 lines).
    pub const COMPACT_ROWS: HubLayout = HubLayout {
        row_height: 50.0,
        row_label_top: 6.0,
        row_note_top: 27.0,
        ..HubLayout::DEFAULT
    };
}

impl Default for HubLayout {
    fn default() -> HubLayout {
        HubLayout::DEFAULT
    }
}

/// Barlow Condensed Bold: game titles, page headings.
pub fn display(size: f32) -> FontId {
    theme::condensed_bold(size)
}
/// Barlow Condensed SemiBold: buttons, shields, the brand.
pub fn display_semi(size: f32) -> FontId {
    theme::condensed(size)
}
/// Barlow SemiBold: labels, the top bar.
pub fn semibold(size: f32) -> FontId {
    theme::semibold(size)
}
/// Barlow Regular: body text.
pub fn body(size: f32) -> FontId {
    theme::body(size)
}

/// `c` at opacity `a` (0–255).
pub fn alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

/// `fill` laid over `under` (fill is premultiplied, as egui keeps it).
pub fn over(fill: Color32, under: Color32) -> Color32 {
    let a = f32::from(fill.a()) / 255.0;
    let mix = |f: u8, b: u8| (f32::from(f) + (1.0 - a) * f32::from(b)).round() as u8;
    Color32::from_rgb(
        mix(fill.r(), under.r()),
        mix(fill.g(), under.g()),
        mix(fill.b(), under.b()),
    )
}

/// The fonts, the SDK palette (for the SDK's widgets and messages) and
/// egui's own style in the hub's colours. Call once, from eframe's
/// creation closure (the test harness does it too).
pub fn install(ctx: &egui::Context, t: &HubTheme) {
    ctx.set_fonts(theme::fonts());
    theme::set_palette(ctx, &t.palette());
    let palette = t.palette();
    ctx.all_styles_mut(|s| {
        let v = &mut s.visuals;
        *v = egui::Visuals::dark();
        v.override_text_color = Some(t.ink);
        v.extreme_bg_color = t.panel;
        v.panel_fill = t.base;
        v.window_fill = t.panel;
        v.selection.bg_fill = palette.selection;
        v.selection.stroke = egui::Stroke::new(2.0, t.lamp);
        v.text_cursor.stroke = egui::Stroke::new(2.0, t.lamp);
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.5, t.line);
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.5, t.ink);
        v.widgets.active.bg_stroke = egui::Stroke::new(2.0, t.lamp);
        s.text_styles
            .insert(egui::TextStyle::Body, body(size::LABEL));
        s.text_styles
            .insert(egui::TextStyle::Monospace, body(size::LABEL));
        s.spacing.item_spacing = egui::vec2(0.0, 0.0);
        s.spacing.window_margin = egui::Margin::ZERO;
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// Initial D's (and Sega's) mountain pass at night.
    pub(crate) const MOUNTAIN: HubTheme = HubTheme {
        base: rgb(0x0B121A),
        panel: rgb(0x141E2A),
        line: rgb(0x6B7A8C),
        lamp: rgb(0xFFC857),
        lamp_hover: rgb(0xFFD784),
        sign: rgb(0x1D4F8F),
        ink: rgb(0xF4F7F2),
        muted: rgb(0x8A97A6),
        signal: rgb(0xB3261E),
        error: rgb(0xFF8A80),
        disabled: rgb(0x5E6B7A),
        focus_fill: rgb(0x1F2C3B),
        scrim: 240,
        brand: "ID",
        title: "Initial D Launcher",
        app_id: "rea-initiald",
    };

    /// Wangan's Shuto expressway at night.
    const EXPRESSWAY: HubTheme = HubTheme {
        base: rgb(0x0E1621),
        panel: rgb(0x16212E),
        lamp: rgb(0xF2A33A),
        lamp_hover: rgb(0xF7B85E),
        sign: rgb(0x0F6B4A),
        brand: "WM",
        title: "Wangan Launcher",
        app_id: "rea-wangan",
        ..MOUNTAIN
    };

    #[test]
    fn the_launchers_palettes_meet_wcag_aa() {
        for t in [MOUNTAIN, EXPRESSWAY] {
            let problems = t.contrast_problems();
            assert!(problems.is_empty(), "{}: {problems:#?}", t.title);
        }
    }

    #[test]
    fn a_bad_pair_is_named() {
        let t = HubTheme {
            sign: rgb(0x8A97A6),
            ..MOUNTAIN
        };
        let problems = t.contrast_problems();
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("ink on the sign plate")),
            "{problems:#?}"
        );
    }

    #[test]
    fn no_text_is_too_small() {
        for s in size::ALL {
            assert!(s >= crate::theme::size::MIN_TEXT, "{s}");
        }
        for s in [size::BODY, size::NOTE, size::LABEL] {
            assert!(s >= crate::theme::size::MIN_BODY, "{s}");
        }
    }

    #[test]
    fn the_scrim_is_the_page_colour_over_white() {
        assert_eq!(
            over(alpha(MOUNTAIN.base, 255), Color32::WHITE),
            MOUNTAIN.base
        );
        assert_eq!(
            over(alpha(MOUNTAIN.base, 0), Color32::WHITE),
            Color32::WHITE
        );
    }

    #[test]
    fn the_layouts_keep_each_launchers_rows() {
        assert_eq!(HubLayout::default().row_height, 58.0);
        assert_eq!(HubLayout::COMPACT_ROWS.row_height, 50.0);
        assert_eq!(HubLayout::COMPACT_ROWS.tile_max_width, None);
    }
}
