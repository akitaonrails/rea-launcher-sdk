//! Colour roles, fonts and the egui style.
//!
//! The style and font set-up is the one all five launchers share (CMR4's
//! copy). The palette roles come from CMR4 (documented WCAG contrast) and
//! OR, which split the accent in two: an *action* colour (Play, primary
//! buttons, the tab underline, progress) that is never used as text on the
//! base, and a *select* colour for whatever is chosen. A launcher with one
//! accent sets both roles to it. SCR adds the disabled roles.
//!
//! [`Palette::contrast_problems`] turns the documented contrast into a test
//! each launcher can keep: `assert!(PALETTE.contrast_problems().is_empty())`.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Id, Stroke, vec2,
};

/// The colours of one launcher, by role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The page background.
    pub base: Color32,
    /// Cards, text fields, a selected card's fill.
    pub panel: Color32,
    /// Borders, dividers, hover fills.
    pub line: Color32,
    /// Main text.
    pub ink: Color32,
    /// Secondary text: notes, headings, unselected tabs.
    pub muted: Color32,
    /// Play, primary buttons, tab underline, progress, the badge.
    pub action: Color32,
    pub action_hover: Color32,
    /// Text on `action`.
    pub on_action: Color32,
    /// Chosen chips, switches that are on, the selected card.
    pub select: Color32,
    /// Text on `select`.
    pub on_select: Color32,
    /// Error text (kept apart from the accents' hue).
    pub error: Color32,
    /// Text and outlines of controls that cannot be changed now.
    pub disabled_ink: Color32,
    pub disabled_line: Color32,
    /// The keyboard/pad focus ring.
    pub focus: Color32,
}

impl Default for Palette {
    /// A neutral grey stand-in, used only when no palette was installed.
    fn default() -> Palette {
        let grey = Color32::from_gray;
        Palette {
            base: grey(0x16),
            panel: grey(0x24),
            line: grey(0x3c),
            ink: grey(0xf2),
            muted: grey(0xa8),
            action: grey(0xe0),
            action_hover: grey(0xff),
            on_action: grey(0x16),
            select: grey(0xe0),
            on_select: grey(0x16),
            error: Color32::from_rgb(0xff, 0x95, 0x80),
            disabled_ink: grey(0x5a),
            disabled_line: grey(0x2c),
            focus: grey(0xf2),
        }
    }
}

/// WCAG 2 contrast ratio of two opaque colours (1.0 to 21.0).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn luminance(color: Color32) -> f32 {
        let channel = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.039_28 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

impl Palette {
    /// The role pairs below WCAG AA: 4.5:1 for body text, 3:1 for large
    /// text (card titles in `select`) and for the focus ring. Disabled
    /// roles are exempt, as in WCAG.
    pub fn contrast_problems(&self) -> Vec<String> {
        let checks = [
            ("ink on base", self.ink, self.base, 4.5),
            ("ink on panel", self.ink, self.panel, 4.5),
            ("muted on base", self.muted, self.base, 4.5),
            ("muted on panel", self.muted, self.panel, 4.5),
            ("on_action on action", self.on_action, self.action, 4.5),
            (
                "on_action on action_hover",
                self.on_action,
                self.action_hover,
                4.5,
            ),
            ("on_select on select", self.on_select, self.select, 4.5),
            ("select on panel", self.select, self.panel, 3.0),
            ("error on base", self.error, self.base, 4.5),
            ("focus on base", self.focus, self.base, 3.0),
        ];
        checks
            .into_iter()
            .filter_map(|(name, fg, bg, least)| {
                let ratio = contrast(fg, bg);
                (ratio < least).then(|| format!("{name}: {ratio:.2}:1, needs {least}:1"))
            })
            .collect()
    }
}

/// Font family names of the bundled Barlow weights (regular is the
/// default proportional font).
pub const SEMIBOLD: &str = "barlow-semibold";
pub const CONDENSED: &str = "condensed";
pub const DECAL: &str = "decal";

/// Barlow Regular, the body font.
pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Barlow SemiBold: headings, chosen chips, buttons.
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

/// Barlow Condensed SemiBold: tabs, card titles.
pub fn condensed(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(CONDENSED.into()))
}

/// Barlow Condensed Black Italic: the title and the Play plate, like a door
/// decal.
pub fn decal(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DECAL.into()))
}

/// The Barlow fonts (SIL OFL 1.1, `assets/fonts/OFL.txt`; ship that file
/// with the launcher).
pub const OFL: &str = include_str!("../assets/fonts/OFL.txt");

/// Barlow everywhere, plus the named weights. egui's own fonts are left
/// out to keep the launcher small.
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let font = |bytes: &'static [u8]| Arc::new(FontData::from_static(bytes));
    for (name, bytes) in [
        (
            "barlow",
            &include_bytes!("../assets/fonts/Barlow-Regular.ttf")[..],
        ),
        (
            SEMIBOLD,
            &include_bytes!("../assets/fonts/Barlow-SemiBold.ttf")[..],
        ),
        (
            CONDENSED,
            &include_bytes!("../assets/fonts/BarlowCondensed-SemiBold.ttf")[..],
        ),
        (
            DECAL,
            &include_bytes!("../assets/fonts/BarlowCondensed-BlackItalic.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(name.into(), font(bytes));
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.insert(family, vec!["barlow".into()]);
    }
    for name in [SEMIBOLD, CONDENSED, DECAL] {
        fonts.families.insert(
            FontFamily::Name(name.into()),
            vec![name.into(), "barlow".into()],
        );
    }
    fonts
}

fn palette_id() -> Id {
    Id::unique("rea-launcher-ui palette")
}

/// Installs the fonts, a dark style in `palette`'s colours (whatever the
/// system theme), and the palette itself for the widgets to read. Call
/// once, from eframe's creation closure.
pub fn install(ctx: &egui::Context, palette: &Palette) {
    ctx.set_fonts(fonts());
    set_palette(ctx, palette);
}

/// Restyles and stores `palette` without touching fonts (a palette that
/// follows the chosen game, as in SCR).
pub fn set_palette(ctx: &egui::Context, palette: &Palette) {
    ctx.data_mut(|data| data.insert_temp(palette_id(), *palette));
    ctx.set_theme(egui::Theme::Dark);
    let p = *palette;
    ctx.all_styles_mut(move |style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = p.base;
        v.extreme_bg_color = p.panel;
        v.text_edit_bg_color = Some(p.panel);
        v.override_text_color = Some(p.ink);
        v.selection.bg_fill = p.select.gamma_multiply(0.5);
        v.selection.stroke = Stroke::new(1.0, p.select);
        v.text_cursor.stroke = Stroke::new(2.0, p.select);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            widget.bg_stroke = Stroke::new(1.0, p.line);
            widget.corner_radius = CornerRadius::same(6);
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.muted);
        v.widgets.active.bg_stroke = Stroke::new(1.0, p.action);
        style.spacing.item_spacing = vec2(8.0, 8.0);
    });
}

/// The installed palette ([`Palette::default`] before [`install`]).
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|data| data.get_temp(palette_id()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// CMR4's palette, one accent in both roles.
    const CMR4: Palette = Palette {
        base: rgb(0x15171b),
        panel: rgb(0x22252b),
        line: rgb(0x3a3f48),
        ink: rgb(0xf4f5f7),
        muted: rgb(0xa4aab4),
        action: rgb(0xff5a1f),
        action_hover: rgb(0xff7a45),
        on_action: rgb(0x15171b),
        select: rgb(0xff5a1f),
        on_select: rgb(0x15171b),
        error: rgb(0xffd04a),
        disabled_ink: rgb(0x4f5560),
        disabled_line: rgb(0x2a2d33),
        focus: rgb(0xf4f5f7),
    };

    /// OR's palette: red for action, yellow for selection.
    const OR: Palette = Palette {
        base: rgb(0x0f0c0b),
        panel: rgb(0x211b18),
        line: rgb(0x3d3430),
        ink: rgb(0xf6f1ea),
        muted: rgb(0xab9f96),
        action: rgb(0xd40000),
        action_hover: rgb(0xe8160e),
        on_action: Color32::WHITE,
        select: rgb(0xffd400),
        on_select: rgb(0x1a1300),
        error: rgb(0xff9580),
        disabled_ink: rgb(0x5a504a),
        disabled_line: rgb(0x2a2320),
        focus: rgb(0xf6f1ea),
    };

    #[test]
    fn contrast_matches_the_documented_ratios() {
        // CMR4's notes: INK on GRAPHITE 16.5:1, ON_ACCENT on ACCENT 5.8:1.
        assert!((contrast(CMR4.ink, CMR4.base) - 16.5).abs() < 0.1);
        assert!((contrast(CMR4.on_action, CMR4.action) - 5.8).abs() < 0.1);
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert_eq!(contrast(CMR4.ink, CMR4.ink), 1.0);
    }

    #[test]
    fn the_launcher_palettes_pass() {
        assert_eq!(CMR4.contrast_problems(), Vec::<String>::new());
        assert_eq!(OR.contrast_problems(), Vec::<String>::new());
        assert_eq!(Palette::default().contrast_problems(), Vec::<String>::new());
    }

    #[test]
    fn red_as_text_is_reported() {
        // OR's rule: red is never text on the base (3.5:1)...
        let bad = Palette {
            error: OR.action,
            ..OR
        };
        let problems = bad.contrast_problems();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("error on base"), "{problems:?}");
        // ...but it is enough for the (non-text) focus ring.
        let ring = Palette {
            focus: OR.action,
            ..OR
        };
        assert!(ring.contrast_problems().is_empty());
    }

    #[test]
    fn install_stores_the_palette_and_fonts_resolve() {
        let ctx = egui::Context::default();
        assert_eq!(palette(&ctx), Palette::default());
        install(&ctx, &OR);
        assert_eq!(palette(&ctx), OR);
        let fonts = fonts();
        for name in [SEMIBOLD, CONDENSED, DECAL] {
            assert!(fonts.families.contains_key(&FontFamily::Name(name.into())));
        }
        assert!(OFL.contains("SIL OPEN FONT LICENSE"));
    }
}
