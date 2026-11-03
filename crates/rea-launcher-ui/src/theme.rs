//! Colour roles, fonts and the egui style.
//!
//! The style and font set-up is the one all five launchers share (CMR4's
//! copy). The palette roles come from CMR4 (documented WCAG contrast) and
//! OR, which split the accent in two: an *action* colour (Play, primary
//! buttons, the tab underline, progress) that is never used as text on the
//! base, and a *select* colour for whatever is chosen. A launcher with one
//! accent sets both roles to it. SCR adds the disabled roles.
//!
//! [`Metrics`] holds the sizes and the few style choices where the
//! launchers' looks differ (chip padding, card typography, button text
//! colour, label wrap); each launcher installs its own next to its palette.
//!
//! [`contrast_problems`] turns the documented contrast into a test each
//! launcher can keep: `assert!(theme::contrast_problems(&PALETTE,
//! &METRICS).is_empty())`. It checks every text/background pair the
//! widgets draw.

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
    /// Control borders and dividers: 3:1 against `base` and `panel`
    /// (WCAG non-text contrast). Never a fill under text.
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
    /// The keyboard/pad focus ring, and focused row labels
    /// ([`crate::widgets::field_focused`]).
    pub focus: Color32,
    /// Behind selected text in text fields; translucent (see [`tint`]).
    /// CMR3/CMR4/SRR/SCR: `tint(select, 128)`; OR: `tint(red, 153)`.
    pub selection: Color32,
}

/// `color` at `alpha` (0–255), premultiplied, as `Color32::gamma_multiply`
/// gives — but usable in a `const` palette.
pub const fn tint(color: Color32, alpha: u8) -> Color32 {
    const fn scale(c: u8, alpha: u8) -> u8 {
        ((c as u16 * alpha as u16 + 127) / 255) as u8
    }
    Color32::from_rgba_premultiplied(
        scale(color.r(), alpha),
        scale(color.g(), alpha),
        scale(color.b(), alpha),
        scale(color.a(), alpha),
    )
}

impl Default for Palette {
    /// A neutral grey stand-in, used only when no palette was installed.
    fn default() -> Palette {
        let grey = Color32::from_gray;
        Palette {
            base: grey(0x16),
            panel: grey(0x24),
            line: grey(0x6d),
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
            selection: tint(grey(0xe0), 128),
        }
    }
}

/// What colour the text of outline buttons (and folder fields' Browse) is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonText {
    /// The `action` colour, like the outline (CMR3, CMR4, SRR, SCR).
    Action,
    /// `ink`, for an action colour too dark to be text (OR's red).
    Ink,
}

/// Look-card and game-card typography and spacing, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardMetrics {
    /// Inner padding.
    pub pad: f32,
    /// Title size (condensed); a [`crate::widgets::LookCard`] can override it.
    pub title: f32,
    /// Badge text size (semibold).
    pub badge: f32,
    /// Horizontal padding inside the badge, each side.
    pub badge_pad: f32,
    /// How far the badge rises above the card's top edge.
    pub badge_rise: f32,
    /// Note (look card) and subtitle (game card) size.
    pub note: f32,
    /// Note offset below the padding.
    pub note_top: f32,
}

/// Sizes and style choices of the widgets. Defaults are the SDK's own;
/// each launcher's values are listed per field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Horizontal padding added to a chip's label: 24 (SCR 22).
    pub chip_pad: f32,
    /// Gap between chips: 6.
    pub chip_gap: f32,
    /// [`crate::widgets::ChipSize::Compact`] padding and gap: 14 and 5 (SCR).
    pub compact_chip_pad: f32,
    pub compact_chip_gap: f32,
    /// Outline-button text colour: `Action` (OR: `Ink`).
    pub button_text: ButtonText,
    /// Width of a [`crate::widgets::field`] label: 118 (SCR 128).
    pub field_width: f32,
    /// Whether toggle labels wrap to the width left: true (CMR3); the
    /// others drew them on one line.
    pub toggle_wrap: bool,
    pub card: CardMetrics,
}

impl Metrics {
    /// The SDK's defaults (CMR4's card).
    pub const DEFAULT: Metrics = Metrics {
        chip_pad: 24.0,
        chip_gap: 6.0,
        compact_chip_pad: 14.0,
        compact_chip_gap: 5.0,
        button_text: ButtonText::Action,
        field_width: 118.0,
        toggle_wrap: true,
        card: CardMetrics {
            pad: 12.0,
            title: 22.0,
            badge: 12.0,
            badge_pad: 6.0,
            badge_rise: 9.0,
            note: 13.0,
            note_top: 30.0,
        },
    };
}

impl Default for Metrics {
    fn default() -> Metrics {
        Metrics::DEFAULT
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

/// One colour pair a widget draws and the contrast it needs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pair {
    pub name: &'static str,
    pub fg: Color32,
    pub bg: Color32,
    pub least: f32,
}

impl Palette {
    /// Whether `base` is light (dark text on a light page, as CMR2005's
    /// white menu band): black contrasts with it more than white does.
    /// [`visuals`] then starts from egui's light style.
    pub fn is_light(&self) -> bool {
        contrast(self.base, Color32::BLACK) > contrast(self.base, Color32::WHITE)
    }

    /// Every foreground/background pair the widgets draw with this palette
    /// (whatever the metrics). WCAG AA: 4.5:1 for body text, 3:1 for large
    /// text (card titles) and for non-text marks (focus ring, Play plate,
    /// selected chip and switch). Disabled text is exempt from AA in WCAG,
    /// but must stay visible: 2:1.
    pub fn pairs(&self) -> Vec<Pair> {
        let pair = |name, fg, bg, least| Pair {
            name,
            fg,
            bg,
            least,
        };
        let mut pairs = vec![
            pair("ink on base", self.ink, self.base, 4.5),
            pair(
                "ink on panel (fields, selected card, hovered chip or button)",
                self.ink,
                self.panel,
                4.5,
            ),
            pair("muted on base", self.muted, self.base, 4.5),
            pair(
                "muted on panel (card note, field hint, Play not ready)",
                self.muted,
                self.panel,
                4.5,
            ),
            pair(
                "line on base (control borders, non-text)",
                self.line,
                self.base,
                3.0,
            ),
            pair(
                "line on panel (control borders, non-text)",
                self.line,
                self.panel,
                3.0,
            ),
            pair("on_action on action", self.on_action, self.action, 4.5),
            pair(
                "on_action on action_hover",
                self.on_action,
                self.action_hover,
                4.5,
            ),
            pair("on_select on select", self.on_select, self.select, 4.5),
            pair(
                "select on panel (selected card title)",
                self.select,
                self.panel,
                3.0,
            ),
            pair(
                "select on base (installed dot)",
                self.select,
                self.base,
                3.0,
            ),
            pair(
                "action on base (Play, tab underline)",
                self.action,
                self.base,
                3.0,
            ),
            pair("error on base", self.error, self.base, 4.5),
            pair(
                "focus on base (ring, focused label)",
                self.focus,
                self.base,
                4.5,
            ),
            pair("focus on panel (ring)", self.focus, self.panel, 3.0),
            pair("disabled_ink on base", self.disabled_ink, self.base, 2.0),
        ];
        // egui's own widgets (sliders, number boxes, combo boxes,
        // tooltips) in every state, with the fills `visuals` gives them.
        pairs.extend(
            egui_fills(self)
                .into_iter()
                .map(|(name, fill)| pair(name, self.ink, fill, 4.5)),
        );
        pairs.push(pair(
            "hyperlink on base",
            visuals(self).hyperlink_color,
            self.base,
            4.5,
        ));
        pairs
    }

    /// The [`Palette::pairs`] below their contrast; see also
    /// [`contrast_problems`], which adds the metric-dependent pairs.
    pub fn contrast_problems(&self) -> Vec<String> {
        problems(self.pairs())
    }
}

/// Every pair the widgets draw with `palette` and `metrics` below its
/// contrast (with [`ButtonText::Action`], also button text in `action` on
/// the base). Empty means the look passes.
pub fn contrast_problems(palette: &Palette, metrics: &Metrics) -> Vec<String> {
    let mut pairs = palette.pairs();
    if metrics.button_text == ButtonText::Action {
        // A hovered button turns its text to ink on the `panel` fill
        // (covered by "ink on panel"): CMR4's orange on its old `line`
        // hover fill was 3.4:1.
        pairs.push(Pair {
            name: "action on base (button text)",
            fg: palette.action,
            bg: palette.base,
            least: 4.5,
        });
    }
    problems(pairs)
}

fn problems(pairs: Vec<Pair>) -> Vec<String> {
    pairs
        .into_iter()
        .filter_map(|pair| {
            let ratio = contrast(pair.fg, pair.bg);
            (ratio < pair.least)
                .then(|| format!("{}: {ratio:.2}:1, needs {}:1", pair.name, pair.least))
        })
        .collect()
}

/// The text sizes the widgets draw, in points. Body text (labels, notes,
/// chips, buttons) is at least [`MIN_BODY`]; nothing is under
/// [`MIN_TEXT`] (card badges and small card notes are the smallest).
pub mod size {
    /// Smallest text anywhere.
    pub const MIN_TEXT: f32 = 11.0;
    /// Smallest body text.
    pub const MIN_BODY: f32 = 13.0;
    /// Chip labels.
    pub const CHIP: f32 = 14.0;
    /// Toggle, field and text-field text.
    pub const LABEL: f32 = 15.0;
    /// Section headings (semibold).
    pub const HEADING: f32 = 14.0;
    /// Notes, error lines, the footer hint, status lines.
    pub const NOTE: f32 = 13.0;
    /// Outline and primary buttons (semibold).
    pub const BUTTON: f32 = 15.0;
    /// Tab names (condensed).
    pub const TAB: f32 = 22.0;
    /// The Play plate (decal).
    pub const PLAY: f32 = 30.0;
    /// The smallest title `widgets::fit_title` shrinks to.
    pub const FIT_TITLE_MIN: f32 = 12.0;

    /// Every fixed body size, by use.
    pub const BODY: [(&str, f32); 5] = [
        ("chip", CHIP),
        ("label", LABEL),
        ("heading", HEADING),
        ("note", NOTE),
        ("button", BUTTON),
    ];
}

pub use size::{MIN_BODY, MIN_TEXT};

impl Metrics {
    /// Every text size the widgets draw with these metrics: the fixed body
    /// sizes, tabs, Play, and the card sizes.
    pub fn text_sizes(&self) -> Vec<(&'static str, f32)> {
        let mut sizes = size::BODY.to_vec();
        sizes.extend([
            ("tab", size::TAB),
            ("play", size::PLAY),
            ("fitted card title (smallest)", size::FIT_TITLE_MIN),
            ("card title", self.card.title),
            ("card badge", self.card.badge),
            ("card note", self.card.note),
        ]);
        sizes
    }
}

/// Text sizes under [`MIN_TEXT`] with these metrics, and fixed body sizes
/// under [`MIN_BODY`]. Empty means the sizes pass.
pub fn text_size_problems(metrics: &Metrics) -> Vec<String> {
    let mut problems: Vec<String> = metrics
        .text_sizes()
        .into_iter()
        .filter(|&(_, pt)| pt < MIN_TEXT)
        .map(|(name, pt)| format!("{name}: {pt} pt, needs {MIN_TEXT} pt"))
        .collect();
    problems.extend(
        size::BODY
            .iter()
            .filter(|&&(_, pt)| pt < MIN_BODY)
            .map(|(name, pt)| format!("{name}: {pt} pt body text, needs {MIN_BODY} pt")),
    );
    problems
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

fn metrics_id() -> Id {
    Id::unique("rea-launcher-ui metrics")
}

/// Installs the fonts, a style in `palette`'s colours ([`visuals`]: dark
/// or light by the palette's base, whatever the system theme), and the
/// palette itself for the widgets to read, with the default [`Metrics`]. Call once, from eframe's creation closure.
pub fn install(ctx: &egui::Context, palette: &Palette) {
    install_with(ctx, palette, &Metrics::DEFAULT);
}

/// [`install`] with the launcher's own [`Metrics`].
pub fn install_with(ctx: &egui::Context, palette: &Palette, metrics: &Metrics) {
    ctx.set_fonts(fonts());
    set_palette(ctx, palette);
    set_metrics(ctx, metrics);
}

/// Stores `metrics` for the widgets.
pub fn set_metrics(ctx: &egui::Context, metrics: &Metrics) {
    ctx.data_mut(|data| data.insert_temp(metrics_id(), *metrics));
}

/// The installed metrics ([`Metrics::DEFAULT`] before any were set).
pub fn metrics(ctx: &egui::Context) -> Metrics {
    ctx.data(|data| data.get_temp(metrics_id()))
        .unwrap_or_default()
}

/// Restyles and stores `palette` without touching fonts (a palette that
/// follows the chosen game, as in SCR). The egui style is [`visuals`].
pub fn set_palette(ctx: &egui::Context, palette: &Palette) {
    ctx.data_mut(|data| data.insert_temp(palette_id(), *palette));
    ctx.set_theme(if palette.is_light() {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    });
    let visuals = visuals(palette);
    ctx.all_styles_mut(move |style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = vec2(8.0, 8.0);
    });
}

/// How much of `ink` a light palette's hovered (and open) egui widget
/// mixes into `panel`; pressed widgets mix twice as much.
const LIGHT_HOVER_INK: f32 = 0.06;

/// `a` moved `t` (0–1) of the way to `b`, per channel (opaque colours).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// The egui style [`set_palette`] installs for `palette`.
///
/// A dark base (see [`Palette::is_light`]) starts from egui's dark visuals
/// and keeps their grey widget fills (the five dark launchers' look). A
/// light base starts from egui's light visuals and fills egui's own widgets
/// (sliders, number boxes, egui buttons, combo boxes, tooltips) from the
/// palette: `panel` at rest, `panel` tinted towards `ink` when hovered or
/// pressed, `ink` strokes and links. Before v0.4.0 every palette got the
/// dark fills, so a light palette's dark text sat on dark grey (1.5:1);
/// [`Palette::pairs`] now checks `ink` on every fill this returns.
pub fn visuals(palette: &Palette) -> egui::Visuals {
    let p = *palette;
    let mut v = if p.is_light() {
        let mut v = egui::Visuals::light();
        let hover = mix(p.panel, p.ink, LIGHT_HOVER_INK);
        let pressed = mix(p.panel, p.ink, 2.0 * LIGHT_HOVER_INK);
        let w = &mut v.widgets;
        for (widget, fill) in [
            (&mut w.noninteractive, p.panel),
            (&mut w.inactive, p.panel),
            (&mut w.hovered, hover),
            (&mut w.active, pressed),
            (&mut w.open, hover),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.fg_stroke.color = p.ink;
        }
        v.faint_bg_color = p.panel;
        v.window_fill = p.base;
        v.window_stroke = Stroke::new(1.0, p.line);
        v.hyperlink_color = p.ink;
        v.error_fg_color = p.error;
        v
    } else {
        egui::Visuals::dark()
    };
    v.panel_fill = p.base;
    v.extreme_bg_color = p.panel;
    v.text_edit_bg_color = Some(p.panel);
    v.override_text_color = Some(p.ink);
    v.selection.bg_fill = p.selection;
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
    v
}

/// Every fill egui's own widgets draw text on with `palette`'s
/// [`visuals`], named by state (equal fills listed once).
fn egui_fills(palette: &Palette) -> Vec<(&'static str, Color32)> {
    let v = visuals(palette);
    let w = &v.widgets;
    let mut fills: Vec<(&'static str, Color32)> = Vec::new();
    for (name, fill) in [
        ("ink on an egui widget at rest", w.inactive.weak_bg_fill),
        ("ink on an egui widget at rest", w.inactive.bg_fill),
        ("ink on a hovered egui widget", w.hovered.weak_bg_fill),
        ("ink on a hovered egui widget", w.hovered.bg_fill),
        ("ink on a pressed egui widget", w.active.weak_bg_fill),
        ("ink on a pressed egui widget", w.active.bg_fill),
        (
            "ink on an open egui widget (combo box)",
            w.open.weak_bg_fill,
        ),
        ("ink on an open egui widget (combo box)", w.open.bg_fill),
        ("ink on egui's frames", w.noninteractive.bg_fill),
        ("ink on egui's frames", w.noninteractive.weak_bg_fill),
        ("ink on a tooltip or popup", v.window_fill),
    ] {
        if !fills.iter().any(|&(_, seen)| seen == fill) {
            fills.push((name, fill));
        }
    }
    fills
}

/// The installed palette ([`Palette::default`] before [`install`]).
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|data| data.get_temp(palette_id()))
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    /// CMR4's palette, one accent in both roles.
    pub(crate) const CMR4: Palette = Palette {
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
        selection: tint(rgb(0xff5a1f), 128),
    };

    /// OR's palette: red for action, yellow for selection.
    pub(crate) const OR: Palette = Palette {
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
        selection: tint(rgb(0xd40000), 153),
    };

    const CMR3: Palette = Palette {
        base: rgb(0x54101c),
        panel: rgb(0x69182a),
        line: rgb(0x8a2a42),
        ink: rgb(0xf7f0f2),
        muted: rgb(0xd6aab6),
        action: rgb(0xf5c400),
        action_hover: rgb(0xffd32a),
        on_action: rgb(0x2b0d18),
        select: rgb(0xf5c400),
        on_select: rgb(0x2b0d18),
        error: rgb(0xff8a80),
        disabled_ink: rgb(0x9a6272),
        disabled_line: rgb(0x6a2234),
        focus: rgb(0xf7f0f2),
        selection: tint(rgb(0xf5c400), 128),
    };

    const SCR: Palette = Palette {
        base: rgb(0x0e172a),
        panel: rgb(0x162442),
        line: rgb(0x283e6b),
        ink: rgb(0xeef2fb),
        muted: rgb(0x9fb0d4),
        action: rgb(0xf5c400),
        action_hover: rgb(0xffd32a),
        on_action: rgb(0x0e172a),
        select: rgb(0xf5c400),
        on_select: rgb(0x0e172a),
        error: rgb(0xff8a80),
        disabled_ink: rgb(0x4f6085),
        disabled_line: rgb(0x1b2947),
        focus: rgb(0xf5c400),
        selection: tint(rgb(0xf5c400), 128),
    };

    const SRR: Palette = Palette {
        base: rgb(0x0a1b33),
        panel: rgb(0x102a4d),
        line: rgb(0x1f4a84),
        ink: rgb(0xedf3fc),
        muted: rgb(0x9fb5d8),
        action: rgb(0xff7a00),
        action_hover: rgb(0xffad33),
        on_action: rgb(0x0a1b33),
        select: rgb(0xff7a00),
        on_select: rgb(0x0a1b33),
        error: rgb(0xff8a80),
        disabled_ink: rgb(0x9fb5d8),
        disabled_line: Color32::from_rgba_premultiplied(16, 37, 66, 128),
        focus: rgb(0xedf3fc),
        selection: tint(rgb(0xff7a00), 128),
    };

    /// CMR2005's light palette (white menu band, warm-black ink).
    pub(crate) const CMR2005: Palette = Palette {
        base: rgb(0xfbfaf8),
        panel: rgb(0xece7e2),
        line: rgb(0x8a7c72),
        ink: rgb(0x1c1612),
        muted: rgb(0x66574d),
        action: rgb(0xc4122e),
        action_hover: rgb(0xa30d24),
        on_action: Color32::WHITE,
        select: rgb(0x3d2f26),
        on_select: Color32::WHITE,
        error: rgb(0x9e1b14),
        disabled_ink: rgb(0x8f847c),
        disabled_line: rgb(0xd6cfc8),
        focus: rgb(0x3d2f26),
        selection: tint(rgb(0xc4122e), 64),
    };

    /// Each launcher's metrics, as its local widgets drew (see the README).
    pub(crate) const CMR3_METRICS: Metrics = Metrics {
        card: CardMetrics {
            pad: 14.0,
            note: 13.5,
            note_top: 32.0,
            ..Metrics::DEFAULT.card
        },
        ..Metrics::DEFAULT
    };
    const CMR4_METRICS: Metrics = Metrics {
        toggle_wrap: false,
        ..Metrics::DEFAULT
    };
    pub(crate) const SCR_METRICS: Metrics = Metrics {
        chip_pad: 22.0,
        field_width: 128.0,
        toggle_wrap: false,
        card: CardMetrics {
            pad: 12.0,
            title: 20.0,
            badge: 11.5,
            badge_pad: 5.0,
            badge_rise: 8.0,
            note: 12.5,
            note_top: 28.0,
        },
        ..Metrics::DEFAULT
    };
    const SRR_METRICS: Metrics = Metrics {
        toggle_wrap: false,
        ..CMR3_METRICS
    };
    pub(crate) const OR_METRICS: Metrics = Metrics {
        button_text: ButtonText::Ink,
        toggle_wrap: false,
        ..CMR3_METRICS
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
    fn the_five_launcher_looks_pass_but_their_borders() {
        // The 3:1 border check is new in 0.2.0: every launcher's `line` is
        // too dark; the second colour is the least lightening (towards
        // white) that passes against both base and panel (README).
        for (name, palette, metrics, line) in [
            ("CMR3", CMR3, CMR3_METRICS, rgb(0xaf6d7d)),
            ("CMR4", CMR4, CMR4_METRICS, rgb(0x6b6e75)),
            ("SCR", SCR, SCR_METRICS, rgb(0x5d6e90)),
            ("SRR", SRR, SRR_METRICS, rgb(0x5475a1)),
            ("OR", OR, OR_METRICS, rgb(0x6d6663)),
        ] {
            let problems = contrast_problems(&palette, &metrics);
            assert!(
                !problems.is_empty() && problems.iter().all(|p| p.starts_with("line on ")),
                "{name}: {problems:?}"
            );
            let fixed = Palette { line, ..palette };
            assert_eq!(
                contrast_problems(&fixed, &metrics),
                Vec::<String>::new(),
                "{name} with the lighter line"
            );
        }
        assert_eq!(Palette::default().contrast_problems(), Vec::<String>::new());
    }

    #[test]
    fn a_light_palette_gets_light_egui_widgets() {
        // v0.3.3 started every palette from egui's dark visuals: CMR2005's
        // warm-black ink sat on dark-grey sliders and buttons (~1.5:1), and
        // the contrast check could not see it.
        assert!(CMR2005.is_light());
        for dark in [CMR3, CMR4, SCR, SRR, OR, Palette::default()] {
            assert!(!dark.is_light(), "{dark:?}");
        }
        let ctx = egui::Context::default();
        install(&ctx, &CMR2005);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            let style = ctx.style_of(theme);
            let w = &style.visuals.widgets;
            assert!(!style.visuals.dark_mode);
            for (state, widget) in [
                ("rest", &w.inactive),
                ("hovered", &w.hovered),
                ("pressed", &w.active),
                ("open", &w.open),
                ("frame", &w.noninteractive),
            ] {
                for fill in [widget.bg_fill, widget.weak_bg_fill] {
                    let ratio = contrast(CMR2005.ink, fill);
                    assert!(ratio >= 4.5, "{state}: ink on {fill:?} is {ratio:.2}:1");
                }
                assert_eq!(widget.fg_stroke.color, CMR2005.ink, "{state}");
            }
            // Hover and press are visible against the resting fill.
            assert_ne!(w.hovered.bg_fill, w.inactive.bg_fill);
            assert_ne!(w.active.bg_fill, w.hovered.bg_fill);
        }
        assert_eq!(
            contrast_problems(&CMR2005, &Metrics::DEFAULT),
            Vec::<String>::new()
        );
    }

    #[test]
    fn the_check_sees_every_fill_egui_draws_text_on() {
        for palette in [CMR2005, CMR4, OR] {
            let pairs = palette.pairs();
            let v = visuals(&palette);
            let w = &v.widgets;
            for fill in [
                w.inactive.bg_fill,
                w.inactive.weak_bg_fill,
                w.hovered.bg_fill,
                w.hovered.weak_bg_fill,
                w.active.bg_fill,
                w.active.weak_bg_fill,
                w.open.bg_fill,
                w.open.weak_bg_fill,
                w.noninteractive.bg_fill,
                v.window_fill,
            ] {
                assert!(
                    pairs.iter().any(|p| p.fg == palette.ink && p.bg == fill),
                    "ink on {fill:?} is not checked"
                );
            }
        }
        // A light palette whose panel is too dark for its ink is caught on
        // the widget fills, not only on the cards.
        let murky = Palette {
            panel: rgb(0x6a625c),
            ..CMR2005
        };
        let problems = murky.contrast_problems();
        assert!(
            problems
                .iter()
                .any(|p| p.starts_with("ink on an egui widget at rest")),
            "{problems:?}"
        );
        // Dark palettes keep egui's dark fills (their look is unchanged).
        let dark = visuals(&CMR4);
        assert_eq!(
            dark.widgets.inactive.bg_fill,
            egui::Visuals::dark().widgets.inactive.bg_fill
        );
        assert!(dark.dark_mode);
    }

    #[test]
    fn red_as_text_is_reported() {
        // OR's rule: red is never text on the base (3.5:1)...
        let bad = Palette {
            error: OR.action,
            ..OR
        };
        let problems: Vec<String> = bad
            .contrast_problems()
            .into_iter()
            .filter(|p| !p.starts_with("line on "))
            .collect();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("error on base"), "{problems:?}");
        // ...so OR's buttons need ink text: action text fails on its base.
        let action_text = contrast_problems(&OR, &Metrics::DEFAULT);
        assert!(
            action_text
                .iter()
                .any(|p| p.starts_with("action on base (button text)")),
            "{action_text:?}"
        );
    }

    #[test]
    fn every_widget_pair_is_checked() {
        let names: Vec<&str> = CMR4.pairs().iter().map(|pair| pair.name).collect();
        for needle in [
            "line on base",
            "line on panel",
            "disabled_ink",
            "focus on panel",
            "on_action on action_hover",
            "on_select on select",
            "muted on panel",
        ] {
            assert!(
                names.iter().any(|n| n.contains(needle)),
                "{needle} not checked"
            );
        }
        // A focus ring that vanishes on the card fill is reported.
        let faint = Palette {
            focus: CMR4.panel,
            ..CMR4
        };
        assert!(
            faint
                .contrast_problems()
                .iter()
                .any(|p| p.starts_with("focus on panel"))
        );
    }

    #[test]
    fn no_text_is_too_small() {
        for (name, metrics) in [
            ("default", Metrics::DEFAULT),
            ("CMR3", CMR3_METRICS),
            ("CMR4", CMR4_METRICS),
            ("SCR", SCR_METRICS),
            ("SRR", SRR_METRICS),
            ("OR", OR_METRICS),
        ] {
            assert_eq!(text_size_problems(&metrics), Vec::<String>::new(), "{name}");
            assert!(metrics.text_sizes().iter().all(|&(_, pt)| pt >= MIN_TEXT));
        }
        assert!(size::BODY.iter().all(|&(_, pt)| pt >= MIN_BODY));
        let tiny = Metrics {
            card: CardMetrics {
                badge: 10.0,
                ..Metrics::DEFAULT.card
            },
            ..Metrics::DEFAULT
        };
        assert_eq!(
            text_size_problems(&tiny),
            ["card badge: 10 pt, needs 11 pt"]
        );
    }

    #[test]
    fn tint_matches_gamma_multiply() {
        for (color, alpha) in [
            (OR.action, 153u8),
            (CMR4.select, 128),
            (Color32::WHITE, 255),
        ] {
            let expected = color.gamma_multiply(f32::from(alpha) / 255.0);
            let got = tint(color, alpha);
            for (a, b) in got.to_array().into_iter().zip(expected.to_array()) {
                assert!(a.abs_diff(b) <= 1, "{got:?} vs {expected:?}");
            }
        }
    }

    #[test]
    fn install_stores_palette_metrics_and_fonts() {
        let ctx = egui::Context::default();
        assert_eq!(palette(&ctx), Palette::default());
        assert_eq!(metrics(&ctx), Metrics::DEFAULT);
        install(&ctx, &OR);
        assert_eq!(palette(&ctx), OR);
        assert_eq!(
            ctx.style_of(egui::Theme::Dark).visuals.selection.bg_fill,
            OR.selection
        );
        install_with(&ctx, &OR, &OR_METRICS);
        assert_eq!(metrics(&ctx), OR_METRICS);
        let fonts = fonts();
        for name in [SEMIBOLD, CONDENSED, DECAL] {
            assert!(fonts.families.contains_key(&FontFamily::Name(name.into())));
        }
        assert!(OFL.contains("SIL OPEN FONT LICENSE"));
    }
}
