//! Headless tests of a hub launcher (feature `testing`): a harness over
//! the launcher's real [`HubBackend`] (fake games and folders are up to
//! the test), waiting for the scan, a close request, the controls that
//! leave the window, and an AccessKit dump for golden snapshots.
//!
//! ```text
//! let mut h = hub::testing::harness(spec, backend, hub::testing::SIZE_720);
//! hub::testing::wait(&mut h, "the scan", |a| !a.scanning());
//! assert!(hub::testing::misfits(&h).is_empty());
//! snapshots.assert("hub-720.txt", &hub::testing::dump(&h));
//! ```

use eframe::egui::{self, Vec2, ViewportEvent, ViewportId, vec2};

use super::{HubApp, HubBackend, HubSpec};
use crate::testing::{Harness, NodeT, kittest::Queryable};

/// The design size: 16:9.
pub const SIZE_720: Vec2 = vec2(1280.0, 720.0);
/// A Steam Deck's 16:10 (zoomed to 720 lines: 1152 points wide).
pub const SIZE_800: Vec2 = vec2(1280.0, 800.0);

/// The hub over `backend` at `size` (in pixels), theme installed, after
/// its first frames.
pub fn harness<B: HubBackend>(
    spec: HubSpec,
    backend: B,
    size: Vec2,
) -> Harness<'static, HubApp<B>> {
    let theme = spec.theme;
    let mut h = Harness::builder()
        .with_size(size)
        .with_theme(egui::Theme::Dark)
        .with_context_setup(move |ctx| super::install(ctx, &theme))
        .build_ui_state(
            |ui, app: &mut HubApp<B>| app.frame(ui),
            HubApp::new(spec, backend),
        );
    h.run_steps(3);
    h
}

/// Runs frames until `done` (a scan, a session's end), for up to 5 s.
#[track_caller]
pub fn wait<B: HubBackend>(
    h: &mut Harness<'_, HubApp<B>>,
    what: &str,
    done: impl Fn(&HubApp<B>) -> bool,
) {
    for _ in 0..500 {
        h.run_steps(1);
        if done(h.state()) {
            h.run_steps(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("timed out waiting for {what}");
}

/// The window's close button, pressed (as the title bar or Alt+F4 does).
pub fn request_close<S>(h: &mut Harness<'_, S>) {
    h.input_mut()
        .viewports
        .entry(ViewportId::ROOT)
        .or_default()
        .events
        .push(ViewportEvent::Close);
}

/// The viewport commands the last frame sent (a close, a cancelled close).
pub fn commands<S>(h: &Harness<'_, S>) -> Vec<egui::ViewportCommand> {
    h.output()
        .viewport_output
        .get(&ViewportId::ROOT)
        .map(|v| v.commands.clone())
        .unwrap_or_default()
}

/// The nodes that matter, depth first: a role with a name, value or level.
fn named<'t>(h: &'t Harness<'_, impl Sized>) -> impl Iterator<Item = crate::testing::Node<'t>> {
    h.root().children_recursive().filter(|n| {
        let a = n.accesskit_node();
        a.label().is_some() || a.value().is_some() || a.numeric_value().is_some()
    })
}

/// The window as AccessKit sees it, one line per named node, depth first:
/// `Button "Play" = value (desc) [disabled] [on|off] @x,y wxh`, positions
/// in whole pixels. Compare it with a golden file; a change must match an
/// intended change.
pub fn dump<S>(h: &Harness<'_, S>) -> String {
    let mut out = String::new();
    for n in named(h) {
        let a = n.accesskit_node();
        if a.role() == egui::accesskit::Role::TextRun {
            continue;
        }
        let mut line = format!("{:?}", a.role());
        if let Some(l) = a.label() {
            line.push_str(&format!(" {l:?}"));
        }
        if let Some(v) = a.value() {
            line.push_str(&format!(" = {v:?}"));
        }
        if let Some(x) = a.numeric_value() {
            line.push_str(&format!(" #{x}"));
        }
        if let Some(d) = a.description() {
            line.push_str(&format!(" ({d})"));
        }
        if a.is_disabled() {
            line.push_str(" [disabled]");
        }
        match a.toggled() {
            Some(egui::accesskit::Toggled::True) => line.push_str(" [on]"),
            Some(_) => line.push_str(" [off]"),
            None => {}
        }
        if let Some(b) = a.bounding_box() {
            line.push_str(&format!(
                " @{},{} {}x{}",
                b.x0.round(),
                b.y0.round(),
                (b.x1 - b.x0).round(),
                (b.y1 - b.y0).round()
            ));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Every named control or line that doesn't fit inside the window (a
/// pixel of slack), as `Role "name" @rect`: empty when all fit.
pub fn misfits<S>(h: &Harness<'_, S>) -> Vec<String> {
    let size = h
        .ctx
        .input(|i| i.viewport_rect().size() * i.pixels_per_point);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size).expand(1.0);
    named(h)
        .filter_map(|n| {
            let a = n.accesskit_node();
            let b = a.bounding_box()?;
            let r = egui::Rect::from_min_max(
                egui::pos2(b.x0 as f32, b.y0 as f32),
                egui::pos2(b.x1 as f32, b.y1 as f32),
            );
            (r.area() > 0.0 && !screen.contains_rect(r)).then(|| {
                let name = a.label().or(a.value()).unwrap_or_default();
                format!("{:?} {name:?} @{r:?}", a.role())
            })
        })
        .collect()
}

/// Whether some label or value contains `words`.
pub fn says<S>(h: &Harness<'_, S>, words: &str) -> bool {
    h.query_by_label_contains(words).is_some()
}
