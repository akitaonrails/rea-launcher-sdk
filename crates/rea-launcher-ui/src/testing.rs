//! Headless GUI tests for launcher windows: [`egui_kittest`] drives the
//! real widgets and queries them through the AccessKit tree, the way a
//! screen reader sees them — no window, no GPU, so it runs on the host and
//! under `scripts/wine-test`.
//!
//! Enable it for tests only:
//!
//! ```toml
//! [dev-dependencies]
//! rea-launcher-ui = { git = "…", tag = "…", features = ["testing"] }
//! ```
//!
//! ```
//! use rea_launcher_ui::testing::{self, Queryable};
//! use rea_launcher_ui::theme::{Metrics, Palette};
//! use rea_launcher_ui::widgets;
//!
//! let mut ui = testing::harness(&Palette::default(), &Metrics::DEFAULT, 60u32, |ui, fps| {
//!     widgets::chips(ui, &[(60u32, "60"), (120, "120")], fps, None);
//! });
//! ui.get_by_label("120").click();
//! ui.run();
//! assert_eq!(*ui.state(), 120);
//! assert!(testing::checked(&ui.get_by_label("120")));
//! ```

use eframe::egui::{self, Ui, accesskit::Toggled};
pub use egui_kittest::{
    Harness, Node,
    kittest::{self, NodeT, Queryable},
};

use crate::theme::{self, Metrics, Palette};

/// The window size the harness lays out in: the SDK's design size.
pub const SIZE: egui::Vec2 = crate::window::SIZE;

/// A harness running `app` over `state` with the theme installed, after
/// its first frames (so every widget is in the AccessKit tree).
pub fn harness<'a, S>(
    palette: &Palette,
    metrics: &Metrics,
    state: S,
    app: impl FnMut(&mut Ui, &mut S) + 'a,
) -> Harness<'a, S> {
    let (palette, metrics) = (*palette, *metrics);
    let mut harness = Harness::builder()
        .with_size(SIZE)
        .with_theme(egui::Theme::Dark)
        .with_context_setup(move |ctx| theme::install_with(ctx, &palette, &metrics))
        .build_ui_state(app, state);
    harness.run();
    harness
}

/// Whether a radio button, switch, tab or card is checked/selected.
pub fn checked(node: &Node<'_>) -> bool {
    node.accesskit_node().toggled() == Some(Toggled::True)
}

/// Whether the control is disabled.
pub fn disabled(node: &Node<'_>) -> bool {
    node.accesskit_node().is_disabled()
}

/// The control's description: a disabled reason, a card's note.
pub fn description(node: &Node<'_>) -> Option<String> {
    node.accesskit_node().description()
}

/// The control's value: a chip row's chosen label, a text field's text.
pub fn value(node: &Node<'_>) -> Option<String> {
    node.accesskit_node().value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::tests::{OR, OR_METRICS, SCR_METRICS};
    use crate::widgets::{self, Chip, GameCard, LookCard};
    use egui::{Role, vec2};

    #[derive(Debug, Default)]
    struct Form {
        fps: u32,
        renderer: u8,
        vsync: bool,
        tab: u8,
        look: u8,
        game: u8,
        folder: String,
        played: u32,
    }

    fn window(ui: &mut Ui, form: &mut Form) {
        widgets::tabs_bar(ui, &[(0u8, "Play"), (1, "Picture")], &mut form.tab);
        widgets::chips(
            ui,
            &[(60u32, "60 fps"), (120, "120 fps")],
            &mut form.fps,
            None,
        );
        widgets::chips_with(
            ui,
            &[
                Chip::new(0u8, "Software"),
                Chip::new(1, "Glide").disabled(Some("Needs the 3dfx build")),
            ],
            &mut form.renderer,
        );
        widgets::toggle(ui, &mut form.vsync, "VSync", None);
        ui.horizontal(|ui| {
            for (look, title) in [(0u8, "Original"), (1, "Enhanced")] {
                let card = LookCard {
                    title,
                    note: "The game's own look.",
                    selected: form.look == look,
                    badge: (look == 1).then_some("Recommended"),
                    ..LookCard::default()
                };
                if widgets::look_card(ui, vec2(180.0, 90.0), &card).clicked() {
                    form.look = look;
                }
            }
        });
        ui.horizontal(|ui| {
            for (game, title, installed) in [(0u8, "Screamer", true), (1, "Screamer 2", false)] {
                let card = GameCard {
                    title,
                    subtitle: "1995",
                    selected: form.game == game,
                    installed,
                };
                if widgets::game_card(ui, vec2(180.0, 64.0), &card).clicked() {
                    form.game = game;
                }
            }
        });
        widgets::folder_field(ui, &mut form.folder, "The game folder", || None);
        let plate = egui::Rect::from_min_size(egui::pos2(600.0, 560.0), vec2(170.0, 52.0));
        if widgets::play_plate(ui, plate, "Play", form.fps == 120).clicked() {
            form.played += 1;
        }
    }

    fn open() -> Harness<'static, Form> {
        harness(
            &OR,
            &OR_METRICS,
            Form {
                fps: 60,
                ..Form::default()
            },
            window,
        )
    }

    #[test]
    fn a_chip_is_found_by_label_and_clicked() {
        let mut ui = open();
        assert!(checked(&ui.get_by_label("60 fps")));
        ui.get_by_role_and_label(Role::RadioButton, "120 fps")
            .click();
        ui.run();
        assert_eq!(ui.state().fps, 120);
        assert!(checked(&ui.get_by_label("120 fps")));
        assert!(!checked(&ui.get_by_label("60 fps")));
        // The row reports the chosen label as its value.
        let rows: Vec<_> = ui.get_all_by_role(Role::RadioGroup).collect();
        assert!(
            rows.iter()
                .any(|row| value(row).as_deref() == Some("120 fps"))
        );
    }

    #[test]
    fn a_disabled_chip_exposes_its_reason_and_ignores_clicks() {
        let mut ui = open();
        let glide = ui.get_by_label("Glide");
        assert!(disabled(&glide));
        assert_eq!(description(&glide).as_deref(), Some("Needs the 3dfx build"));
        glide.click();
        ui.run();
        assert_eq!(ui.state().renderer, 0);
    }

    #[test]
    fn toggle_tabs_and_cards_are_accessible() {
        let mut ui = open();
        let vsync = ui.get_by_role_and_label(Role::Switch, "VSync");
        assert!(!checked(&vsync));
        vsync.click();
        ui.run();
        assert!(ui.state().vsync);
        assert!(checked(&ui.get_by_label("VSync")));

        assert!(checked(&ui.get_by_role_and_label(Role::Tab, "Play")));
        ui.get_by_role_and_label(Role::Tab, "Picture").click();
        ui.run();
        assert_eq!(ui.state().tab, 1);
        assert!(checked(&ui.get_by_role_and_label(Role::Tab, "Picture")));

        let enhanced = ui.get_by_label("Enhanced");
        assert_eq!(
            description(&enhanced).as_deref(),
            Some("Recommended. The game's own look.")
        );
        enhanced.click();
        ui.run();
        assert_eq!(ui.state().look, 1);

        let missing = ui.get_by_label("Screamer 2");
        assert_eq!(
            description(&missing).as_deref(),
            Some("1995; not installed")
        );
        missing.click();
        ui.run();
        assert_eq!(ui.state().game, 1);
    }

    #[test]
    fn folder_field_and_play_plate_are_named() {
        let mut ui = open();
        let field = ui.get_by_label("The game folder");
        assert_eq!(field.accesskit_node().role(), Role::TextInput);
        assert!(
            ui.query_by_role_and_label(Role::Button, "Browse…")
                .is_some()
        );
        // Play is disabled until the form is ready.
        let play = ui.get_by_role_and_label(Role::Button, "Play");
        assert!(disabled(&play));
        ui.get_by_label("120 fps").click();
        ui.run();
        assert!(!disabled(&ui.get_by_role_and_label(Role::Button, "Play")));
        ui.get_by_role_and_label(Role::Button, "Play").click();
        ui.run();
        assert_eq!(ui.state().played, 1);
    }

    #[test]
    fn keyboard_reaches_the_chip_row() {
        let mut ui = harness(
            &Palette::default(),
            &SCR_METRICS,
            60u32,
            |ui, fps: &mut u32| {
                widgets::chips(ui, &[(60u32, "60"), (90, "90")], fps, None);
            },
        );
        ui.key_press(egui::Key::Tab);
        ui.run();
        ui.key_press(egui::Key::ArrowRight);
        ui.run();
        assert_eq!(*ui.state(), 90);
    }
}
