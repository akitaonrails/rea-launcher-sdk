//! The launchers' widgets, drawn in the installed [`Palette`].
//!
//! Where each one comes from:
//! - [`chips`]/[`chips_with`]: SCR's per-chip disabled reasons + SRR's
//!   "keep the saved value outlined while disabled", made one focus stop
//!   whose value ←/→ (the pad's left/right) step, as SCR's pad did.
//! - [`toggle`]: OR's (returns its `Response`) with CMR3's label wrap and
//!   SCR's disabled reason.
//! - [`look_card`]/[`fit_title`]: CMR4's focus ring and detail tooltip, OR's
//!   title fitting.
//! - [`primary_button`], [`progress_bar`]: OR. [`outline_button`],
//!   [`folder_field`], [`heading`], [`field`], [`tabs_bar`],
//!   [`play_plate`]/[`footer`], [`gradient`], [`stripes`]: identical in all
//!   five; the colours follow OR's split (action vs selection).
//! - [`decode_jpeg`]/[`hero_image`]: the portrait strip, CMR4 (left crop)
//!   and OR (centre crop).
//!
//! Every focusable widget draws a focus ring, so pad navigation is visible.

use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, EventFilter, Key, Mesh, Painter, Rect,
    Response, RichText, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2,
};

use crate::theme::{self, Palette, body, condensed, decal, semibold};

fn palette(ui: &Ui) -> Palette {
    theme::palette(ui.ctx())
}

/// Outlines `rect` in the focus colour, just outside it.
pub fn focus_ring(painter: &Painter, rect: Rect, color: Color32) {
    painter.rect_stroke(
        rect.expand(3.0),
        CornerRadius::same(8),
        Stroke::new(1.5, color),
        StrokeKind::Outside,
    );
}

// ---------------------------------------------------------------- chips

/// One choice in a chip row.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip<'a, T> {
    pub value: T,
    pub label: &'a str,
    /// Why it cannot be chosen now (shown on hover); `None` when it can.
    pub disabled: Option<&'a str>,
}

impl<'a, T> Chip<'a, T> {
    pub fn new(value: T, label: &'a str) -> Chip<'a, T> {
        Chip {
            value,
            label,
            disabled: None,
        }
    }

    pub fn disabled(mut self, reason: Option<&'a str>) -> Chip<'a, T> {
        self.disabled = reason;
        self
    }
}

/// How a chip is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipState {
    /// Chosen: filled with `select`.
    Selected,
    /// Can be chosen.
    Idle,
    /// The saved value, but it cannot change now: a dimmed `select`
    /// outline, so the player still sees what is saved (SRR).
    SavedDisabled,
    /// Cannot be chosen now.
    Disabled,
}

pub fn chip_state(selected: bool, enabled: bool) -> ChipState {
    match (selected, enabled) {
        (true, true) => ChipState::Selected,
        (false, true) => ChipState::Idle,
        (true, false) => ChipState::SavedDisabled,
        (false, false) => ChipState::Disabled,
    }
}

/// The enabled choice `by` steps from `current` (wrapping, skipping
/// disabled ones). Without a current choice, forward starts at the first
/// enabled one and backward at the last. `None` when none is enabled.
pub fn step_choice(enabled: &[bool], current: Option<usize>, by: isize) -> Option<usize> {
    let len = enabled.len() as isize;
    if len == 0 || !enabled.contains(&true) || by == 0 {
        return current.filter(|_| by == 0);
    }
    let direction = by.signum();
    let mut at = match current {
        Some(index) => index as isize,
        None if direction > 0 => -1,
        None => len,
    };
    let mut steps = by.abs();
    while steps > 0 {
        at = (at + direction).rem_euclid(len);
        if enabled[at as usize] {
            steps -= 1;
        }
    }
    Some(at as usize)
}

/// A row of choices, all with the same `disabled` reason (or none). See
/// [`chips_with`].
pub fn chips<T: PartialEq + Clone, S: AsRef<str>>(
    ui: &mut Ui,
    options: &[(T, S)],
    current: &mut T,
    disabled: Option<&str>,
) -> Response {
    let chips: Vec<Chip<'_, T>> = options
        .iter()
        .map(|(value, label)| Chip::new(value.clone(), label.as_ref()).disabled(disabled))
        .collect();
    chips_with(ui, &chips, current)
}

/// A row of choice chips. The row is one focus stop: ←/→ (or the pad's
/// left/right) step to the previous/next enabled choice and Space steps
/// forward, so a pad never has to visit every chip. Clicks pick a chip.
/// Disabled chips ignore clicks and show their reason on hover. The row's
/// response is `changed()` when the value changed.
pub fn chips_with<T: PartialEq + Clone>(
    ui: &mut Ui,
    options: &[Chip<'_, T>],
    current: &mut T,
) -> Response {
    let p = palette(ui);
    let id = ui.next_auto_id().with("rea-chips");
    let mut changed = false;
    let row = ui
        .horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for chip in options {
                let enabled = chip.disabled.is_none();
                let state = chip_state(*current == chip.value, enabled);
                let (text, fill, border) = match state {
                    ChipState::Selected => (p.on_select, p.select, p.select),
                    ChipState::Idle => (p.ink, Color32::TRANSPARENT, p.line),
                    ChipState::SavedDisabled => (
                        p.disabled_ink,
                        Color32::TRANSPARENT,
                        p.select.gamma_multiply(0.5),
                    ),
                    ChipState::Disabled => (p.disabled_ink, Color32::TRANSPARENT, p.disabled_line),
                };
                let font = if state == ChipState::Selected {
                    semibold(14.0)
                } else {
                    body(14.0)
                };
                let galley = ui
                    .painter()
                    .layout_no_wrap(chip.label.to_owned(), font, text);
                let size = vec2(galley.size().x + 24.0, 30.0);
                // Chips are not focus stops themselves: the row is.
                let sense = if enabled {
                    Sense::CLICK
                } else {
                    Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(size, sense);
                let fill = if state == ChipState::Idle && response.hovered() {
                    p.line
                } else {
                    fill
                };
                ui.painter().rect(
                    rect,
                    CornerRadius::same(6),
                    fill,
                    Stroke::new(1.0, border),
                    StrokeKind::Inside,
                );
                ui.painter()
                    .galley(rect.center() - galley.size() / 2.0, galley, text);
                if let Some(reason) = chip.disabled {
                    response.on_hover_text(reason);
                } else if response.on_hover_cursor(CursorIcon::PointingHand).clicked()
                    && *current != chip.value
                {
                    *current = chip.value.clone();
                    changed = true;
                }
            }
        })
        .response
        .rect;
    let enabled: Vec<bool> = options.iter().map(|chip| chip.disabled.is_none()).collect();
    let focusable = enabled.contains(&true);
    let mut response = ui.interact(
        row,
        id,
        if focusable {
            Sense::FOCUSABLE
        } else {
            Sense::hover()
        },
    );
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                EventFilter {
                    horizontal_arrows: true,
                    ..EventFilter::default()
                },
            );
        });
        let by = ui.input(|input| {
            isize::from(input.key_pressed(Key::ArrowRight) || input.key_pressed(Key::Space))
                - isize::from(input.key_pressed(Key::ArrowLeft))
        });
        if by != 0 {
            let at = options.iter().position(|chip| chip.value == *current);
            if let Some(next) = step_choice(&enabled, at, by)
                && options[next].value != *current
            {
                *current = options[next].value.clone();
                changed = true;
            }
        }
        focus_ring(ui.painter(), row, p.focus);
    }
    if changed {
        response.mark_changed();
    }
    response
}

// ---------------------------------------------------------------- toggle

/// A switch: knob left and outlined is off, knob right on `select` is on.
/// Long labels wrap to the width left (CMR3). With a `disabled` reason it
/// ignores clicks and shows the reason on hover.
pub fn toggle(ui: &mut Ui, on: &mut bool, text: &str, disabled: Option<&str>) -> Response {
    let p = palette(ui);
    let enabled = disabled.is_none();
    let label = if enabled { p.ink } else { p.disabled_ink };
    let wrap = (ui.available_width() - 58.0).max(100.0);
    let galley = ui
        .painter()
        .layout(text.to_owned(), body(15.0), label, wrap);
    let size = vec2(46.0 + 12.0 + galley.size().x, 30.0_f32.max(galley.size().y));
    let (rect, mut response) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if enabled && response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let active = enabled && *on;
    let t = ui.ctx().animate_bool(response.id, active);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 13.0), vec2(46.0, 26.0));
    let painter = ui.painter();
    let (fill, stroke, knob) = match (enabled, active) {
        (false, _) => (Color32::TRANSPARENT, p.disabled_line, p.disabled_ink),
        (true, true) => (p.select, p.select, p.on_select),
        (true, false) => (Color32::TRANSPARENT, p.muted, p.muted),
    };
    painter.rect(
        track,
        CornerRadius::same(13),
        fill,
        Stroke::new(1.5, stroke),
        StrokeKind::Inside,
    );
    let x = egui::lerp(track.left() + 13.0..=track.right() - 13.0, t);
    painter.circle_filled(pos2(x, track.center().y), 9.0, knob);
    painter.galley(
        pos2(
            track.right() + 12.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        label,
    );
    if response.has_focus() {
        focus_ring(painter, track, p.focus);
    }
    match disabled {
        Some(reason) => response.on_hover_text(reason),
        None => response.on_hover_cursor(CursorIcon::PointingHand),
    }
}

// ---------------------------------------------------------------- cards

/// One look/preset card.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LookCard<'a> {
    pub title: &'a str,
    /// The short note under the title.
    pub note: &'a str,
    /// Shown on hover.
    pub detail: Option<&'a str>,
    pub selected: bool,
    /// A badge on the top edge (e.g. "Recommended").
    pub badge: Option<&'a str>,
    /// From [`fit_title`], so every card's title has the same size; 0 = 22.
    pub title_size: f32,
}

/// The largest title size (22 at most, 12 at least) at which every one of
/// `titles` fits `width` (OR).
pub fn fit_title(ui: &Ui, titles: &[&str], width: f32) -> f32 {
    let fits = |size: f32| {
        titles.iter().all(|title| {
            ui.painter()
                .layout_no_wrap((*title).to_owned(), condensed(size), Color32::WHITE)
                .size()
                .x
                <= width
        })
    };
    let mut size = 22.0;
    while size > 12.0 && !fits(size) {
        size -= 0.5;
    }
    size
}

/// A look card of `size`; `clicked()` when chosen (also by Space/A while
/// focused, which is how the pad reaches it).
pub fn look_card(ui: &mut Ui, size: Vec2, card: &LookCard<'_>) -> Response {
    let p = palette(ui);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let focused = response.has_focus();
    let painter = ui.painter();
    let fill = if card.selected {
        p.panel
    } else if response.hovered() || focused {
        p.panel.gamma_multiply(0.6)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if card.selected {
        Stroke::new(2.0, p.select)
    } else {
        Stroke::new(1.0, p.line)
    };
    painter.rect(
        rect,
        CornerRadius::same(8),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    if focused {
        focus_ring(painter, rect, p.focus);
    }
    let pad = 12.0;
    let title_size = if card.title_size > 0.0 {
        card.title_size
    } else {
        22.0
    };
    painter.text(
        rect.left_top() + vec2(pad, pad - 2.0),
        Align2::LEFT_TOP,
        card.title,
        condensed(title_size),
        if card.selected { p.select } else { p.ink },
    );
    if let Some(badge) = card.badge {
        let galley = painter.layout_no_wrap(badge.to_owned(), semibold(12.0), p.on_action);
        let tab = Rect::from_min_size(
            pos2(
                rect.right() - pad - galley.size().x - 12.0,
                rect.top() - 9.0,
            ),
            galley.size() + vec2(12.0, 2.0),
        );
        painter.rect_filled(tab, CornerRadius::same(4), p.action);
        painter.galley(tab.min + vec2(6.0, 1.0), galley, p.on_action);
    }
    let galley = painter.layout(
        card.note.to_owned(),
        body(13.0),
        p.muted,
        size.x - 2.0 * pad,
    );
    painter.galley(rect.left_top() + vec2(pad, pad + 30.0), galley, p.muted);
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    match card.detail {
        Some(detail) => response.on_hover_text(detail),
        None => response,
    }
}

// ---------------------------------------------------------------- text

/// A section heading: small semibold in `muted`.
pub fn heading(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).font(semibold(14.0)).color(p.muted))
}

/// A note under a control: 13 pt, `muted`.
pub fn note(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).font(body(13.0)).color(p.muted))
}

/// An error line: 13 pt, `error`.
pub fn error_note(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).font(body(13.0)).color(p.error))
}

/// Width of a [`field`] label.
pub const FIELD_WIDTH: f32 = 118.0;

/// A row's label, as tall as a chip. Put rows in `ui.horizontal_top` so
/// the label and its chips start on the same line.
pub fn field(ui: &mut Ui, text: &str) {
    let p = palette(ui);
    let (rect, _) = ui.allocate_exact_size(vec2(FIELD_WIDTH, 30.0), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        body(15.0),
        p.ink,
    );
}

// ---------------------------------------------------------------- buttons

/// A secondary button: `action` outline, `ink` text (action is never text).
pub fn outline_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), semibold(15.0), p.ink);
    let size = vec2((galley.size().x + 24.0).max(92.0), 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let fill = if response.hovered() {
        p.line
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::new(1.5, p.action),
        StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.ink);
    if response.has_focus() {
        focus_ring(ui.painter(), rect, p.focus);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A tab's main action: a filled `action` plate, like Play (OR).
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), semibold(15.0), p.on_action);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 36.0, 34.0), Sense::click());
    let fill = if response.hovered() {
        p.action_hover
    } else {
        p.action
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.on_action);
    if response.has_focus() {
        focus_ring(ui.painter(), rect, p.focus);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A thin bar filled from the left with `action`; `fraction` is clamped.
pub fn progress_bar(ui: &mut Ui, fraction: f32) -> Response {
    let p = palette(ui);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 10.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(5), p.line);
    let done = Rect::from_min_size(
        rect.min,
        vec2(width * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter()
        .rect_filled(done, CornerRadius::same(5), p.action);
    response
}

/// A path field with a "Browse…" button; true when the text changed.
/// `pick` opens the folder picker (`rea_launcher_win32::dialog::pick_folder`
/// on Windows, zenity/kdialog natively on Linux) and runs only on click.
pub fn folder_field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    pick: impl FnOnce() -> Option<PathBuf>,
) -> bool {
    let p = palette(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(value)
            .hint_text(RichText::new(hint).color(p.muted))
            .font(body(15.0))
            .margin(vec2(10.0, 8.0))
            .desired_width(ui.available_width() - 100.0);
        changed |= ui.add(edit).changed();
        if outline_button(ui, "Browse…").clicked()
            && let Some(dir) = pick()
        {
            *value = dir.display().to_string();
            changed = true;
        }
    });
    changed
}

// ---------------------------------------------------------------- tabs, footer

/// The tab names, the current one underlined in `action`, then a divider.
/// Tabs are clicked or changed with LB/RB, so they are not focus stops.
/// True when a click changed `current`.
pub fn tabs_bar<T: PartialEq + Clone>(ui: &mut Ui, tabs: &[(T, &str)], current: &mut T) -> bool {
    let p = palette(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 26.0;
        for (tab, name) in tabs {
            let on = *current == *tab;
            let galley = ui
                .painter()
                .layout_no_wrap((*name).to_owned(), condensed(22.0), p.muted);
            let (rect, response) =
                ui.allocate_exact_size(galley.size() + vec2(0.0, 8.0), Sense::CLICK);
            let color = if on || response.hovered() {
                p.ink
            } else {
                p.muted
            };
            ui.painter().galley(rect.min, galley, color);
            if on {
                let bar = Rect::from_min_size(
                    pos2(rect.left(), rect.bottom() - 3.0),
                    vec2(rect.width(), 3.0),
                );
                ui.painter().rect_filled(bar, 0.0, p.action);
            }
            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !on {
                *current = tab.clone();
                changed = true;
            }
        }
    });
    let y = ui.cursor().top() - 4.0;
    let rect = ui.max_rect();
    ui.painter()
        .hline(rect.left()..=rect.right(), y, Stroke::new(1.0, p.line));
    changed
}

/// The slanted Play plate filling `rect`: `action` when `ready`, `line`
/// and muted text when not. Not a focus stop (Start/Enter play).
pub fn play_plate(ui: &mut Ui, rect: Rect, label: &str, ready: bool) -> Response {
    let p = palette(ui);
    let response = ui.interact(rect, ui.scope_id().with("rea-play"), Sense::CLICK);
    let fill = match (ready, response.hovered()) {
        (false, _) => p.line,
        (true, true) => p.action_hover,
        (true, false) => p.action,
    };
    let slant = 10.0;
    ui.painter().add(Shape::convex_polygon(
        vec![
            pos2(rect.left() + slant, rect.top()),
            rect.right_top(),
            pos2(rect.right() - slant, rect.bottom()),
            rect.left_bottom(),
        ],
        fill,
        Stroke::NONE,
    ));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        decal(30.0),
        if ready { p.on_action } else { p.muted },
    );
    if ready {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Width of the Play plate in [`footer`].
pub const PLAY_WIDTH: f32 = 170.0;

/// The footer: `hint` wrapped on the left, the Play plate on the right.
/// Returns the plate's response.
pub fn footer(ui: &mut Ui, rect: Rect, hint: &str, ready: bool) -> Response {
    let p = palette(ui);
    let plate = Rect::from_min_size(
        pos2(rect.right() - PLAY_WIDTH, rect.top()),
        vec2(PLAY_WIDTH, rect.height()),
    );
    let galley = ui.painter().layout(
        hint.to_owned(),
        body(13.0),
        p.muted,
        plate.left() - rect.left() - 20.0,
    );
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(rect.left(), y), galley, p.muted);
    play_plate(ui, plate, "Play", ready)
}

/// A vertical scroll area for one tab's content, remembered per tab, so
/// long tabs scroll inside the body instead of running under the footer (OR).
pub fn tab_body<R>(
    ui: &mut Ui,
    tab: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::ScrollArea::vertical()
        .id_salt(tab)
        .auto_shrink([false, false])
        .show(ui, add)
        .inner
}

// ---------------------------------------------------------------- art

/// Which way a [`gradient`] fades from clear to its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fade {
    /// Clear on the left, solid on the right.
    Right,
    /// Clear on top, solid at the bottom.
    Down,
}

/// Fades `rect` from clear to `color`.
pub fn gradient(painter: &Painter, rect: Rect, color: Color32, fade: Fade) {
    let clear = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 0);
    let (a, b, c, d) = match fade {
        Fade::Right => (clear, color, color, clear),
        Fade::Down => (clear, clear, color, color),
    };
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), a);
    mesh.colored_vertex(rect.right_top(), b);
    mesh.colored_vertex(rect.right_bottom(), c);
    mesh.colored_vertex(rect.left_bottom(), d);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

/// The decal: slanted stripes in the top-right corner of `full`, one per
/// `(width, colour)`, 40 points apart (each launcher passes its colours).
pub fn stripes(painter: &Painter, full: Rect, stripes: &[(f32, Color32)]) {
    let painter = painter.with_clip_rect(full);
    let (slant, depth) = (60.0, 112.0);
    for (index, &(width, color)) in stripes.iter().enumerate() {
        let x = full.right() - 120.0 + index as f32 * 40.0;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(x, full.top()),
                pos2(x + width, full.top()),
                pos2(x + width - slant, full.top() + depth),
                pos2(x - slant, full.top() + depth),
            ],
            color,
            Stroke::NONE,
        ));
    }
}

/// A JPEG (RGB or greyscale) as an egui image; `None` when it does not decode.
pub fn decode_jpeg(bytes: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let size = [usize::from(info.width), usize::from(info.height)];
    let count = size[0] * size[1];
    match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 if pixels.len() == count * 3 => {
            Some(egui::ColorImage::from_rgb(size, &pixels))
        }
        jpeg_decoder::PixelFormat::L8 if pixels.len() == count => Some(egui::ColorImage::new(
            size,
            pixels.iter().map(|&v| Color32::from_gray(v)).collect(),
        )),
        _ => None,
    }
}

/// [`decode_jpeg`] uploaded as a texture (do it once, at start-up).
pub fn load_jpeg(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<TextureHandle> {
    decode_jpeg(bytes).map(|image| ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

/// Which part of a wide picture the photo strip keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crop {
    /// The left edge (CMR4's portrait).
    Start,
    /// The middle (OR's car).
    Center,
}

/// The horizontal texture range that fills a `rect_width`×`rect_height`
/// strip with an `image_width`×`image_height` picture scaled to the height.
pub fn crop_u(image: Vec2, rect: Vec2, crop: Crop) -> (f32, f32) {
    if image.x <= 0.0 || image.y <= 0.0 || rect.y <= 0.0 {
        return (0.0, 1.0);
    }
    let shown = rect.y * image.x / image.y;
    let span = (rect.x / shown).min(1.0);
    match crop {
        Crop::Start => (0.0, span),
        Crop::Center => ((1.0 - span) / 2.0, (1.0 + span) / 2.0),
    }
}

/// The photo strip: `texture` filling `rect`'s height (cropped as asked),
/// then fades into `base` on the right and at the bottom (where the credit
/// lines go). Without a texture only the fades are drawn.
pub fn hero_image(
    painter: &Painter,
    rect: Rect,
    texture: Option<&TextureHandle>,
    crop: Crop,
    base: Color32,
) {
    if let Some(texture) = texture {
        let (u0, u1) = crop_u(texture.size_vec2(), rect.size(), crop);
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(u0, 0.0), pos2(u1, 1.0)),
            Color32::WHITE,
        );
    }
    gradient(
        painter,
        Rect::from_min_max(
            pos2(rect.left() + rect.width() * 0.55, rect.top()),
            rect.max,
        ),
        base,
        Fade::Right,
    );
    gradient(
        painter,
        Rect::from_min_max(pos2(rect.left(), rect.bottom() - 130.0), rect.max),
        base,
        Fade::Down,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_states() {
        assert_eq!(chip_state(true, true), ChipState::Selected);
        assert_eq!(chip_state(false, true), ChipState::Idle);
        assert_eq!(chip_state(true, false), ChipState::SavedDisabled);
        assert_eq!(chip_state(false, false), ChipState::Disabled);
    }

    #[test]
    fn stepping_wraps_and_skips_disabled_choices() {
        let all = [true, true, true];
        assert_eq!(step_choice(&all, Some(0), 1), Some(1));
        assert_eq!(step_choice(&all, Some(2), 1), Some(0));
        assert_eq!(step_choice(&all, Some(0), -1), Some(2));
        assert_eq!(step_choice(&all, Some(0), 2), Some(2));
        let some = [true, false, true, false];
        assert_eq!(step_choice(&some, Some(0), 1), Some(2));
        assert_eq!(step_choice(&some, Some(2), 1), Some(0));
        assert_eq!(step_choice(&some, Some(0), -1), Some(2));
        // From a disabled (saved) value, to the next enabled one.
        assert_eq!(step_choice(&some, Some(1), 1), Some(2));
        assert_eq!(step_choice(&some, Some(1), -1), Some(0));
        // No current value: forward from the start, backward from the end.
        assert_eq!(step_choice(&some, None, 1), Some(0));
        assert_eq!(step_choice(&some, None, -1), Some(2));
        // A single enabled choice stays put; none at all gives nothing.
        assert_eq!(step_choice(&[false, true], Some(1), 1), Some(1));
        assert_eq!(step_choice(&[false, false], Some(0), 1), None);
        assert_eq!(step_choice(&[], None, 1), None);
        assert_eq!(step_choice(&all, Some(1), 0), Some(1));
    }

    #[test]
    fn crop_keeps_the_start_or_the_middle() {
        // A 600×400 picture in a 300×400 strip shows half its width.
        let (image, strip) = (vec2(600.0, 400.0), vec2(300.0, 400.0));
        assert_eq!(crop_u(image, strip, Crop::Start), (0.0, 0.5));
        assert_eq!(crop_u(image, strip, Crop::Center), (0.25, 0.75));
        // A narrow picture is stretched, never over-cropped.
        assert_eq!(crop_u(vec2(100.0, 400.0), strip, Crop::Center), (0.0, 1.0));
        assert_eq!(crop_u(Vec2::ZERO, strip, Crop::Start), (0.0, 1.0));
    }

    #[test]
    fn jpegs_decode_in_colour_and_grey() {
        let red = decode_jpeg(include_bytes!("../tests/data/red-4x2.jpg")).unwrap();
        assert_eq!(red.size, [4, 2]);
        let px = red.pixels[0];
        assert!(px.r() > 240 && px.g() < 16 && px.b() < 16, "{px:?}");
        let grey = decode_jpeg(include_bytes!("../tests/data/gray-3x3.jpg")).unwrap();
        assert_eq!(grey.size, [3, 3]);
        assert!(grey.pixels[4].r().abs_diff(128) < 4);
        assert!(decode_jpeg(b"not a jpeg").is_none());
    }

    /// Runs one frame of `add` with `events`, after a first frame that
    /// lays the widgets out.
    fn frames(ctx: &egui::Context, events: Vec<Vec<egui::Event>>, mut add: impl FnMut(&mut Ui)) {
        for events in events {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
                ..Default::default()
            };
            ctx.run_ui(input, &mut add).textures_delta.clear();
        }
    }

    fn key(key: Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .collect()
    }

    #[test]
    fn a_focused_chip_row_steps_with_the_arrows() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx, &Palette::default());
        let mut value = 60u32;
        let mut toggled = false;
        let options = [(60u32, "60"), (90, "90"), (120, "120")];
        let tab = key(Key::Tab, egui::Modifiers::NONE);
        let right = key(Key::ArrowRight, egui::Modifiers::NONE);
        let left = key(Key::ArrowLeft, egui::Modifiers::NONE);
        frames(
            &ctx,
            vec![
                vec![],
                tab.clone(),   // focus the chip row
                right.clone(), // 60 → 90 (focus lock not yet set)
                right.clone(), // 90 → 120
                right,         // wraps to 60
                left,          // back to 120
                tab,           // on to the toggle
                key(Key::Space, egui::Modifiers::NONE),
            ],
            |ui| {
                chips(ui, &options, &mut value, None);
                toggle(ui, &mut toggled, "VSync", None);
            },
        );
        assert_eq!(value, 120);
        assert!(toggled);
    }

    #[test]
    fn a_disabled_row_is_not_a_focus_stop_and_keeps_its_value() {
        let ctx = egui::Context::default();
        let mut value = 1u32;
        let mut on = false;
        frames(
            &ctx,
            vec![
                vec![],
                key(Key::Tab, egui::Modifiers::NONE),
                key(Key::Space, egui::Modifiers::NONE),
            ],
            |ui| {
                chips(ui, &[(1u32, "a"), (2, "b")], &mut value, Some("needs DXVK"));
                toggle(ui, &mut on, "x", None);
            },
        );
        assert_eq!(value, 1);
        assert!(on, "Tab skipped the disabled row and reached the toggle");
    }
}
