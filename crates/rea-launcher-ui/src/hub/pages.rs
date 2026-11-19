//! Drawing the hub's pages: hero, top bar, hub (title, state, buttons,
//! tiles), settings, controls, setup, the extra page, the footer, the
//! running overlay and messages. Laid out for 1280×720 points (zoomed to
//! the window; at 16:10 the canvas is 1152 wide).

use std::sync::atomic::Ordering;

use eframe::egui::accesskit::Role;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Rect, Sense, Stroke, StrokeKind, pos2, vec2,
};
use rea_launcher::hub::{Focus, Input, Item, Page, Tab, TileState, display_path};
use rea_launcher::wheel::Kind;

use super::app::{Click, HubApp};
use super::backend::{HubBackend, HubForm, Mark, Note};
use super::paint::{ARROWS, Button, ButtonStyle, Pen, Semantics, TextField};
use super::theme::{alpha, body, display, semibold, size};
use crate::widgets::Fade;

const MARGIN: f32 = 48.0;
const BAR_H: f32 = 56.0;

fn kind_word(kind: Kind) -> &'static str {
    match kind {
        Kind::Wheel => "Wheel",
        Kind::Pad => "Pad",
        Kind::Other => "Other",
    }
}

impl<B: HubBackend> HubApp<B> {
    pub(crate) fn draw(&mut self, ui: &egui::Ui) -> Vec<Click> {
        let ctx = ui.ctx().clone();
        let r = ui.max_rect();
        let p = ui.painter().clone();
        let t = self.spec.theme;
        let pen = Pen { ui, p: &p, t: &t };
        p.rect_filled(r, CornerRadius::ZERO, t.base);
        let fade = self.fade_step(&ctx);
        self.draw_hero(pen, r, fade);
        let mut clicks: Vec<Click> = Vec::new();
        match self.model.page {
            Page::Hub => self.draw_hub(pen, r, &mut clicks),
            Page::Settings => self.draw_settings(pen, r, &mut clicks),
            Page::Controls => self.draw_controls(pen, r, &mut clicks),
            Page::Setup => self.draw_setup(pen, r, &mut clicks),
            Page::Extra => self.draw_extra(pen, r, &mut clicks),
        }
        if self.model.page.has_fields() {
            self.brand_bar(pen, r);
        } else {
            self.draw_top_bar(pen, r, &mut clicks);
        }
        self.draw_footer(pen, r);
        if self.running.is_some() {
            self.draw_running(pen, r, &mut clicks);
        }
        if self.message.is_some() {
            self.draw_message(pen, r, &mut clicks);
        }
        clicks
    }

    /// How far the hero's cross-fade is (0…1); ends it at 1.
    fn fade_step(&mut self, ctx: &egui::Context) -> f32 {
        let now = ctx.input(|i| i.time);
        let t = match self.fade {
            Some((_, t0)) => ((now - t0) / 0.3).clamp(0.0, 1.0) as f32,
            None => 1.0,
        };
        if t >= 1.0 {
            self.fade = None;
        }
        t
    }

    fn draw_hero(&self, pen: Pen, r: Rect, t: f32) {
        let (p, th) = (pen.p, pen.t);
        if let Some((from, _)) = self.fade
            && let Some(tex) = &self.heroes[from]
        {
            pen.picture(tex, r, Color32::WHITE);
        }
        if let Some(tex) = &self.heroes[self.model.selected] {
            pen.picture(tex, r, Color32::WHITE.gamma_multiply(t));
        }
        if self.model.page != Page::Hub {
            p.rect_filled(r, CornerRadius::ZERO, alpha(th.base, 225));
            return;
        }
        // a solid backing behind the text column, fading to the picture
        let w = r.width();
        let solid = alpha(th.base, th.scrim);
        let col = Rect::from_min_max(r.min, pos2(r.left() + w * 0.40, r.bottom()));
        p.rect_filled(col, CornerRadius::ZERO, solid);
        pen.gradient(
            Rect::from_min_max(
                pos2(col.right(), r.top()),
                pos2(r.left() + w * 0.78, r.bottom()),
            ),
            solid,
            Fade::Left,
        );
        let full = r.top() + 520.0;
        pen.gradient(
            Rect::from_min_max(pos2(r.left(), r.top() + 470.0), pos2(r.right(), full)),
            solid,
            Fade::Down,
        );
        p.rect_filled(
            Rect::from_min_max(pos2(r.left(), full), r.max),
            CornerRadius::ZERO,
            solid,
        );
    }

    fn brand_bar(&self, pen: Pen, r: Rect) -> Rect {
        let (p, t) = (pen.p, pen.t);
        let bar = Rect::from_min_size(r.min, vec2(r.width(), BAR_H));
        p.rect_filled(bar, CornerRadius::ZERO, t.sign);
        p.rect_filled(
            Rect::from_min_size(pos2(bar.left(), bar.bottom() - 2.0), vec2(bar.width(), 2.0)),
            CornerRadius::ZERO,
            alpha(t.ink, 70),
        );
        let s = pen.shield(
            pos2(bar.left() + MARGIN, bar.center().y + 14.0),
            t.brand,
            size::SHIELD,
        );
        pen.text(
            pos2(s.right() + 12.0, bar.center().y),
            Align2::LEFT_CENTER,
            t.title,
            super::theme::display_semi(size::BRAND),
            t.ink,
        );
        bar
    }

    fn draw_top_bar(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let (p, t) = (pen.p, pen.t);
        let bar = self.brand_bar(pen, r);
        let current = Tab::of(self.model.page).unwrap_or(Tab::Games);
        let mut x = bar.left() + 330.0;
        for tab in Tab::ALL {
            let font = semibold(size::NAV);
            let tw = pen.width(tab.label(), font.clone());
            let tr =
                Rect::from_center_size(pos2(x + tw / 2.0, bar.center().y), vec2(tw + 24.0, 38.0));
            let resp = pen
                .semantic(
                    pen.id(("tab", tab.label())),
                    tr,
                    Sense::click(),
                    Semantics::new(Role::Tab, tab.label()).selected(tab == current),
                )
                .on_hover_cursor(CursorIcon::PointingHand);
            if resp.hovered() {
                p.rect_filled(tr, CornerRadius::same(4), alpha(t.base, 60));
            }
            pen.text(tr.center(), Align2::CENTER_CENTER, tab.label(), font, t.ink);
            if tab == current {
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(tr.left() + 8.0, bar.bottom() - 6.0),
                        vec2(tr.width() - 16.0, 4.0),
                    ),
                    CornerRadius::same(1),
                    t.lamp,
                );
            }
            if self.model.focus == Focus::Tab(tab) {
                p.rect_stroke(
                    tr,
                    CornerRadius::same(4),
                    Stroke::new(3.0, t.lamp),
                    StrokeKind::Outside,
                );
            }
            if resp.clicked() {
                clicks.push(Click::Focus(Focus::Tab(tab), Some(Input::Press)));
            }
            x = tr.right() + 20.0;
        }
        // the controller that drives the car
        let c = &self.controls;
        let (label, kind) = match (&c.error, c.driver.and_then(|d| c.devices.get(d))) {
            (Some(_), _) => ("Controllers unavailable".to_string(), None),
            (None, Some(d)) => (d.name.clone(), Some(d.kind)),
            (None, None) => ("No controller".into(), None),
        };
        let font = body(size::BODY);
        let shown = pen.ellipsize(&label, font.clone(), 300.0);
        let tr = pen.text(
            pos2(bar.right() - MARGIN, bar.center().y),
            Align2::RIGHT_CENTER,
            &shown,
            font,
            t.ink,
        );
        pen.say("driver", tr, &format!("Controller: {label}"));
        pen.device_glyph(pos2(tr.left() - 20.0, bar.center().y), kind);
    }

    fn draw_hub(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let sel = self.model.selected;
        let games = self.backend.games();
        let g = &games[sel];
        let state = self.state(sel);
        let found = self.found(sel);
        let x = r.left() + MARGIN;
        let colw = (r.width() * 0.40 - MARGIN).max(360.0);

        // title, anchored at its bottom
        let title_g = pen.p.layout(
            g.title.to_string(),
            display(size::TITLE),
            t.ink,
            colw + 80.0,
        );
        let title_bottom = r.top() + 330.0;
        let title_at = pos2(x, title_bottom - title_g.size().y);
        pen.say(
            "title",
            Rect::from_min_size(title_at, title_g.size()),
            g.title,
        );
        pen.p.galley(title_at, title_g, t.ink);

        let mut line2 = format!("{} · {}", g.board, g.year);
        if let Some(e) = found.as_ref().and_then(|f| f.edition.as_ref()) {
            line2.push_str(&format!(" · {e}"));
        }
        let meta = pen.text(
            pos2(x + 2.0, title_bottom + 6.0),
            Align2::LEFT_TOP,
            &line2,
            body(size::LABEL),
            t.muted,
        );
        pen.say("meta", meta, &line2);

        let chip_y = r.top() + 372.0;
        let (fill, color) = match state {
            TileState::Ready | TileState::Unverified(_) => (Some(t.sign), t.ink),
            TileState::Blocked(_) => (Some(t.signal), t.ink),
            TileState::Needs(_) => (Some(t.lamp), t.base),
            TileState::NotInLibrary => (None, t.ink),
            TileState::Looking => (None, t.muted),
        };
        let cr = pen.chip(pos2(x, chip_y), state.chip(), fill, color);
        pen.say("state", cr, state.chip());
        let reason = match &state {
            TileState::Blocked(why) => why.to_string(),
            TileState::Unverified(note) => note.to_string(),
            TileState::Needs(need) => need.reason.clone(),
            TileState::NotInLibrary => {
                "Not found in your game folders. Add its folder in Settings, then choose Rescan."
                    .into()
            }
            TileState::Ready | TileState::Looking => String::new(),
        };
        let mut y = chip_y + 38.0;
        if !reason.is_empty() {
            let rr = pen.wrapped(pos2(x, y), &reason, body(size::BODY), t.ink, colw + 40.0);
            pen.say("reason", rr, &reason);
            y = rr.bottom() + 4.0;
        }
        if let Some(f) = &found {
            let line = format!("Folder: {}", display_path(&f.dir));
            let fr = pen.ellipsized(pos2(x, y), &line, body(size::BODY), t.muted, colw + 40.0);
            pen.say("folder", fr, &line);
            y = fr.bottom();
        }
        if let Some(note) = state
            .playable()
            .then(|| self.backend.note(sel, &self.config))
            .flatten()
        {
            let color = if note.error { t.error } else { t.muted };
            let nr = pen.wrapped(
                pos2(x, y + 4.0),
                &note.text,
                body(size::BODY),
                color,
                colw + 40.0,
            );
            pen.say("game-note", nr, &note.text);
            y = nr.bottom();
        }

        // the buttons follow the text above (a two-line title pushes them down)
        let by = (r.top() + 446.0).max(y + 14.0);
        let play_r = Rect::from_min_size(pos2(x, by), vec2(170.0, 50.0));
        let set_r = Rect::from_min_size(pos2(play_r.right() + 16.0, by), vec2(170.0, 50.0));
        let needs = matches!(state, TileState::Needs(_));
        let enabled = (state.playable() || needs) && self.running.is_none();
        let why_not = match &state {
            TileState::Blocked(why) => Some(*why),
            TileState::NotInLibrary => Some("Not found in your game folders."),
            TileState::Looking => Some("Still looking for your games."),
            _ => None,
        };
        let label = match &state {
            TileState::Needs(need) => need.action,
            _ => "Play",
        };
        let play = Button {
            id: "play",
            label,
            style: ButtonStyle::Primary,
            enabled,
            focused: self.model.focus == Focus::Play,
            why_not,
        };
        if pen.button(play_r, play).clicked() && enabled {
            clicks.push(Click::Focus(Focus::Play, Some(Input::Press)));
        }
        let settings = Button {
            id: "game-settings",
            label: "Settings",
            style: ButtonStyle::Outline,
            enabled: true,
            focused: self.model.focus == Focus::GameSettings,
            why_not: None,
        };
        if pen.button(set_r, settings).clicked() {
            clicks.push(Click::Focus(Focus::GameSettings, Some(Input::Press)));
        }
        self.draw_tiles(pen, r, clicks);
    }

    fn draw_tiles(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let (p, t) = (pen.p, pen.t);
        let games = self.backend.games();
        let sel = self.model.selected;
        let x = r.left() + MARGIN;
        let row_label = match &self.library {
            None => "Your games · looking…".to_string(),
            Some(_) => format!(
                "Your games · {} of {} found",
                self.found_count(),
                games.len()
            ),
        };
        let rl = pen.text(
            pos2(x, r.top() + 528.0),
            Align2::LEFT_BOTTOM,
            &row_label,
            semibold(size::BODY),
            t.muted,
        );
        pen.say("found", rl, &row_label);
        let gap = 10.0;
        let n = games.len() as f32;
        let spread = (r.width() - 2.0 * MARGIN - gap * (n - 1.0)) / n;
        let tw = self
            .spec
            .layout
            .tile_max_width
            .map_or(spread, |max| spread.min(max));
        let th = tw * 9.0 / 16.0;
        let ty = r.top() + 540.0;
        for (i, game) in games.iter().enumerate() {
            let tr = Rect::from_min_size(pos2(x + i as f32 * (tw + gap), ty), vec2(tw, th));
            let st = self.state(i);
            let dim = matches!(st, TileState::Blocked(_) | TileState::NotInLibrary);
            let about = format!("{}, {}, {}", game.year, game.board, st.chip());
            let resp = pen
                .semantic(
                    pen.id(("tile", i)),
                    tr,
                    Sense::click(),
                    Semantics::new(Role::RadioButton, game.title)
                        .described(&about)
                        .selected(i == sel),
                )
                .on_hover_cursor(CursorIcon::PointingHand);
            let radius = CornerRadius::same(6);
            p.rect_filled(tr, radius, t.panel);
            if let Some(tex) = &self.tiles[i] {
                let tint = if dim {
                    Color32::from_gray(95)
                } else if resp.hovered() || i == sel {
                    Color32::WHITE
                } else {
                    Color32::from_gray(200)
                };
                pen.picture(tex, tr, tint);
            }
            p.rect_stroke(
                tr,
                radius,
                Stroke::new(1.0, alpha(t.line, 120)),
                StrokeKind::Inside,
            );
            pen.shield(
                pos2(tr.left() + 6.0, tr.bottom() - 6.0),
                game.short,
                size::SHIELD,
            );
            if let Some(badge) = st.badge() {
                self.draw_badge(pen, tr, &st, badge);
            }
            if i == sel {
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(tr.left() + tw * 0.3, tr.bottom() + 10.0),
                        vec2(tw * 0.4, 4.0),
                    ),
                    CornerRadius::same(2),
                    t.lamp,
                );
                if self.model.focus == Focus::Tile {
                    pen.focus_ring(tr, 6);
                }
            }
            if resp.clicked() {
                clicks.push(Click::Tile(i));
            }
        }
    }

    /// A tile's badge: an icon and a word (never colour alone).
    fn draw_badge(&self, pen: Pen, tr: Rect, st: &TileState, badge: &str) {
        let (p, t) = (pen.p, pen.t);
        let bf = semibold(size::BADGE);
        let bw = pen.width(badge, bf.clone()) + 26.0;
        let br = Rect::from_min_size(pos2(tr.right() - bw - 5.0, tr.top() + 5.0), vec2(bw, 20.0));
        p.rect_filled(br, CornerRadius::same(3), alpha(t.base, 235));
        let c = pos2(br.left() + 10.0, br.center().y);
        let line = Stroke::new(1.5, t.ink);
        match st {
            TileState::Blocked(_) => pen.padlock(pos2(c.x, c.y - 1.0), 8.0, t.ink),
            TileState::Unverified(_) => {
                p.circle_stroke(c, 4.5, line);
                pen.text(c, Align2::CENTER_CENTER, "?", semibold(8.0), t.ink);
            }
            TileState::Needs(_) => {
                let s = Stroke::new(2.0, t.lamp);
                p.line_segment([c - vec2(4.5, 0.0), c + vec2(4.5, 0.0)], s);
                p.line_segment([c - vec2(0.0, 4.5), c + vec2(0.0, 4.5)], s);
            }
            _ => {
                p.circle_stroke(c, 4.0, line);
                p.line_segment(
                    [pos2(c.x - 3.0, c.y + 3.0), pos2(c.x + 3.0, c.y - 3.0)],
                    line,
                );
            }
        }
        pen.text(
            pos2(br.left() + 19.0, br.center().y),
            Align2::LEFT_CENTER,
            badge,
            bf,
            t.ink,
        );
    }

    fn draw_page_heading(&self, pen: Pen, r: Rect, title: &str, sub: &str) {
        let x = r.left() + MARGIN;
        let tr = pen.text(
            pos2(x, r.top() + BAR_H + 22.0),
            Align2::LEFT_TOP,
            title,
            display(size::PAGE_TITLE),
            pen.t.ink,
        );
        pen.say("heading", tr, title);
        let sr = pen.text(
            pos2(x, r.top() + BAR_H + 66.0),
            Align2::LEFT_TOP,
            sub,
            body(size::BODY),
            pen.t.muted,
        );
        pen.say("sub-heading", sr, sub);
    }

    fn draw_settings(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let g = &self.backend.games()[self.model.selected];
        self.draw_page_heading(
            pen,
            r,
            &format!("{} settings", g.title),
            "Changes are saved as you make them and used the next time you press Play.",
        );
        let Some((_, form)) = &self.form else {
            return;
        };
        let row_h = self.spec.layout.row_height;
        let top = r.top() + 162.0;
        let left = Rect::from_min_max(
            pos2(r.left() + MARGIN, top),
            pos2(
                r.right() - MARGIN - 340.0 - 24.0,
                top + 40.0 + self.rows.len() as f32 * row_h + 8.0,
            ),
        );
        pen.card(left);
        let mut y = pen.card_heading(left, "Picture and controls") + 4.0;
        // the focused row's note when the row has no room for all of it
        let mut cut_note = None;
        for (idx, item) in self.model.items.iter().enumerate() {
            let Item::Option(key) = *item else { continue };
            let Some(spec) = self.rows.iter().find(|s| s.key == key) else {
                continue;
            };
            let rr =
                Rect::from_min_size(pos2(left.left() + 4.0, y), vec2(left.width() - 8.0, row_h));
            y += row_h;
            let off = self.row_off(form, spec);
            let cut = self.draw_row(
                pen,
                rr,
                idx,
                spec.label,
                spec.description,
                &form.value(key),
                off.as_deref(),
                clicks,
            );
            if cut {
                cut_note = Some(off.unwrap_or_else(|| spec.description.to_string()));
            }
        }
        let library = Rect::from_min_max(
            pos2(left.right() + 24.0, top),
            pos2(r.right() - MARGIN, left.bottom()),
        );
        self.draw_library_card(pen, library, cut_note.as_deref(), clicks);
    }

    /// One settings row: label, note (or why it is off), `◀ value ▶`.
    /// Returns whether it is focused and its note didn't fit.
    #[allow(clippy::too_many_arguments)]
    fn draw_row(
        &self,
        pen: Pen,
        rr: Rect,
        idx: usize,
        label: &str,
        description: &str,
        value: &str,
        off: Option<&str>,
        clicks: &mut Vec<Click>,
    ) -> bool {
        let (p, t) = (pen.p, pen.t);
        let layout = self.spec.layout;
        let focused = self.model.focus == Focus::Item(idx);
        let note = off.unwrap_or(description);
        let resp = pen
            .semantic(
                pen.id(("row", idx)),
                rr,
                Sense::click(),
                Semantics::new(Role::Button, label)
                    .valued(value)
                    .described(note)
                    .disabled(off.is_some()),
            )
            .on_hover_cursor(CursorIcon::PointingHand);
        if focused {
            p.rect_filled(rr, CornerRadius::same(4), t.focus_fill);
            p.rect_stroke(
                rr,
                CornerRadius::same(4),
                Stroke::new(3.0, t.lamp),
                StrokeKind::Inside,
            );
        } else if resp.hovered() {
            p.rect_filled(rr, CornerRadius::same(4), alpha(t.ink, 10));
        }
        let lx = rr.left() + 18.0;
        pen.text(
            pos2(lx, rr.top() + layout.row_label_top),
            Align2::LEFT_TOP,
            label,
            semibold(size::LABEL),
            if off.is_some() { t.muted } else { t.ink },
        );
        let valw = 270.0;
        let note_w = rr.width() - valw - 40.0;
        let shown = pen.ellipsize(note, body(size::NOTE), note_w);
        pen.text(
            pos2(lx, rr.top() + layout.row_note_top),
            Align2::LEFT_TOP,
            &shown,
            body(size::NOTE),
            t.muted,
        );
        let cut = focused && shown != note;
        let vr = Rect::from_min_max(
            pos2(rr.right() - valw - 12.0, rr.top()),
            pos2(rr.right() - 12.0, rr.bottom()),
        );
        let vc = if off.is_some() { t.disabled } else { t.ink };
        pen.text(
            vr.center(),
            Align2::CENTER_CENTER,
            value,
            semibold(size::LABEL),
            vc,
        );
        if off.is_some() {
            // "off" shown by shape too: a lock beside the value
            pen.padlock(
                pos2(vr.right() - 12.0, vr.center().y - 1.0),
                9.0,
                t.disabled,
            );
        } else {
            let ac = if focused { t.lamp } else { t.muted };
            for (dir, way, input) in [(-1.0, "previous", Input::Left), (1.0, "next", Input::Right)]
            {
                let x = if dir < 0.0 {
                    vr.left() + 12.0
                } else {
                    vr.right() - 12.0
                };
                let ar = Rect::from_center_size(pos2(x, vr.center().y), vec2(28.0, 40.0));
                pen.triangle(ar.center(), dir, 7.0, ac);
                let a = pen.semantic(
                    pen.id((way, idx)),
                    ar,
                    Sense::click(),
                    Semantics::new(Role::Button, &format!("{label}: {way}")),
                );
                if a.clicked() {
                    clicks.push(Click::Focus(Focus::Item(idx), Some(input)));
                    return cut;
                }
            }
        }
        if resp.clicked() {
            let input = off.is_none().then_some(Input::Press);
            clicks.push(Click::Focus(Focus::Item(idx), input));
        }
        cut
    }

    /// The library card; under its buttons the last save's note, then
    /// `cut_note` (the focused row's whole note when its row cut it).
    fn draw_library_card(
        &self,
        pen: Pen,
        rc: Rect,
        cut_note: Option<&str>,
        clicks: &mut Vec<Click>,
    ) {
        let t = pen.t;
        pen.card(rc);
        let mut y = pen.card_heading(rc, "Your game folders") + 16.0;
        let ix = rc.left() + 20.0;
        let iw = rc.width() - 40.0;
        y = self.draw_folders(pen, ix, y, iw, "Not chosen yet", true) + 8.0;
        let status = match &self.library {
            None => "Looking for your games…".to_string(),
            Some(lib) => {
                let unknown = B::unknown(lib).len();
                let mut s = format!(
                    "{} of {} games found.",
                    self.found_count(),
                    self.backend.games().len()
                );
                if unknown > 0 {
                    s.push_str(&format!(
                        " {unknown} folder{} holds a build this launcher doesn't know; launcher.log has its digest.",
                        if unknown == 1 { "" } else { "s" }
                    ));
                }
                s
            }
        };
        let sr = pen.wrapped(pos2(ix, y), &status, body(size::BODY), t.muted, iw);
        pen.say("library-status", sr, &status);
        y = sr.bottom() + 18.0;
        let n = self.model.items.len();
        let b1 = Rect::from_min_size(pos2(ix, y), vec2(iw, 46.0));
        let b2 = Rect::from_min_size(pos2(ix, y + 60.0), vec2(iw, 46.0));
        let scanning = self.scanning();
        let rescan = Button {
            id: "rescan",
            label: if scanning { "Looking…" } else { "Rescan" },
            style: ButtonStyle::Outline,
            enabled: !scanning,
            focused: self.model.focus == Focus::Item(n - 2),
            why_not: Some("Already looking."),
        };
        if pen.button(b1, rescan).clicked() && !scanning {
            clicks.push(Click::Focus(Focus::Item(n - 2), Some(Input::Press)));
        }
        let folders = Button {
            id: "folder",
            label: self.spec.words.folders_button,
            style: ButtonStyle::Outline,
            enabled: true,
            focused: self.model.focus == Focus::Item(n - 1),
            why_not: None,
        };
        if pen.button(b2, folders).clicked() {
            clicks.push(Click::Focus(Focus::Item(n - 1), Some(Input::Press)));
        }
        let mut y = b2.bottom() + 22.0;
        if let Some(note) = &self.note {
            let color = if note.error { t.error } else { t.ink };
            let nr = pen.wrapped(pos2(ix, y), &note.text, body(size::BODY), color, iw);
            pen.say("note", nr, &note.text);
            y = nr.bottom() + 12.0;
        }
        if let Some(whole) = cut_note {
            let wr = pen.wrapped(pos2(ix, y), whole, body(size::NOTE), t.muted, iw);
            pen.say("row-note", wr, whole);
        }
    }

    /// The library folders (three, then "and n more"); returns the y
    /// below. With none, says `none`: in ink on the settings card
    /// (`in_card`), muted on the setup page.
    fn draw_folders(&self, pen: Pen, x: f32, mut y: f32, w: f32, none: &str, in_card: bool) -> f32 {
        let t = pen.t;
        let folders = &self.config.libraries;
        if folders.is_empty() && in_card {
            let fr = pen.wrapped(pos2(x, y), none, body(size::BODY), t.ink, w);
            pen.say("folders", fr, none);
            y = fr.bottom() + 8.0;
        } else if folders.is_empty() {
            let fr = pen.text(
                pos2(x, y),
                Align2::LEFT_TOP,
                none,
                body(size::BODY),
                t.muted,
            );
            pen.say("folders", fr, none);
            y += 24.0;
        }
        for (k, f) in folders.iter().enumerate().take(3) {
            let line = display_path(f);
            let fr = pen.ellipsized(pos2(x, y), &line, body(size::BODY), t.ink, w);
            pen.say(&format!("folder-{k}"), fr, &line);
            y += 24.0;
        }
        if folders.len() > 3 {
            let more = format!("and {} more", folders.len() - 3);
            let mr = pen.text(
                pos2(x, y),
                Align2::LEFT_TOP,
                &more,
                body(size::BODY),
                t.muted,
            );
            pen.say("folders-more", mr, &more);
            y += 24.0;
        }
        y
    }

    fn draw_controls(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        self.draw_page_heading(
            pen,
            r,
            "Controls",
            "Turn the wheel or move the stick, press the pedals and buttons: the bars should follow.",
        );
        let c = &self.controls;
        let top = r.top() + 162.0;
        let bottom = r.bottom() - 62.0;
        let split = r.left() + MARGIN + (r.width() - 2.0 * MARGIN - 24.0) * 0.46;

        // devices
        let dev = Rect::from_min_max(pos2(r.left() + MARGIN, top), pos2(split, top + 196.0));
        pen.card(dev);
        let mut y = pen.card_heading(dev, "Your controllers") + 14.0;
        let ix = dev.left() + 20.0;
        let iw = dev.width() - 40.0;
        if let Some(e) = &c.error {
            let msg = format!(
                "Controllers can't be read: {e}. Plug the controller in again, then restart the launcher."
            );
            let mr = pen.wrapped(pos2(ix, y), &msg, body(size::BODY), t.error, iw);
            pen.say("devices-error", mr, &msg);
        } else if c.devices.is_empty() {
            let msg = "No controller found. Plug in a wheel or a pad; it shows up here.";
            let mr = pen.wrapped(pos2(ix, y), msg, body(size::BODY), t.ink, iw);
            pen.say("devices-none", mr, msg);
        }
        for (i, d) in c.devices.iter().enumerate().take(4) {
            let kind = kind_word(d.kind);
            let k = pen.chip(pos2(ix, y), kind, None, t.ink);
            let mut right = dev.right() - 20.0;
            let drives = c.driver == Some(i);
            if drives {
                let label = "Drives the car";
                let w = pen.width(label, semibold(size::BODY)) + 24.0;
                pen.chip(pos2(right - w, y), label, Some(t.sign), t.ink);
                right -= w + 10.0;
            }
            let name = pen.ellipsize(&d.name, body(size::BODY), right - k.right() - 14.0);
            let nr = pen.text(
                pos2(k.right() + 12.0, k.center().y),
                Align2::LEFT_CENTER,
                &name,
                body(size::BODY),
                t.ink,
            );
            let words = format!(
                "{kind}: {}{}",
                d.name,
                if drives { " (drives the car)" } else { "" }
            );
            pen.say(&format!("device-{i}"), nr.union(k), &words);
            y += 38.0;
        }

        // the pad layout
        let lay = Rect::from_min_max(
            pos2(r.left() + MARGIN, dev.bottom() + 16.0),
            pos2(split, bottom),
        );
        pen.card(lay);
        let y = pen.card_heading(lay, "Pad layout") + 10.0;
        let ix = lay.left() + 20.0;
        let colw = (lay.width() - 40.0) / 2.0;
        let rebindable = self.backend.rebindable();
        for (k, (what, keys)) in c.layout.iter().enumerate() {
            let at = pos2(ix + (k / 6) as f32 * colw, y + (k % 6) as f32 * 28.0);
            let binding = rebindable.iter().position(|&row| row == k);
            let waiting = binding.is_some() && self.capture.as_ref().map(|c| c.n) == binding;
            let shown = if waiting { "?" } else { keys.as_str() };
            let kf = semibold(size::NOTE);
            let kw = pen.width(shown, kf.clone()) + 14.0;
            let kr = Rect::from_min_size(at, vec2(kw.max(36.0), 22.0));
            let (stroke, fill) = if waiting {
                (Stroke::new(2.0, t.lamp), Some(t.lamp))
            } else {
                (Stroke::new(1.5, t.line), None)
            };
            if let Some(f) = fill {
                pen.p.rect_filled(kr, CornerRadius::same(4), f);
            }
            pen.p
                .rect_stroke(kr, CornerRadius::same(4), stroke, StrokeKind::Inside);
            let ink = if waiting { t.base } else { t.ink };
            pen.text(kr.center(), Align2::CENTER_CENTER, shown, kf, ink);
            let wr = pen.ellipsized(
                pos2(kr.right() + 10.0, at.y + 2.0),
                what,
                body(size::BODY),
                t.ink,
                colw - kr.width() - 18.0,
            );
            let row = wr.union(kr);
            match binding {
                Some(j) => {
                    let words = if waiting {
                        format!("{what}: press the new button")
                    } else {
                        format!("{what}: {keys}. Change")
                    };
                    let hit = Rect::from_min_size(at, vec2(colw - 12.0, 24.0));
                    let resp = pen.semantic(
                        pen.id(("binding", k)),
                        hit,
                        Sense::click(),
                        Semantics::new(Role::Button, &words),
                    );
                    if resp.hovered() {
                        pen.ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        clicks.push(Click::Focus(Focus::Binding(j), Some(Input::Press)));
                    }
                    if self.model.focus == Focus::Binding(j) {
                        pen.focus_ring(row, 4);
                    }
                }
                None => pen.say(&format!("layout-{k}"), row, &format!("{what}: {keys}")),
            }
        }
        let note_at = pos2(ix, y + 6.0 * 28.0 + 8.0);
        let mut note_w = lay.width() - 40.0;
        if !rebindable.is_empty() {
            let reset = Button {
                id: "reset-bindings",
                label: "Reset",
                style: ButtonStyle::Outline,
                enabled: true,
                focused: self.model.focus == Focus::ResetBindings,
                why_not: None,
            };
            let br = Rect::from_min_size(
                pos2(lay.right() - 20.0 - 120.0, lay.bottom() - 52.0),
                vec2(120.0, 38.0),
            );
            if pen.button(br, reset).clicked() {
                clicks.push(Click::Focus(Focus::ResetBindings, Some(Input::Press)));
            }
            note_w -= 140.0;
        }
        match (&self.capture, &self.binding_note) {
            (Some(cap), _) => {
                let what = rebindable
                    .get(cap.n)
                    .and_then(|&row| c.layout.get(row))
                    .map_or("it", |(what, _)| what.as_str());
                let words = format!(
                    "Press the pad button or trigger for {what} ({} s). Esc: keep it as it is.",
                    cap.remaining_ms.div_ceil(1000)
                );
                let nr = pen.wrapped(note_at, &words, body(size::NOTE), t.ink, note_w);
                pen.say("binding-wait", nr, &words);
            }
            (None, Some(note)) => {
                let color = if note.error { t.error } else { t.ink };
                let nr = pen.wrapped(note_at, &note.text, body(size::NOTE), color, note_w);
                pen.say("binding-note", nr, &note.text);
            }
            (None, None) => {
                pen.wrapped(note_at, &c.layout_note, body(size::NOTE), t.muted, note_w);
            }
        }

        let live = Rect::from_min_max(pos2(split + 24.0, top), pos2(r.right() - MARGIN, bottom));
        self.draw_live_check(pen, live);
        if self.backend.pad_now().is_some() {
            self.draw_pad_test(pen, live, clicks);
        }
    }

    /// The live check's last line: whether the pad tests the controls or
    /// moves the menu, and the button that gives it to the test.
    fn draw_pad_test(&self, pen: Pen, live: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let ix = live.left() + 20.0;
        let row = live.bottom() - 52.0;
        let test = Button {
            id: "test-pad",
            label: "Test the pad",
            style: ButtonStyle::Outline,
            enabled: !self.pad_testing,
            focused: self.model.focus == Focus::TestPad,
            why_not: Some("The pad is being tested."),
        };
        let br = Rect::from_min_size(pos2(ix, row), vec2(190.0, 38.0));
        if pen.button(br, test).clicked() && !self.pad_testing {
            clicks.push(Click::Focus(Focus::TestPad, Some(Input::Press)));
        }
        let words = if self.pad_testing {
            "Your pad's buttons only light up here now. Press Back and Start together to move through the menu with it."
        } else {
            "Your pad moves through the menu."
        };
        let wr = pen.wrapped(
            pos2(br.right() + 16.0, row),
            words,
            body(size::NOTE),
            t.ink,
            live.right() - 20.0 - br.right() - 16.0,
        );
        pen.say("pad-test", wr, words);
    }

    fn draw_live_check(&self, pen: Pen, live: Rect) {
        let t = pen.t;
        let c = &self.controls;
        pen.card(live);
        let mut y = pen.card_heading(live, "Live check") + 18.0;
        let ix = live.left() + 20.0;
        let iw = live.width() - 40.0;
        let (label_w, num_w) = (90.0, 64.0);
        for (name, v, centred) in [
            ("Steering", c.steer, true),
            ("Gas", c.accel, false),
            ("Brake", c.brake, false),
        ] {
            pen.text(
                pos2(ix, y + 12.0),
                Align2::LEFT_CENTER,
                name,
                semibold(size::LABEL),
                t.ink,
            );
            let br = Rect::from_min_size(pos2(ix + label_w, y), vec2(iw - label_w - num_w, 24.0));
            pen.bar(br, v, centred);
            let pct = format!("{:.0}%", v * 100.0);
            pen.text(
                pos2(live.right() - 20.0, y + 12.0),
                Align2::RIGHT_CENTER,
                &pct,
                body(size::BODY),
                t.ink,
            );
            pen.semantic(
                pen.id(("meter", name)),
                br,
                Sense::hover(),
                Semantics::new(Role::Meter, name)
                    .valued(&pct)
                    .level(f64::from(v * 100.0).round()),
            );
            y += 44.0;
        }
        for (k, (label, value)) in c.lines.iter().enumerate() {
            let lr = pen.text(
                pos2(ix, y + 12.0),
                Align2::LEFT_CENTER,
                label,
                semibold(size::LABEL),
                t.ink,
            );
            let vr = pen.ellipsized(
                pos2(ix + label_w, y + 2.0),
                value,
                body(size::BODY),
                t.ink,
                iw - label_w,
            );
            pen.say(
                &format!("line-{k}"),
                lr.union(vr),
                &format!("{label}: {value}"),
            );
            y += 32.0;
        }
        y += 8.0;
        pen.text(
            pos2(ix, y),
            Align2::LEFT_TOP,
            "Buttons",
            semibold(size::LABEL),
            t.ink,
        );
        y += 30.0;
        let mut x = ix;
        for b in &c.buttons {
            let w = pen.width(&b.label, semibold(size::BODY)) + 24.0;
            if x + w > live.right() - 20.0 {
                x = ix;
                y += 38.0;
            }
            let cr = if b.pressed {
                pen.chip(pos2(x, y), &b.label, Some(t.lamp), t.base)
            } else {
                pen.chip(pos2(x, y), &b.label, None, t.muted)
            };
            let state = if b.pressed { "pressed" } else { "released" };
            pen.say(
                &format!("button-{}", b.key),
                cr,
                &format!("{}: {state}", b.label),
            );
            x += w + 10.0;
        }
        y += 44.0;
        let nr = pen.wrapped(
            pos2(ix, y),
            "A pressed button turns solid.",
            body(size::NOTE),
            t.muted,
            iw,
        );
        if c.feeding {
            let words = "A game is running: it reads these controls.";
            let fr = pen.wrapped(
                pos2(ix, nr.bottom() + 8.0),
                words,
                body(size::NOTE),
                t.ink,
                iw,
            );
            pen.say("feeding", fr, words);
        }
    }

    fn draw_setup(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let w = 900.0_f32.min(r.width() - 2.0 * MARGIN - 80.0);
        let x = r.left() + MARGIN + 40.0;
        let mut y = r.top() + BAR_H + 26.0;
        let has_library = !self.config.libraries.is_empty();
        let title = if has_library {
            "Your game folders"
        } else {
            "Choose your game folders"
        };
        let tr = pen.text(pos2(x, y), Align2::LEFT_TOP, title, display(40.0), t.ink);
        pen.say("heading", tr, title);
        y += 54.0;
        let intro = self.spec.words.setup_intro;
        let ir = pen.wrapped(pos2(x, y), intro, body(size::LABEL), t.ink, w);
        pen.say("intro", ir, intro);
        y = ir.bottom() + 14.0;
        y = self.draw_folders(pen, x, y, w, "No folder yet.", false) + 10.0;

        let btn_w = 180.0;
        let field_w = w - btn_w - 16.0;
        let library = SetupRow {
            label: "Game folder",
            placeholder: "/path/to/your/games",
            button: "Add folder",
        };
        y = self.setup_row(pen, 0, library, x, y, field_w, btn_w, clicks);
        y = self.note_line(pen, "folder-note", &self.notes[0], x, y, w) + 12.0;

        for (k, f) in self.spec.fields.iter().enumerate() {
            let n = k + 1;
            let hr = pen.text(
                pos2(x, y),
                Align2::LEFT_TOP,
                f.heading,
                semibold(size::LABEL),
                t.ink,
            );
            pen.say(&format!("path-heading-{k}"), hr, f.heading);
            y += 30.0;
            let row = SetupRow {
                label: f.label,
                placeholder: f.placeholder,
                button: f.button,
            };
            y = self.setup_row(pen, n, row, x, y, field_w, btn_w, clicks);
            if self.notes[n].is_none()
                && let Some(p) = &self.config.paths[k]
            {
                let line = format!("In use: {}", display_path(p));
                let lr = pen.ellipsized(pos2(x, y), &line, body(size::BODY), t.muted, w);
                pen.say(&format!("path-current-{k}"), lr, &line);
                y += 26.0;
            }
            y = self.note_line(pen, &format!("path-note-{k}"), &self.notes[n], x, y, w) + 12.0;
        }

        let dr = Rect::from_min_size(pos2(x, y), vec2(btn_w, 50.0));
        let done = Button {
            id: "done",
            label: "Done",
            style: ButtonStyle::Outline,
            enabled: has_library,
            focused: self.model.focus == Focus::Done,
            why_not: Some("Add a game folder first."),
        };
        if pen.button(dr, done).clicked() && has_library {
            clicks.push(Click::Focus(Focus::Done, Some(Input::Press)));
        }
    }

    /// A field and its button on the setup page; returns the y below.
    #[allow(clippy::too_many_arguments)]
    fn setup_row(
        &self,
        pen: Pen,
        n: usize,
        row: SetupRow,
        x: f32,
        y: f32,
        field_w: f32,
        btn_w: f32,
        clicks: &mut Vec<Click>,
    ) -> f32 {
        let fr = Rect::from_min_size(pos2(x, y), vec2(field_w, 46.0));
        let field = TextField {
            id: &format!("field-{n}"),
            label: row.label,
            text: &self.typed[n],
            placeholder: row.placeholder,
            focused: self.model.focus == Focus::Field(n),
        };
        if pen.text_field(fr, field).clicked() {
            clicks.push(Click::Focus(Focus::Field(n), None));
        }
        let br = Rect::from_min_size(pos2(fr.right() + 16.0, y - 2.0), vec2(btn_w, 50.0));
        let button = Button {
            id: &format!("submit-{n}"),
            label: row.button,
            style: ButtonStyle::Primary,
            enabled: true,
            focused: self.model.focus == Focus::Submit(n),
            why_not: None,
        };
        if pen.button(br, button).clicked() {
            clicks.push(Click::Focus(Focus::Submit(n), Some(Input::Press)));
        }
        fr.bottom() + 8.0
    }

    /// A one-line result under a field; returns the y below it.
    fn note_line(&self, pen: Pen, key: &str, note: &Option<Note>, x: f32, y: f32, w: f32) -> f32 {
        let Some(note) = note else { return y };
        let color = if note.error { pen.t.error } else { pen.t.ink };
        let nr = pen.wrapped(pos2(x, y), &note.text, body(size::BODY), color, w);
        pen.say(key, nr, &note.text);
        nr.bottom().max(y + 24.0)
    }

    fn draw_extra(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let Some(page) = &self.extra else { return };
        let w = 760.0_f32.min(r.width() - 2.0 * MARGIN);
        let x = r.left() + MARGIN + 40.0;
        let mut y = r.top() + 120.0;
        let tr = pen.text(
            pos2(x, y),
            Align2::LEFT_TOP,
            &page.title,
            display(44.0),
            t.ink,
        );
        pen.say("heading", tr, &page.title);
        y += 60.0;
        let ir = pen.wrapped(pos2(x, y), &page.intro, body(size::LABEL), t.ink, w);
        pen.say("intro", ir, &page.intro);
        y = ir.bottom() + 18.0;
        for (k, check) in page.checklist.iter().enumerate() {
            let cr = match check.mark {
                Mark::Good => pen.chip(pos2(x, y), check.word, Some(t.sign), t.ink),
                Mark::Missing => pen.chip(pos2(x, y), check.word, None, t.muted),
                Mark::Bad => pen.chip(pos2(x, y), check.word, None, t.error),
            };
            let lr = pen.text(
                pos2(x + 120.0_f32.max(cr.width() + 12.0), cr.center().y),
                Align2::LEFT_CENTER,
                &check.text,
                body(size::BODY),
                t.ink,
            );
            pen.say(
                &format!("check-{k}"),
                cr.union(lr),
                &format!("{}: {}", check.text, check.word),
            );
            y += 36.0;
        }
        y += 14.0;
        let fr = Rect::from_min_size(pos2(x, y), vec2(w, 46.0));
        let field = TextField {
            id: "extra-field",
            label: page.label,
            text: &self.extra_typed,
            placeholder: page.placeholder,
            focused: self.model.focus == Focus::Field(0),
        };
        if pen.text_field(fr, field).clicked() {
            clicks.push(Click::Focus(Focus::Field(0), None));
        }
        y = fr.bottom() + 8.0;
        let hr = pen.wrapped(pos2(x, y), page.hint, body(size::NOTE), t.muted, w);
        pen.say("extra-hint", hr, page.hint);
        y = hr.bottom() + 10.0;
        y = self.note_line(pen, "extra-note", &self.extra_note, x, y, w) + 12.0;
        let br = Rect::from_min_size(pos2(x, y), vec2(240.0, 50.0));
        let submit = Button {
            id: "extra-submit",
            label: page.button,
            style: ButtonStyle::Primary,
            enabled: true,
            focused: self.model.focus == Focus::Submit(0),
            why_not: None,
        };
        if pen.button(br, submit).clicked() {
            clicks.push(Click::Focus(Focus::Submit(0), Some(Input::Press)));
        }
        let back = Button {
            id: "extra-back",
            label: "Back",
            style: ButtonStyle::Outline,
            enabled: true,
            focused: false,
            why_not: None,
        };
        let cr = Rect::from_min_size(pos2(br.right() + 16.0, y), vec2(150.0, 50.0));
        if pen.button(cr, back).clicked() {
            clicks.push(Click::Focus(Focus::Submit(0), Some(Input::Back)));
        }
    }

    fn draw_footer(&self, pen: Pen, r: Rect) {
        let cy = r.bottom() - 30.0;
        let back_or_quit = if self.config.libraries.is_empty() {
            "Quit"
        } else {
            "Back"
        };
        let extra_button = self.extra.as_ref().map_or("Look", |e| e.button);
        let hints: Vec<(&str, &str)> = match self.model.page {
            Page::Hub => vec![(ARROWS, "Choose"), ("Enter", "Play"), ("Esc", "Quit")],
            Page::Settings => vec![
                (ARROWS, "Change"),
                ("Enter", "Next choice"),
                ("Esc", "Back"),
                ("PgUp PgDn", "Switch page"),
            ],
            Page::Controls if !self.backend.rebindable().is_empty() => vec![
                ("Up Down", "Move"),
                ("Enter", "Change"),
                ("Esc", "Back"),
                ("PgUp PgDn", "Switch page"),
            ],
            Page::Controls => vec![("Esc", "Back"), ("PgUp PgDn", "Switch page")],
            Page::Setup => vec![("Enter", "Add"), ("Up Down", "Move"), ("Esc", back_or_quit)],
            Page::Extra => vec![("Enter", extra_button), ("Esc", "Back")],
        };
        let mut hx = r.left() + MARGIN;
        for (k, w) in hints {
            hx = pen.hint(hx, cy, k, w);
        }
        pen.text(
            pos2(r.right() - MARGIN, cy),
            Align2::RIGHT_CENTER,
            &format!("Version {}", self.spec.version),
            body(size::NOTE),
            pen.t.muted,
        );
    }

    fn draw_running(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let Some(run) = &self.running else { return };
        let game = &self.backend.games()[run.game];
        pen.p.rect_filled(r, CornerRadius::ZERO, alpha(t.base, 248));
        let c = r.center();
        let (head, body_text) = match (&run.stage, run.playing) {
            (_, true) => (
                format!("{} is running", game.title),
                "Close the game window or press Stop to come back.".to_string(),
            ),
            (Some(stage), false) => (stage.title.clone(), stage.body.clone()),
            (None, false) => (
                format!("Getting {} ready…", game.title),
                "This takes a few seconds the first time.".to_string(),
            ),
        };
        pen.shield(pos2(c.x - 30.0, c.y - 50.0), game.short, 20.0);
        let hr = pen.text(
            c,
            Align2::CENTER_CENTER,
            &head,
            display(size::PAGE_TITLE),
            t.ink,
        );
        pen.say("running", hr, &head);
        let br = pen.text(
            pos2(c.x, c.y + 44.0),
            Align2::CENTER_CENTER,
            &body_text,
            body(size::LABEL),
            t.muted,
        );
        pen.say("running-body", br, &body_text);
        if run.playing {
            let sr = Rect::from_center_size(pos2(c.x, c.y + 120.0), vec2(180.0, 50.0));
            let stop = Button {
                id: "stop",
                label: "Stop",
                style: ButtonStyle::Outline,
                enabled: !run.stop.load(Ordering::SeqCst),
                focused: true,
                why_not: Some("Stopping the game…"),
            };
            if pen.button(sr, stop).clicked() {
                clicks.push(Click::StopGame);
            }
        }
    }

    fn draw_message(&self, pen: Pen, r: Rect, clicks: &mut Vec<Click>) {
        let t = pen.t;
        let Some(m) = &self.message else { return };
        pen.p.rect_filled(r, CornerRadius::ZERO, alpha(t.base, 200));
        let w = 620.0;
        let body_g = pen
            .p
            .layout(m.body.clone(), body(size::BODY), t.ink, w - 48.0);
        let h = 40.0 + 24.0 + body_g.size().y + 24.0 + 50.0 + 24.0;
        let cr = Rect::from_center_size(r.center(), vec2(w, h));
        pen.card(cr);
        let y = pen.card_heading(cr, &m.title) + 24.0;
        pen.say(
            "message-title",
            Rect::from_min_size(cr.min, vec2(cr.width(), 40.0)),
            &m.title,
        );
        let body_at = pos2(cr.left() + 24.0, y);
        pen.say(
            "message-body",
            Rect::from_min_size(body_at, body_g.size()),
            &m.body,
        );
        let body_h = body_g.size().y;
        pen.p.galley(body_at, body_g, t.ink);
        let br = Rect::from_min_size(pos2(cr.left() + 24.0, y + body_h + 24.0), vec2(140.0, 50.0));
        let ok = Button {
            id: "dismiss",
            label: "OK",
            style: ButtonStyle::Primary,
            enabled: true,
            focused: true,
            why_not: None,
        };
        if pen.button(br, ok).clicked() {
            clicks.push(Click::Dismiss);
        }
    }
}

/// The words of one setup-page field.
#[derive(Clone, Copy)]
struct SetupRow<'a> {
    label: &'a str,
    placeholder: &'a str,
    button: &'a str,
}
