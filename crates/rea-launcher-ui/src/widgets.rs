//! The launchers' widgets, drawn in the installed [`Palette`] and
//! [`Metrics`].
//!
//! Where each one comes from:
//! - [`chips`]/[`chips_with`]/[`chips_sized`]: SCR's per-chip disabled
//!   reasons and compact size + SRR's "keep the saved value outlined while
//!   disabled", made one focus stop whose value ←/→ (the pad's left/right)
//!   step, as SCR's pad did.
//! - [`toggle`]: OR's (returns its `Response`) with CMR3's label wrap
//!   ([`Metrics::toggle_wrap`]) and SCR's disabled reason.
//! - [`look_card`]/[`fit_title`]: CMR4's focus ring and detail tooltip, OR's
//!   title fitting; [`game_card`]: SCR's trilogy picker.
//! - [`field_focused`], [`heading_focused`], [`focus_bar`]: SCR's
//!   row-focus model.
//! - [`primary_button`], [`progress_bar`]: OR. [`outline_button`],
//!   [`folder_field`], [`heading`], [`field`], [`tabs_bar`],
//!   [`play_plate`]/[`footer`], [`gradient`], [`stripes`]: the same in all
//!   five; the colours follow OR's split (action vs selection).
//! - [`decode_jpeg`]/[`hero_image`]: the portrait strip, CMR4 (left crop)
//!   and OR (centre crop).
//!
//! Every focusable widget draws a focus ring, so pad navigation is
//! visible, and every widget reports its role, label, state and (when
//! disabled) the reason to AccessKit, so screen readers and the
//! [`crate::testing`] harness can find it.

use std::path::PathBuf;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, EventFilter, Key, Mesh, Modifiers, Painter,
    Rect, Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, TextureHandle, Ui, Vec2,
    WidgetInfo, pos2, vec2,
};

use crate::theme::{self, ButtonText, Metrics, Palette, body, condensed, decal, semibold, size};

fn palette(ui: &Ui) -> Palette {
    theme::palette(ui.ctx())
}

fn metrics(ui: &Ui) -> Metrics {
    theme::metrics(ui.ctx())
}

/// Reports `info` for `response` to AccessKit, with an optional
/// description (a disabled reason, a card's note).
fn accessible(response: &Response, info: WidgetInfo, description: Option<&str>) {
    response.widget_info(|| info.clone());
    if let Some(description) = description.filter(|text| !text.is_empty()) {
        response.ctx.accesskit_node_builder(response.id, |node| {
            node.set_description(description.to_owned());
        });
    }
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
    /// Why it cannot be chosen now (shown under the row, on hover and to
    /// screen readers); `None` when it can.
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

/// Chip padding: [`Metrics::chip_pad`]/[`Metrics::chip_gap`] or the compact
/// pair (SCR's button-mapping rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipSize {
    #[default]
    Normal,
    Compact,
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
/// Disabled chips ignore clicks; their reasons are written under the row
/// (a pad never hovers: [`disabled_reasons`]) and shown on hover. The saved
/// value stays outlined while disabled ([`ChipState::SavedDisabled`]). The
/// row's response is `changed()` when the value changed.
///
/// Accessibility: the row is a radio group whose value is the chosen
/// label; each chip is a radio button (checked when chosen, disabled with
/// its reason as the description).
pub fn chips_with<T: PartialEq + Clone>(
    ui: &mut Ui,
    options: &[Chip<'_, T>],
    current: &mut T,
) -> Response {
    chips_sized(ui, options, current, ChipSize::Normal)
}

/// [`chips_with`] at a [`ChipSize`].
pub fn chips_sized<T: PartialEq + Clone>(
    ui: &mut Ui,
    options: &[Chip<'_, T>],
    current: &mut T,
    size: ChipSize,
) -> Response {
    chips_noted(ui, options, current, size, true)
}

/// What a chip row writes under itself about its disabled chips, one line
/// per reason: the reason alone when every chip is disabled for it,
/// otherwise the chips' labels first ("Glide, Vulkan: Needs the 3dfx
/// build."). Empty when every chip can be chosen.
pub fn disabled_reasons<T>(options: &[Chip<'_, T>]) -> Vec<String> {
    let mut reasons: Vec<(&str, Vec<&str>)> = Vec::new();
    for chip in options {
        let Some(why) = chip.disabled.filter(|why| !why.is_empty()) else {
            continue;
        };
        match reasons.iter_mut().find(|(seen, _)| *seen == why) {
            Some((_, labels)) => labels.push(chip.label),
            None => reasons.push((why, vec![chip.label])),
        }
    }
    let all = reasons.len() == 1 && reasons[0].1.len() == options.len();
    reasons
        .into_iter()
        .map(|(why, labels)| {
            if all {
                why.to_owned()
            } else {
                format!("{}: {why}", labels.join(", "))
            }
        })
        .collect()
}

/// A chip row, with its [`disabled_reasons`] under it when `noted` (spec
/// rows write the reason in place of their description instead).
pub(crate) fn chips_noted<T: PartialEq + Clone>(
    ui: &mut Ui,
    options: &[Chip<'_, T>],
    current: &mut T,
    size: ChipSize,
    noted: bool,
) -> Response {
    // The focus id stays the same whether or not a note follows.
    let id = ui.next_auto_id().with("rea-chips");
    let reasons = if noted {
        disabled_reasons(options)
    } else {
        Vec::new()
    };
    if reasons.is_empty() {
        return chip_row(ui, id, options, current, size);
    }
    ui.vertical(|ui| {
        let row = chip_row(ui, id, options, current, size);
        for line in &reasons {
            note(ui, line);
        }
        row
    })
    .inner
}

fn chip_row<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: egui::Id,
    options: &[Chip<'_, T>],
    current: &mut T,
    size: ChipSize,
) -> Response {
    let p = palette(ui);
    let m = metrics(ui);
    let (pad, gap) = match size {
        ChipSize::Normal => (m.chip_pad, m.chip_gap),
        ChipSize::Compact => (m.compact_chip_pad, m.compact_chip_gap),
    };
    let mut changed = false;
    let row = ui
        .horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for chip in options {
                let enabled = chip.disabled.is_none();
                let selected = *current == chip.value;
                let state = chip_state(selected, enabled);
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
                    semibold(size::CHIP)
                } else {
                    body(size::CHIP)
                };
                let galley = ui
                    .painter()
                    .layout_no_wrap(chip.label.to_owned(), font, text);
                let size = vec2(galley.size().x + pad, 30.0);
                // Chips are not focus stops themselves: the row is.
                let sense = if enabled {
                    Sense::CLICK
                } else {
                    Sense::hover()
                };
                let (rect, response) = ui.allocate_exact_size(size, sense);
                let fill = if state == ChipState::Idle && response.hovered() {
                    p.panel
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
                accessible(
                    &response,
                    WidgetInfo::selected(Role::RadioButton, enabled, selected, chip.label),
                    chip.disabled,
                );
                if let Some(reason) = chip.disabled {
                    response.on_hover_text(reason);
                } else if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !selected
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
    let chosen = options
        .iter()
        .find(|chip| chip.value == *current)
        .map(|chip| chip.label);
    response.widget_info(|| WidgetInfo {
        enabled: focusable,
        ..WidgetInfo::new(Role::RadioGroup)
    });
    if let Some(chosen) = chosen {
        ui.ctx()
            .accesskit_node_builder(id, |node| node.set_value(chosen.to_owned()));
    }
    response
}

// ---------------------------------------------------------------- toggle

/// A switch: knob left and outlined is off, knob right on `select` is on.
/// Long labels wrap to the width left when [`Metrics::toggle_wrap`]. With a
/// `disabled` reason it ignores clicks, keeps showing the saved value
/// (knob where it is, a dimmed outline) and writes the reason under its
/// label, where a pad player sees it (also on hover).
/// Accessibility: a switch named by its label, the reason its description.
pub fn toggle(ui: &mut Ui, on: &mut bool, text: &str, disabled: Option<&str>) -> Response {
    toggle_noted(ui, on, text, disabled, true)
}

/// [`toggle`]; the reason under the label only when `noted` (spec rows
/// write it in place of their description instead).
pub(crate) fn toggle_noted(
    ui: &mut Ui,
    on: &mut bool,
    text: &str,
    disabled: Option<&str>,
    noted: bool,
) -> Response {
    let p = palette(ui);
    let enabled = disabled.is_none();
    let label = if enabled { p.ink } else { p.disabled_ink };
    let galley = if metrics(ui).toggle_wrap {
        let wrap = (ui.available_width() - 58.0).max(100.0);
        ui.painter()
            .layout(text.to_owned(), body(size::LABEL), label, wrap)
    } else {
        ui.painter()
            .layout_no_wrap(text.to_owned(), body(size::LABEL), label)
    };
    // The reason always wraps: it can be a sentence.
    let reason = disabled.filter(|why| noted && !why.is_empty()).map(|why| {
        let wrap = (ui.available_width() - 58.0).max(100.0);
        ui.painter()
            .layout(why.to_owned(), body(size::NOTE), p.muted, wrap)
    });
    let line = 30.0_f32.max(galley.size().y);
    let below = reason.as_ref().map_or(0.0, |r| r.size().y + 2.0);
    let width = galley
        .size()
        .x
        .max(reason.as_ref().map_or(0.0, |r| r.size().x));
    let size = vec2(46.0 + 12.0 + width, line + below);
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
    // Disabled, the knob still shows the saved value (SRR's chips).
    let t = ui.ctx().animate_bool(response.id, *on);
    let middle = rect.top() + line / 2.0;
    let track = Rect::from_min_size(pos2(rect.left(), middle - 13.0), vec2(46.0, 26.0));
    let painter = ui.painter();
    let (fill, stroke, knob) = match (enabled, *on) {
        (false, true) => (
            Color32::TRANSPARENT,
            p.select.gamma_multiply(0.5),
            p.disabled_ink,
        ),
        (false, false) => (Color32::TRANSPARENT, p.disabled_line, p.disabled_ink),
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
    let text_x = track.right() + 12.0;
    painter.galley(pos2(text_x, middle - galley.size().y / 2.0), galley, label);
    if let Some(reason) = reason {
        painter.galley(pos2(text_x, rect.top() + line + 2.0), reason, p.muted);
    }
    if response.has_focus() {
        focus_ring(painter, track, p.focus);
    }
    accessible(
        &response,
        WidgetInfo::selected(Role::Switch, enabled, *on, text),
        disabled,
    );
    match disabled {
        Some(reason) => response.on_hover_text(reason),
        None => response.on_hover_cursor(CursorIcon::PointingHand),
    }
}

/// `first` and `second` as two sentences: a full stop between them unless
/// `first` already ends one (". ", "! ", "? ", "… "); either may be empty.
pub fn join_sentences(first: &str, second: &str) -> String {
    let (first, second) = (first.trim_end(), second.trim());
    if first.is_empty() {
        return second.to_owned();
    }
    if second.is_empty() {
        return first.to_owned();
    }
    if first.ends_with(['.', '!', '?', '…', ':']) {
        format!("{first} {second}")
    } else {
        format!("{first}. {second}")
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
    /// From [`fit_title`], so every card's title has the same size; 0 uses
    /// [`theme::CardMetrics::title`].
    pub title_size: f32,
    /// Why it can't be chosen now: dimmed, ignores clicks, and the reason
    /// is written in place of the note (a pad never hovers), shown on hover
    /// and given to screen readers after the note.
    pub disabled: Option<&'a str>,
}

/// The largest title size ([`theme::CardMetrics::title`] at most,
/// [`size::FIT_TITLE_MIN`] at least) at which every one of `titles` fits `width` (OR).
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
    let mut size = metrics(ui).card.title;
    while size > size::FIT_TITLE_MIN && !fits(size) {
        size -= 0.5;
    }
    size
}

/// The card frame shared by look and game cards: fill, border, focus ring.
fn card_frame(ui: &Ui, rect: Rect, response: &Response, selected: bool, enabled: bool) {
    let p = palette(ui);
    let focused = response.has_focus();
    let fill = if !enabled {
        Color32::TRANSPARENT
    } else if selected {
        p.panel
    } else if response.hovered() || focused {
        p.panel.gamma_multiply(0.6)
    } else {
        Color32::TRANSPARENT
    };
    let stroke = match (enabled, selected) {
        (false, true) => Stroke::new(2.0, p.select.gamma_multiply(0.5)),
        (false, false) => Stroke::new(1.0, p.disabled_line),
        (true, true) => Stroke::new(2.0, p.select),
        (true, false) => Stroke::new(1.0, p.line),
    };
    let painter = ui.painter();
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
}

/// A look card of `size`; `clicked()` when chosen (also by Space/A while
/// focused, which is how the pad reaches it). Accessibility: a radio
/// button named by its title, its note (and detail) as the description,
/// disabled with its reason when [`LookCard::disabled`].
pub fn look_card(ui: &mut Ui, size: Vec2, card: &LookCard<'_>) -> Response {
    let p = palette(ui);
    let c = metrics(ui).card;
    let enabled = card.disabled.is_none();
    let (rect, response) = ui.allocate_exact_size(size, card_sense(enabled));
    card_frame(ui, rect, &response, card.selected, enabled);
    let (ink, muted) = if enabled {
        (p.ink, p.muted)
    } else {
        (p.disabled_ink, p.disabled_ink)
    };
    let painter = ui.painter();
    let title_size = if card.title_size > 0.0 {
        card.title_size
    } else {
        c.title
    };
    painter.text(
        rect.left_top() + vec2(c.pad, c.pad - 2.0),
        Align2::LEFT_TOP,
        card.title,
        condensed(title_size),
        if card.selected && enabled {
            p.select
        } else {
            ink
        },
    );
    if let Some(badge) = card.badge {
        let galley = painter.layout_no_wrap(badge.to_owned(), semibold(c.badge), p.on_action);
        let tab = Rect::from_min_size(
            pos2(
                rect.right() - c.pad - galley.size().x - 2.0 * c.badge_pad,
                rect.top() - c.badge_rise,
            ),
            galley.size() + vec2(2.0 * c.badge_pad, 2.0),
        );
        painter.rect_filled(tab, CornerRadius::same(4), p.action);
        painter.galley(tab.min + vec2(c.badge_pad, 1.0), galley, p.on_action);
    }
    // Disabled, the reason takes the note's place, readable in `muted`.
    let (shown, color) = match card.disabled.filter(|why| !why.is_empty()) {
        Some(why) => (why, p.muted),
        None => (card.note, muted),
    };
    let galley = painter.layout(shown.to_owned(), body(c.note), color, size.x - 2.0 * c.pad);
    painter.galley(
        rect.left_top() + vec2(c.pad, c.pad + c.note_top),
        galley,
        color,
    );
    let mut description = join_sentences(card.badge.unwrap_or_default(), card.note);
    if let Some(detail) = card.detail {
        description = join_sentences(&description, detail);
    }
    if let Some(why) = card.disabled {
        description = join_sentences(&description, why);
    }
    accessible(
        &response,
        WidgetInfo::selected(Role::RadioButton, enabled, card.selected, card.title),
        Some(&description),
    );
    finish_card(response, card.disabled.or(card.detail), enabled)
}

fn card_sense(enabled: bool) -> Sense {
    if enabled {
        Sense::click()
    } else {
        Sense::hover()
    }
}

/// The hover text (reason or detail) and pointer of a card.
fn finish_card(response: Response, hover: Option<&str>, enabled: bool) -> Response {
    let response = if enabled {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    };
    match hover {
        Some(text) => response.on_hover_text(text),
        None => response,
    }
}

/// One game in a multi-game launcher (SCR's trilogy picker).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameCard<'a> {
    pub title: &'a str,
    /// The line under the title (year, edition, "Not in folder").
    pub subtitle: &'a str,
    pub selected: bool,
    /// Whether the game was found: the dot is `select` when it was,
    /// `disabled_ink` (and the subtitle dimmed) when not.
    pub installed: bool,
    /// Why it can't be chosen now: written in place of the subtitle (see
    /// [`LookCard::disabled`]).
    pub disabled: Option<&'a str>,
}

/// A game card of `size`; `clicked()` when chosen. It stays choosable when
/// the game is not installed (the subtitle says why it can't play).
/// Accessibility: a radio button named by the title, the subtitle and
/// "installed"/"not installed" as the description.
pub fn game_card(ui: &mut Ui, size: Vec2, card: &GameCard<'_>) -> Response {
    let p = palette(ui);
    let c = metrics(ui).card;
    let enabled = card.disabled.is_none();
    let (rect, response) = ui.allocate_exact_size(size, card_sense(enabled));
    card_frame(ui, rect, &response, card.selected, enabled);
    let painter = ui.painter();
    let title = match (enabled, card.selected) {
        (false, _) => p.disabled_ink,
        (true, true) => p.select,
        (true, false) => p.ink,
    };
    painter.text(
        rect.left_top() + vec2(c.pad, c.pad - 2.0),
        Align2::LEFT_TOP,
        card.title,
        condensed(c.title),
        title,
    );
    let (dot, sub) = if card.installed && enabled {
        (p.select, p.muted)
    } else {
        (p.disabled_ink, p.disabled_ink)
    };
    painter.circle_filled(pos2(rect.right() - c.pad, rect.top() + 18.0), 4.0, dot);
    let (subtitle, sub) = match card.disabled.filter(|why| !why.is_empty()) {
        Some(why) => (why, p.muted),
        None => (card.subtitle, sub),
    };
    painter.text(
        rect.left_top() + vec2(c.pad, c.pad + 24.0),
        Align2::LEFT_TOP,
        subtitle,
        body(c.note),
        sub,
    );
    let state = if card.installed {
        "installed"
    } else {
        "not installed"
    };
    let mut description = format!("{}; {state}", card.subtitle);
    if let Some(why) = card.disabled {
        description = join_sentences(&description, why);
    }
    accessible(
        &response,
        WidgetInfo::selected(Role::RadioButton, enabled, card.selected, card.title),
        Some(&description),
    );
    finish_card(response, card.disabled, enabled)
}

// ---------------------------------------------------------------- text

/// A section heading: small semibold in `muted`.
pub fn heading(ui: &mut Ui, text: &str) -> Response {
    heading_focused(ui, text, false)
}

/// [`heading`], in `focus` while its row has the pad's focus (SCR).
pub fn heading_focused(ui: &mut Ui, text: &str, focused: bool) -> Response {
    let p = palette(ui);
    let color = if focused { p.focus } else { p.muted };
    ui.label(
        RichText::new(text)
            .font(semibold(size::HEADING))
            .color(color),
    )
}

/// A note under a control: 13 pt, `muted`.
pub fn note(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).font(body(size::NOTE)).color(p.muted))
}

/// An error line: 13 pt, `error`.
pub fn error_note(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    ui.label(RichText::new(text).font(body(size::NOTE)).color(p.error))
}

/// The default width of a [`field`] label ([`Metrics::field_width`]).
pub const FIELD_WIDTH: f32 = Metrics::DEFAULT.field_width;

/// A row's label, [`Metrics::field_width`] wide and as tall as a chip. Put
/// rows in `ui.horizontal_top` so the label and its chips start on the
/// same line. Accessibility: a text label.
pub fn field(ui: &mut Ui, text: &str) -> Response {
    field_focused(ui, text, false)
}

/// [`field`], in `focus` while its row has the pad's focus (SCR).
pub fn field_focused(ui: &mut Ui, text: &str, focused: bool) -> Response {
    let p = palette(ui);
    let width = metrics(ui).field_width;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        body(size::LABEL),
        if focused { p.focus } else { p.ink },
    );
    accessible(
        &response,
        WidgetInfo::labeled(Role::Label, true, text),
        None,
    );
    response
}

/// A bar left of `rect` in `focus` when `focused`: SCR's mark for the row
/// the pad is on.
pub fn focus_bar(ui: &Ui, rect: Rect, focused: bool) {
    if focused {
        let bar = Rect::from_min_size(
            pos2(rect.left() - 8.0, rect.top() + 2.0),
            vec2(3.0, (rect.height() - 4.0).max(8.0)),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(1), palette(ui).focus);
    }
}

// ---------------------------------------------------------------- buttons

/// The text colour of an outline button.
fn button_text(p: &Palette, m: &Metrics, hovered: bool) -> Color32 {
    match (hovered, m.button_text) {
        // The hover fill is `panel`; ink always reads on it.
        (true, _) | (false, ButtonText::Ink) => p.ink,
        (false, ButtonText::Action) => p.action,
    }
}

/// A secondary button: `action` outline, text per
/// [`Metrics::button_text`] (ink while hovered). Accessibility: a button.
pub fn outline_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    let m = metrics(ui);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), semibold(size::BUTTON), p.ink)
        .size()
        .x;
    let size = vec2((width + 24.0).max(92.0), 34.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let fill = if hovered {
        p.panel
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
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        semibold(size::BUTTON),
        button_text(&p, &m, hovered),
    );
    if response.has_focus() {
        focus_ring(ui.painter(), rect, p.focus);
    }
    accessible(
        &response,
        WidgetInfo::labeled(Role::Button, true, text),
        None,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A tab's main action: a filled `action` plate, like Play (OR).
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let p = palette(ui);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), semibold(size::BUTTON), p.on_action);
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
    accessible(
        &response,
        WidgetInfo::labeled(Role::Button, true, text),
        None,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A thin bar filled from the left with `action`; `fraction` is clamped.
/// Accessibility: a progress indicator with the fraction as its value.
pub fn progress_bar(ui: &mut Ui, fraction: f32) -> Response {
    let p = palette(ui);
    let fraction = fraction.clamp(0.0, 1.0);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 10.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(5), p.line);
    let done = Rect::from_min_size(rect.min, vec2(width * fraction, rect.height()));
    ui.painter()
        .rect_filled(done, CornerRadius::same(5), p.action);
    accessible(
        &response,
        WidgetInfo {
            value: Some(f64::from(fraction)),
            ..WidgetInfo::new(Role::ProgressIndicator)
        },
        None,
    );
    response
}

/// A path field with a "Browse…" button; true when the text changed.
/// `label` names the field (the heading or field label shown with it),
/// `hint` is the placeholder. `pick` opens the folder picker
/// (`rea_launcher_win32::dialog::pick_folder` on Windows, zenity/kdialog
/// natively on Linux) and runs only on click. Accessibility: the text
/// field is named by `label`, with `hint` as its description.
pub fn folder_field(
    ui: &mut Ui,
    value: &mut String,
    label: &str,
    hint: &str,
    pick: impl FnOnce() -> Option<PathBuf>,
) -> bool {
    let p = palette(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(value)
            .hint_text(RichText::new(hint).color(p.muted))
            .font(body(size::LABEL))
            .margin(vec2(10.0, 8.0))
            .desired_width(ui.available_width() - 100.0);
        let response = ui.add(edit);
        changed |= response.changed();
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_label(label.to_owned());
            node.set_description(hint.to_owned());
        });
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

/// The colour of a tab's name: `ink` for the current or hovered tab,
/// `muted` otherwise.
fn tab_color(p: &Palette, current: bool, hovered: bool) -> Color32 {
    if current || hovered { p.ink } else { p.muted }
}

/// The tab names, the current one in `ink` and underlined in `action`,
/// then a divider. Tabs are clicked, changed with LB/RB on a pad, or with
/// Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PageDown / Ctrl+PageUp (wrapping);
/// they are not Tab-key focus stops. True when `current` changed.
/// Accessibility: a tab list of tabs, the current one selected; each tab
/// takes the screen reader's click action.
pub fn tabs_bar<T: PartialEq + Clone>(ui: &mut Ui, tabs: &[(T, &str)], current: &mut T) -> bool {
    let p = palette(ui);
    let mut changed = false;
    // Keyboard: Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+PageDown / Ctrl+PageUp
    // (CMR4), as LB/RB do on a pad.
    let step = ui.input_mut(|input| {
        // The Shift form first: Ctrl+Tab would also match it.
        let back = input.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::Tab)
            || input.consume_key(Modifiers::CTRL, Key::PageUp);
        let next = input.consume_key(Modifiers::CTRL, Key::Tab)
            || input.consume_key(Modifiers::CTRL, Key::PageDown);
        isize::from(next) - isize::from(back)
    });
    if step != 0 && !tabs.is_empty() {
        let at = tabs.iter().position(|(tab, _)| tab == current).unwrap_or(0) as isize;
        let next = (at + step).rem_euclid(tabs.len() as isize) as usize;
        *current = tabs[next].0.clone();
        changed = true;
    }
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 26.0;
        for (tab, name) in tabs {
            let on = *current == *tab;
            let size = ui
                .painter()
                .layout_no_wrap((*name).to_owned(), condensed(size::TAB), p.muted)
                .size();
            let (rect, response) = ui.allocate_exact_size(size + vec2(0.0, 8.0), Sense::CLICK);
            // Laid out in its final colour: a galley's own colour wins
            // over the painter's fallback.
            let color = tab_color(&p, on, response.hovered());
            let galley =
                ui.painter()
                    .layout_no_wrap((*name).to_owned(), condensed(size::TAB), color);
            ui.painter().galley(rect.min, galley, color);
            if on {
                let bar = Rect::from_min_size(
                    pos2(rect.left(), rect.bottom() - 3.0),
                    vec2(rect.width(), 3.0),
                );
                ui.painter().rect_filled(bar, 0.0, p.action);
            }
            accessible(
                &response,
                WidgetInfo::selected(Role::Tab, true, on, *name),
                None,
            );
            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !on {
                *current = tab.clone();
                changed = true;
            }
        }
    });
    // The tabs' container, so screen readers announce a tab list.
    let list = ui.interact(
        row.response.rect,
        row.response.id.with("rea-tabs"),
        Sense::hover(),
    );
    accessible(&list, WidgetInfo::new(Role::TabList), None);
    let y = ui.cursor().top() - 4.0;
    let rect = ui.max_rect();
    ui.painter()
        .hline(rect.left()..=rect.right(), y, Stroke::new(1.0, p.line));
    changed
}

/// The slanted Play plate filling `rect`: `action` when `ready`, `panel`
/// and muted text when not. Not a focus stop (Start/Enter play).
/// Accessibility: a button named `label`, disabled when not ready.
pub fn play_plate(ui: &mut Ui, rect: Rect, label: &str, ready: bool) -> Response {
    let p = palette(ui);
    let response = ui.interact(rect, ui.scope_id().with("rea-play"), Sense::CLICK);
    let fill = match (ready, response.hovered()) {
        (false, _) => p.panel,
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
        decal(size::PLAY),
        if ready { p.on_action } else { p.muted },
    );
    accessible(
        &response,
        WidgetInfo::labeled(Role::Button, ready, label),
        None,
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
/// Returns the plate's response. While not `ready` the hint is also the
/// plate's accessible description (why Play is disabled).
pub fn footer(ui: &mut Ui, rect: Rect, hint: &str, ready: bool) -> Response {
    let p = palette(ui);
    let plate = Rect::from_min_size(
        pos2(rect.right() - PLAY_WIDTH, rect.top()),
        vec2(PLAY_WIDTH, rect.height()),
    );
    let galley = ui.painter().layout(
        hint.to_owned(),
        body(size::NOTE),
        p.muted,
        plate.left() - rect.left() - 20.0,
    );
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter().galley(pos2(rect.left(), y), galley, p.muted);
    let play = play_plate(ui, plate, "Play", ready);
    if !ready {
        // A disabled Play says why to screen readers too (CMR4).
        name_control(&play, None, Some(hint));
    }
    play
}

/// Gives any control an accessible name and/or description: a chip row's
/// group label ("Frame rate") and what it does, a Play plate's reason.
pub fn name_control(response: &Response, label: Option<&str>, description: Option<&str>) {
    response.ctx.accesskit_node_builder(response.id, |node| {
        if let Some(label) = label {
            node.set_label(label.to_owned());
        }
        if let Some(description) = description.filter(|text| !text.is_empty()) {
            node.set_description(description.to_owned());
        }
    });
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

/// How a [`photo_caption`] line is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionStyle {
    /// A name: semibold 19 pt, `ink`.
    Title,
    /// A line under it: 13 pt, `muted`.
    Text,
    /// The photo credit: 11 pt, `muted`.
    Credit,
}

/// The caption in the photo strip's lower left (CMR4: name, years, credit;
/// OR: three small lines), `lines` top to bottom. Accessibility: the strip
/// is an image named by `alt` (what the photo shows), and the caption one
/// label with every line.
pub fn photo_caption(ui: &mut Ui, strip: Rect, alt: &str, lines: &[(&str, CaptionStyle)]) {
    let p = palette(ui);
    let image = ui.interact(strip, ui.scope_id().with("rea-photo"), Sense::hover());
    accessible(&image, WidgetInfo::labeled(Role::Image, true, alt), None);
    let x = strip.left() + 20.0;
    let mut y = strip.bottom() - 20.0;
    let mut top = y;
    for &(text, style) in lines.iter().rev() {
        let (font, color, step) = match style {
            CaptionStyle::Title => (semibold(19.0), p.ink, 24.0),
            CaptionStyle::Text => (body(size::NOTE), p.muted, 18.0),
            CaptionStyle::Credit => (body(size::MIN_TEXT), p.muted, 16.0),
        };
        let drawn = ui
            .painter()
            .text(pos2(x, y), Align2::LEFT_BOTTOM, text, font, color);
        top = drawn.top();
        y -= step;
    }
    if !lines.is_empty() {
        let rect = Rect::from_min_max(
            pos2(x, top),
            pos2(strip.right() - 8.0, strip.bottom() - 20.0),
        );
        let caption = ui.interact(rect, ui.scope_id().with("rea-caption"), Sense::hover());
        let text: Vec<&str> = lines.iter().map(|(t, _)| *t).collect();
        accessible(
            &caption,
            WidgetInfo::labeled(Role::Label, true, text.join(". ")),
            None,
        );
    }
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

    /// The text drawn in one frame of `add`, with its colours and rects.
    fn texts(
        palette: &Palette,
        metrics: &Metrics,
        mut add: impl FnMut(&mut Ui),
    ) -> Vec<(String, Color32, Rect)> {
        let ctx = egui::Context::default();
        theme::install_with(&ctx, palette, metrics);
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            ..Default::default()
        };
        // The first frame loads the fonts.
        ctx.run_ui(input(), &mut add).textures_delta.clear();
        let mut output = ctx.run_ui(input(), &mut add);
        output.textures_delta.clear();
        output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    text.galley.job.sections[0].format.color,
                    text.visual_bounding_rect(),
                )),
                _ => None,
            })
            .collect()
    }

    fn color_of(texts: &[(String, Color32, Rect)], text: &str) -> Color32 {
        texts
            .iter()
            .find(|(t, ..)| t == text)
            .unwrap_or_else(|| panic!("{text} not drawn in {texts:?}"))
            .1
    }

    fn rect_of(texts: &[(String, Color32, Rect)], text: &str) -> Rect {
        texts.iter().find(|(t, ..)| t == text).unwrap().2
    }

    #[test]
    fn the_current_tab_is_ink_the_others_muted() {
        let p = crate::theme::tests::OR;
        let texts = texts(&p, &Metrics::DEFAULT, |ui| {
            let mut tab = 1u8;
            tabs_bar(ui, &[(0u8, "Play"), (1, "Picture")], &mut tab);
        });
        assert_eq!(color_of(&texts, "Picture"), p.ink);
        assert_eq!(color_of(&texts, "Play"), p.muted);
    }

    #[test]
    fn button_text_follows_the_metrics() {
        let p = crate::theme::tests::OR;
        let action = texts(&p, &Metrics::DEFAULT, |ui| {
            outline_button(ui, "Browse…");
        });
        assert_eq!(color_of(&action, "Browse…"), p.action);
        let ink = texts(&p, &crate::theme::tests::OR_METRICS, |ui| {
            outline_button(ui, "Browse…");
        });
        assert_eq!(color_of(&ink, "Browse…"), p.ink);
        // Hovered, the text is ink on the line fill whatever the setting.
        assert_eq!(button_text(&p, &Metrics::DEFAULT, true), p.ink);
    }

    #[test]
    fn metrics_size_chips_fields_and_cards() {
        let p = Palette::default();
        let draw = |m: Metrics| {
            texts(&p, &m, |ui| {
                ui.horizontal(|ui| {
                    field(ui, "Label");
                    let mut v = 1u8;
                    chips(ui, &[(1u8, "A"), (2, "B")], &mut v, None);
                });
                let mut v = 1u8;
                chips_sized(
                    ui,
                    &[Chip::new(1u8, "C"), Chip::new(2, "D")],
                    &mut v,
                    ChipSize::Compact,
                );
                look_card(
                    ui,
                    vec2(200.0, 100.0),
                    &LookCard {
                        title: "Title",
                        note: "Note",
                        ..LookCard::default()
                    },
                );
            })
        };
        let sdk = draw(Metrics::DEFAULT);
        let scr = draw(crate::theme::tests::SCR_METRICS);
        // SCR's field label is 10 points wider; its chips 2 points
        // narrower, so the centred "A" lands 10 - 1 points further right
        // and B 2 points closer to A.
        let a = rect_of(&scr, "A").left() - rect_of(&sdk, "A").left();
        assert!((a - 9.0).abs() < 0.5, "{a}");
        let gap = |t: &[(String, Color32, Rect)]| rect_of(t, "B").left() - rect_of(t, "A").left();
        assert!((gap(&sdk) - gap(&scr) - 2.0).abs() < 0.5);
        // Compact chips sit closer: 14 + 5 instead of 24 + 6.
        let compact = rect_of(&sdk, "D").left() - rect_of(&sdk, "C").left();
        assert!(compact < gap(&sdk) - 8.0, "{compact} vs {}", gap(&sdk));
        // Card typography: SCR's title is smaller and its note higher.
        assert!(rect_of(&scr, "Title").height() < rect_of(&sdk, "Title").height());
        let note =
            |t: &[(String, Color32, Rect)]| rect_of(t, "Note").top() - rect_of(t, "Title").top();
        assert!(note(&scr) < note(&sdk));
    }

    #[test]
    fn toggle_labels_wrap_only_when_asked() {
        let long = "A very long switch label that cannot fit into a narrow column at all";
        let lines = |m: Metrics| {
            let ctx = egui::Context::default();
            theme::install_with(&ctx, &Palette::default(), &m);
            let mut height = 0.0;
            for _ in 0..2 {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 600.0))),
                        ..Default::default()
                    },
                    |ui| {
                        let mut on = false;
                        height = toggle(ui, &mut on, long, None).rect.height();
                    },
                )
                .textures_delta
                .clear();
            }
            height
        };
        assert!(lines(Metrics::DEFAULT) > 30.0, "wraps to several lines");
        assert_eq!(
            lines(Metrics {
                toggle_wrap: false,
                ..Metrics::DEFAULT
            }),
            30.0
        );
    }

    #[test]
    fn game_card_dims_a_missing_game() {
        let p = crate::theme::tests::OR;
        let texts = texts(&p, &Metrics::DEFAULT, |ui| {
            for (title, subtitle, installed) in [("One", "1995", true), ("Two", "1996", false)] {
                game_card(
                    ui,
                    vec2(180.0, 64.0),
                    &GameCard {
                        title,
                        subtitle,
                        selected: installed,
                        installed,
                        disabled: None,
                    },
                );
            }
        });
        assert_eq!(color_of(&texts, "One"), p.select);
        assert_eq!(color_of(&texts, "1995"), p.muted);
        assert_eq!(color_of(&texts, "Two"), p.ink);
        assert_eq!(color_of(&texts, "1996"), p.disabled_ink);
    }

    #[test]
    fn sentences_join_with_one_full_stop() {
        // SCR: a note ending in "." plus a reason gave "look.. Needs".
        assert_eq!(
            join_sentences("The game's look.", "Needs DXVK"),
            "The game's look. Needs DXVK"
        );
        assert_eq!(
            join_sentences("Buttons", "Needs axes"),
            "Buttons. Needs axes"
        );
        assert_eq!(join_sentences("Really?", "Yes."), "Really? Yes.");
        assert_eq!(join_sentences("", "Needs DXVK"), "Needs DXVK");
        assert_eq!(join_sentences("Note.", ""), "Note.");
        assert_eq!(join_sentences("Note. ", " why"), "Note. why");
    }

    #[test]
    fn disabled_reasons_name_the_chips_unless_all_share_one() {
        let all = [
            Chip::new(1, "a").disabled(Some("Needs DXVK.")),
            Chip::new(2, "b").disabled(Some("Needs DXVK.")),
        ];
        assert_eq!(disabled_reasons(&all), ["Needs DXVK."]);
        let some = [
            Chip::new(0, "Off"),
            Chip::new(1, "Bloom").disabled(Some("Needs Vulkan on.")),
            Chip::new(2, "CRT").disabled(Some("Needs Vulkan on.")),
            Chip::new(3, "Glide").disabled(Some("Needs the 3dfx build.")),
        ];
        assert_eq!(
            disabled_reasons(&some),
            [
                "Bloom, CRT: Needs Vulkan on.",
                "Glide: Needs the 3dfx build."
            ]
        );
        assert!(disabled_reasons(&[Chip::new(0, "Off")]).is_empty());
        assert!(disabled_reasons(&[Chip::new(0, "Off").disabled(Some(""))]).is_empty());
    }

    #[test]
    fn disabled_controls_say_why_where_a_pad_player_looks() {
        let p = crate::theme::tests::OR;
        let texts = texts(&p, &Metrics::DEFAULT, |ui| {
            let mut fps = 60u32;
            chips(
                ui,
                &[(60u32, "60"), (120, "120")],
                &mut fps,
                Some("Needs DXVK."),
            );
            let mut on = true;
            toggle(ui, &mut on, "Rumble", Some("Plug in a pad first."));
            look_card(
                ui,
                vec2(220.0, 100.0),
                &LookCard {
                    title: "Custom",
                    note: "Your own buttons.",
                    disabled: Some("Turn the pad on first."),
                    ..LookCard::default()
                },
            );
            game_card(
                ui,
                vec2(220.0, 64.0),
                &GameCard {
                    title: "Screamer 2",
                    subtitle: "1996",
                    disabled: Some("Not in this folder."),
                    ..GameCard::default()
                },
            );
        });
        // Drawn on screen, in a colour that passes AA on the base.
        for reason in [
            "Needs DXVK.",
            "Plug in a pad first.",
            "Turn the pad on first.",
            "Not in this folder.",
        ] {
            assert_eq!(color_of(&texts, reason), p.muted, "{reason}");
        }
        // The reason sits under the switch's label, not beside it.
        assert!(rect_of(&texts, "Plug in a pad first.").top() > rect_of(&texts, "Rumble").bottom());
        // The card's reason replaces its note (still in the description).
        assert!(
            texts
                .iter()
                .all(|(t, ..)| t != "Your own buttons." && t != "1996")
        );
    }

    #[test]
    fn a_disabled_toggle_keeps_showing_the_saved_value() {
        let knob_x = |on: bool| {
            let ctx = egui::Context::default();
            theme::install(&ctx, &Palette::default());
            let mut x = 0.0;
            for _ in 0..2 {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let mut on = on;
                    toggle(ui, &mut on, "Saved", Some("Locked."));
                });
                output.textures_delta.clear();
                for clipped in output.shapes {
                    if let Shape::Circle(circle) = clipped.shape {
                        x = circle.center.x;
                    }
                }
            }
            x
        };
        // On stays on the right while disabled (v0.3 drew it off).
        assert!(knob_x(true) > knob_x(false) + 10.0);
    }

    #[test]
    fn focused_labels_use_the_focus_colour() {
        let p = crate::theme::tests::OR;
        let texts = texts(&p, &Metrics::DEFAULT, |ui| {
            field_focused(ui, "On", true);
            field_focused(ui, "Off", false);
        });
        assert_eq!(color_of(&texts, "On"), p.focus);
        assert_eq!(color_of(&texts, "Off"), p.ink);
    }
}
