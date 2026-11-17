//! The hub's painted widgets: text, cards, chips, buttons, the route
//! shield, the padlock, meters, a text field. Everything is painted, so
//! each control and each line a player needs to read is also named for
//! assistive technology ([`Semantics`]); the headless tests and the golden
//! AccessKit dumps read the window through that tree.

use eframe::egui::accesskit::{self, Role, Toggled};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Painter, Pos2, Rect, Response,
    Sense, Shape, Stroke, StrokeKind, TextureHandle, Vec2, pos2, vec2,
};
use rea_launcher::wheel::Kind;

use super::theme::{HubTheme, alpha, body, display_semi, semibold, size};
use crate::widgets::{self, Fade};

/// What a painted control or line of text is, for assistive technology.
#[derive(Debug, Clone, Copy)]
pub struct Semantics<'a> {
    pub role: Role,
    pub label: &'a str,
    pub description: Option<&'a str>,
    /// A text field's text, a row's value.
    pub value: Option<&'a str>,
    pub disabled: bool,
    /// A tile or tab that is the current one.
    pub selected: Option<bool>,
    /// A meter's level.
    pub number: Option<f64>,
}

impl<'a> Semantics<'a> {
    pub fn new(role: Role, label: &'a str) -> Semantics<'a> {
        Semantics {
            role,
            label,
            description: None,
            value: None,
            disabled: false,
            selected: None,
            number: None,
        }
    }
    pub fn described(mut self, d: &'a str) -> Self {
        self.description = Some(d);
        self
    }
    pub fn disabled(mut self, off: bool) -> Self {
        self.disabled = off;
        self
    }
    pub fn selected(mut self, on: bool) -> Self {
        self.selected = Some(on);
        self
    }
    pub fn valued(mut self, v: &'a str) -> Self {
        self.value = Some(v);
        self
    }
    pub fn level(mut self, n: f64) -> Self {
        self.number = Some(n);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Outline,
}

/// A button's words and state.
#[derive(Debug, Clone, Copy)]
pub struct Button<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub style: ButtonStyle,
    pub enabled: bool,
    pub focused: bool,
    /// Why it is off (only said while it is).
    pub why_not: Option<&'a str>,
}

/// The hub's pen: the window's `Ui` (for the accessibility tree), a
/// painter and the theme.
#[derive(Clone, Copy)]
pub struct Pen<'a> {
    pub ui: &'a egui::Ui,
    pub p: &'a Painter,
    pub t: &'a HubTheme,
}

/// "◀ ▶" is drawn: Barlow has no such glyphs.
pub const ARROWS: &str = "◀ ▶";

impl<'a> Pen<'a> {
    /// A stable id for a part of the window.
    pub fn id(&self, what: impl std::hash::Hash + std::fmt::Debug) -> Id {
        Id::unique(("hub", self.t.app_id, what))
    }

    /// Makes `r` a widget with `id` and describes it. Text is a
    /// `Role::Label` (AccessKit keeps its words in the value).
    pub fn semantic(&self, id: Id, r: Rect, sense: Sense, s: Semantics) -> Response {
        let resp = self.ui.interact(r, id, sense);
        self.ui.ctx().accesskit_node_builder(resp.id, |b| {
            b.set_role(s.role);
            if s.role == Role::Label {
                b.set_value(s.label);
            } else {
                b.set_label(s.label);
                if let Some(v) = s.value {
                    b.set_value(v);
                }
            }
            if let Some(d) = s.description {
                b.set_description(d);
            }
            if let Some(n) = s.number {
                b.set_numeric_value(n);
            }
            if s.disabled {
                b.set_disabled();
            }
            if let Some(on) = s.selected {
                b.set_toggled(if on { Toggled::True } else { Toggled::False });
            }
            if sense.senses_click() && !s.disabled {
                b.add_action(accesskit::Action::Click);
            }
        });
        resp
    }

    /// Names a painted line of text.
    pub fn say(&self, key: &str, r: Rect, words: &str) {
        if !words.is_empty() {
            self.semantic(
                self.id(("say", key)),
                r,
                Sense::hover(),
                Semantics::new(Role::Label, words),
            );
        }
    }

    pub fn text(&self, pos: Pos2, align: Align2, s: &str, font: FontId, color: Color32) -> Rect {
        self.p.text(pos, align, s, font, color)
    }

    /// Text wrapped to `width`, top-left at `pos`; returns its rect.
    pub fn wrapped(&self, pos: Pos2, s: &str, font: FontId, color: Color32, width: f32) -> Rect {
        let g = self.p.layout(s.to_string(), font, color, width);
        let r = Rect::from_min_size(pos, g.size());
        self.p.galley(pos, g, color);
        r
    }

    pub fn width(&self, s: &str, font: FontId) -> f32 {
        self.p
            .layout_no_wrap(s.to_string(), font, self.t.ink)
            .size()
            .x
    }

    /// `s` cut with "…" to fit `max`.
    pub fn ellipsize(&self, s: &str, font: FontId, max: f32) -> String {
        if self.width(s, font.clone()) <= max {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let t: String = chars.iter().collect::<String>() + "…";
            if self.width(&t, font.clone()) <= max {
                return t;
            }
        }
        "…".into()
    }

    /// One line, cut to `max`, top-left at `pos`.
    pub fn ellipsized(&self, pos: Pos2, s: &str, font: FontId, color: Color32, max: f32) -> Rect {
        let t = self.ellipsize(s, font.clone(), max);
        self.text(pos, Align2::LEFT_TOP, &t, font, color)
    }

    pub fn focus_ring(&self, r: Rect, radius: u8) {
        self.p.rect_stroke(
            r.expand(4.0),
            CornerRadius::same(radius.saturating_add(4)),
            Stroke::new(3.0, self.t.lamp),
            StrokeKind::Outside,
        );
    }

    /// `tex` covering `r`, centred, cropped on its long side.
    pub fn picture(&self, tex: &TextureHandle, r: Rect, tint: Color32) {
        self.p
            .image(tex.id(), r, cover_uv(tex.size_vec2(), r), tint);
    }

    pub fn gradient(&self, r: Rect, color: Color32, fade: Fade) {
        widgets::gradient(self.p, r, color, fade);
    }

    /// A route shield, as on road signs: the sign colour, an ink rim, a
    /// flat top, a point at the bottom. Returns its rect.
    pub fn shield(&self, left_bottom: Pos2, label: &str, font_size: f32) -> Rect {
        let font = display_semi(font_size);
        let w = self.width(label, font.clone()).max(font_size * 0.8) + font_size;
        let h = font_size * 1.75;
        let r = Rect::from_min_size(pos2(left_bottom.x, left_bottom.y - h), vec2(w, h));
        let point = h * 0.22;
        let pts = |r: Rect| {
            vec![
                r.left_top(),
                r.right_top(),
                pos2(r.right(), r.bottom() - point),
                pos2(r.center().x, r.bottom()),
                pos2(r.left(), r.bottom() - point),
            ]
        };
        self.p
            .add(Shape::convex_polygon(pts(r), self.t.ink, Stroke::NONE));
        self.p.add(Shape::convex_polygon(
            pts(r.shrink(2.0)),
            self.t.sign,
            Stroke::NONE,
        ));
        self.text(
            pos2(r.center().x, r.top() + (h - point) / 2.0 + 1.0),
            Align2::CENTER_CENTER,
            label,
            font,
            self.t.ink,
        );
        r
    }

    /// A key cap and what it does, for the footer; returns the next x.
    pub fn hint(&self, x: f32, cy: f32, key: &str, what: &str) -> f32 {
        let kf = semibold(size::NOTE);
        let arrows = key == ARROWS;
        let kw = if arrows {
            44.0
        } else {
            self.width(key, kf.clone()) + 14.0
        };
        let kr = Rect::from_center_size(pos2(x + kw / 2.0, cy), vec2(kw, 22.0));
        self.p.rect_stroke(
            kr,
            CornerRadius::same(4),
            Stroke::new(1.5, self.t.line),
            StrokeKind::Inside,
        );
        if arrows {
            let c = kr.center();
            for (dx, dir) in [(-9.0, -1.0), (9.0, 1.0)] {
                self.triangle(pos2(c.x + dx, c.y), dir, 4.5, self.t.ink);
            }
        } else {
            self.text(kr.center(), Align2::CENTER_CENTER, key, kf, self.t.ink);
        }
        let r = self.text(
            pos2(kr.right() + 8.0, cy),
            Align2::LEFT_CENTER,
            what,
            body(size::NOTE + 1.0),
            self.t.ink,
        );
        r.right() + 26.0
    }

    /// A triangle pointing left (`dir` -1) or right (+1).
    pub fn triangle(&self, c: Pos2, dir: f32, s: f32, color: Color32) {
        let pts = vec![
            pos2(c.x + dir * s, c.y),
            pos2(c.x - dir * s * 0.6, c.y - s),
            pos2(c.x - dir * s * 0.6, c.y + s),
        ];
        self.p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    }

    pub fn button(&self, r: Rect, b: Button) -> Response {
        let t = self.t;
        let mut sem = Semantics::new(Role::Button, b.label).disabled(!b.enabled);
        if let Some(why) = b.why_not.filter(|_| !b.enabled) {
            sem = sem.described(why);
        }
        let resp = self.semantic(self.id(("button", b.id)), r, Sense::click(), sem);
        let hovered = b.enabled && resp.hovered();
        let radius = CornerRadius::same(6);
        let font = display_semi(size::BUTTON);
        let label = |color| {
            self.text(
                r.center(),
                Align2::CENTER_CENTER,
                b.label,
                font.clone(),
                color,
            )
        };
        match (b.style, b.enabled) {
            (ButtonStyle::Primary, true) => {
                let fill = if hovered { t.lamp_hover } else { t.lamp };
                self.p.rect_filled(r, radius, fill);
                label(t.base);
            }
            (ButtonStyle::Outline, true) => {
                self.p.rect_filled(r, radius, alpha(t.base, 200));
                let stroke = if hovered { t.ink } else { t.line };
                self.p
                    .rect_stroke(r, radius, Stroke::new(2.0, stroke), StrokeKind::Inside);
                label(t.ink);
            }
            (_, false) => {
                self.p
                    .rect_stroke(r, radius, Stroke::new(2.0, t.disabled), StrokeKind::Inside);
                label(t.disabled);
            }
        }
        if b.focused {
            self.focus_ring(r, 6);
        }
        if b.enabled {
            resp.on_hover_cursor(CursorIcon::PointingHand)
        } else {
            resp
        }
    }

    pub fn card(&self, r: Rect) {
        self.p
            .rect_filled(r, CornerRadius::same(8), alpha(self.t.panel, 245));
        self.p.rect_stroke(
            r,
            CornerRadius::same(8),
            Stroke::new(1.0, alpha(self.t.line, 90)),
            StrokeKind::Inside,
        );
    }

    /// A card's heading on a sign plate; returns the plate's bottom.
    pub fn card_heading(&self, r: Rect, title: &str) -> f32 {
        let plate = Rect::from_min_size(r.min, vec2(r.width(), 40.0));
        let radius = CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        };
        self.p.rect_filled(plate, radius, self.t.sign);
        self.text(
            pos2(plate.left() + 20.0, plate.center().y),
            Align2::LEFT_CENTER,
            title,
            semibold(size::LABEL),
            self.t.ink,
        );
        plate.bottom()
    }

    /// A chip: filled, or outlined when `fill` is `None`.
    pub fn chip(&self, pos: Pos2, label: &str, fill: Option<Color32>, color: Color32) -> Rect {
        let font = semibold(size::BODY);
        let w = self.width(label, font.clone()) + 24.0;
        let r = Rect::from_min_size(pos, vec2(w, 28.0));
        match fill {
            Some(f) => {
                self.p.rect_filled(r, CornerRadius::same(4), f);
            }
            None => {
                self.p.rect_stroke(
                    r,
                    CornerRadius::same(4),
                    Stroke::new(1.5, self.t.line),
                    StrokeKind::Inside,
                );
            }
        }
        self.text(r.center(), Align2::CENTER_CENTER, label, font, color);
        r
    }

    /// A small padlock (the "Locked" badge, a row that is off).
    pub fn padlock(&self, c: Pos2, s: f32, color: Color32) {
        let body = Rect::from_center_size(pos2(c.x, c.y + s * 0.2), vec2(s, s * 0.7));
        self.p.rect_filled(body, CornerRadius::same(2), color);
        let r = s * 0.3;
        let top = pos2(c.x, body.top());
        let pts: Vec<Pos2> = (0..=12)
            .map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 12.0);
                pos2(top.x - r * a.cos(), top.y - r * a.sin() * 1.1)
            })
            .collect();
        self.p.add(Shape::line(pts, Stroke::new(s * 0.14, color)));
    }

    /// A level bar: 0…1, or -1…1 from the centre.
    pub fn bar(&self, r: Rect, v: f32, centred: bool) {
        let t = self.t;
        self.p.rect_filled(r, CornerRadius::same(4), t.base);
        self.p.rect_stroke(
            r,
            CornerRadius::same(4),
            Stroke::new(1.5, t.line),
            StrokeKind::Inside,
        );
        let inner = r.shrink(3.0);
        let fill = if centred {
            let c = inner.center().x;
            let x = c + v.clamp(-1.0, 1.0) * inner.width() / 2.0;
            Rect::from_min_max(pos2(c.min(x), inner.top()), pos2(c.max(x), inner.bottom()))
        } else {
            Rect::from_min_size(
                inner.min,
                vec2(inner.width() * v.clamp(0.0, 1.0), inner.height()),
            )
        };
        self.p.rect_filled(fill, CornerRadius::same(2), t.lamp);
        if centred {
            self.p.line_segment(
                [
                    pos2(r.center().x, r.top() - 3.0),
                    pos2(r.center().x, r.bottom() + 3.0),
                ],
                Stroke::new(2.0, t.ink),
            );
        }
    }

    /// A wheel, a pad, or a crossed-out pad (no controller).
    pub fn device_glyph(&self, c: Pos2, kind: Option<Kind>) {
        let t = self.t;
        let st = Stroke::new(2.0, t.ink);
        let pad = Rect::from_center_size(c, vec2(20.0, 12.0));
        match kind {
            Some(Kind::Wheel) => {
                self.p.circle_stroke(c, 8.0, st);
                self.p.circle_filled(c, 2.5, t.ink);
                self.p
                    .line_segment([pos2(c.x - 8.0, c.y), pos2(c.x + 8.0, c.y)], st);
            }
            Some(_) => {
                self.p
                    .rect_stroke(pad, CornerRadius::same(5), st, StrokeKind::Inside);
                self.p.circle_filled(pos2(c.x + 4.5, c.y), 1.6, t.ink);
                self.p.line_segment(
                    [pos2(c.x - 7.0, c.y), pos2(c.x - 2.0, c.y)],
                    Stroke::new(1.5, t.ink),
                );
            }
            None => {
                self.p.rect_stroke(
                    pad,
                    CornerRadius::same(5),
                    Stroke::new(2.0, t.muted),
                    StrokeKind::Inside,
                );
                self.p.line_segment(
                    [pos2(c.x - 10.0, c.y + 8.0), pos2(c.x + 10.0, c.y - 8.0)],
                    st,
                );
            }
        }
    }

    /// A one-line field: the text, its end kept in view, and a caret. The
    /// window edits the text itself ([`edit_text`]). Returns its response.
    pub fn text_field(&self, field: Rect, f: TextField) -> Response {
        let t = self.t;
        self.p.rect_filled(field, CornerRadius::same(6), t.panel);
        let resp = self
            .semantic(
                self.id(f.id),
                field,
                Sense::click(),
                Semantics::new(Role::TextInput, f.label).valued(f.text),
            )
            .on_hover_cursor(CursorIcon::Text);
        let font = body(size::LABEL);
        let inner = field.shrink2(vec2(16.0, 0.0));
        if f.text.is_empty() {
            self.text(
                inner.left_center(),
                Align2::LEFT_CENTER,
                f.placeholder,
                font.clone(),
                t.muted,
            );
        }
        let g = self.p.layout_no_wrap(f.text.to_string(), font, t.ink);
        let tw = g.size().x;
        let tx = inner.left() - (tw - inner.width() + 6.0).max(0.0);
        self.p.with_clip_rect(inner).galley(
            pos2(tx, inner.center().y - g.size().y / 2.0),
            g,
            t.ink,
        );
        let blink = self.ui.input(|i| i.time).fract() < 0.6;
        if f.focused && blink {
            let cx = (tx + tw + 2.0).min(inner.right());
            self.p.line_segment(
                [
                    pos2(cx, inner.center().y - 11.0),
                    pos2(cx, inner.center().y + 11.0),
                ],
                Stroke::new(2.0, t.lamp),
            );
        }
        let edge = if f.focused { t.ink } else { t.line };
        self.p.rect_stroke(
            field,
            CornerRadius::same(6),
            Stroke::new(2.0, edge),
            StrokeKind::Inside,
        );
        if f.focused {
            self.focus_ring(field, 6);
        }
        resp
    }
}

/// A painted one-line text field.
#[derive(Debug, Clone, Copy)]
pub struct TextField<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub text: &'a str,
    pub placeholder: &'a str,
    pub focused: bool,
}

/// Applies typed text, pastes and Backspace to `s`; Ctrl+Backspace drops a
/// whole path part (after `/` or `\`). Returns whether `s` changed.
pub fn edit_text(s: &mut String, events: &[egui::Event]) -> bool {
    let before = s.clone();
    for e in events {
        match e {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                s.extend(t.chars().filter(|c| !c.is_control()));
            }
            egui::Event::Key {
                key: egui::Key::Backspace,
                pressed: true,
                modifiers,
                ..
            } => {
                if modifiers.ctrl {
                    let sep = ['/', '\\'];
                    let keep = s.trim_end_matches(sep).rfind(sep).map_or(0, |i| i + 1);
                    s.truncate(keep);
                } else {
                    s.pop();
                }
            }
            _ => {}
        }
    }
    *s != before
}

/// The part of a picture of size `img` that covers `r`, centred.
pub fn cover_uv(img: Vec2, r: Rect) -> Rect {
    let ia = img.x / img.y.max(1.0);
    let ra = r.width() / r.height().max(1.0);
    if ra > ia {
        let h = ia / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    } else {
        let w = ra / ia;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_fit_crops_the_long_side_and_centres() {
        let wide = Rect::from_min_size(Pos2::ZERO, vec2(2000.0, 500.0));
        let uv = cover_uv(vec2(1920.0, 1080.0), wide);
        assert!((uv.width() - 1.0).abs() < 1e-6);
        assert!(uv.height() < 1.0 && (uv.center().y - 0.5).abs() < 1e-6);
        let tall = Rect::from_min_size(Pos2::ZERO, vec2(500.0, 1000.0));
        let uv = cover_uv(vec2(1920.0, 1080.0), tall);
        assert!((uv.height() - 1.0).abs() < 1e-6 && uv.width() < 1.0);
        let same = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        assert!((cover_uv(vec2(1920.0, 1080.0), same).width() - 1.0).abs() < 1e-4);
    }

    #[test]
    fn the_text_fields_take_text_paste_and_backspace() {
        let key = |ctrl| egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers {
                ctrl,
                ..Default::default()
            },
        };
        let mut s = String::new();
        assert!(edit_text(
            &mut s,
            &[
                egui::Event::Text("/mnt/".into()),
                egui::Event::Paste("games/Teknoparrot\n".into()),
            ]
        ));
        assert_eq!(s, "/mnt/games/Teknoparrot");
        edit_text(&mut s, &[key(false)]);
        assert_eq!(s, "/mnt/games/Teknoparro");
        edit_text(&mut s, &[key(true)]);
        assert_eq!(s, "/mnt/games/");
        edit_text(&mut s, &[key(true)]);
        assert_eq!(s, "/mnt/");
        assert!(!edit_text(&mut s, &[egui::Event::Copy]));
    }

    #[test]
    fn ctrl_backspace_takes_a_windows_path_part_too() {
        let mut s = String::from("C:\\Games\\Teknoparrot\\");
        let ctrl = egui::Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        };
        edit_text(&mut s, std::slice::from_ref(&ctrl));
        assert_eq!(s, "C:\\Games\\");
        edit_text(&mut s, &[ctrl]);
        assert_eq!(s, "C:\\");
    }
}
