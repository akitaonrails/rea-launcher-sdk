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
//!
//! **Wrapped text.** A label egui wraps over several rows is one `Label`
//! node holding the whole text, with one `TextRun` child per row. Find it,
//! or a phrase in it, by label: `get_by_label(text)` or
//! `get_by_label_contains("a phrase that may cross a wrap")` match the
//! whole text. Anything that looks at the rows (`get_by_value` of a
//! phrase, a predicate over `TextRun` nodes) sees only one row, and a
//! phrase cut by the wrap is in none of them. Text a widget paints itself
//! (the photo caption) is found by the label that widget gives it.

use eframe::egui::{self, Ui, accesskit::Toggled};
pub use egui_kittest::{
    Harness, Node,
    kittest::{self, NodeT, Queryable},
};

use crate::theme::{self, Metrics, Palette};

/// The window size the harness lays out in: the SDK's design size.
pub const SIZE: egui::Vec2 = crate::window::SIZE;

/// A harness running `app` over `state` with the theme installed, after
/// its first frames (so every widget is in the AccessKit tree), at the
/// SDK's design [`SIZE`]. See [`harness_sized`] for a taller window.
pub fn harness<'a, S>(
    palette: &Palette,
    metrics: &Metrics,
    state: S,
    app: impl FnMut(&mut Ui, &mut S) + 'a,
) -> Harness<'a, S> {
    harness_sized(palette, metrics, SIZE, state, app)
}

/// [`harness`] at `size` (the launcher's own design size, e.g. OR's
/// 880×760).
///
/// **Below the fold.** A widget below a scrolling tab's fold is in the
/// AccessKit tree, but a click on it lands outside the window and is
/// silently lost (the test then fails on the state, not on the click).
/// Use [`click_in_view`], or by hand: `scroll_to_me`, `run()` (the scroll
/// area moves), query the node again (the old one has the old position),
/// `click`, `run()`:
///
/// ```text
/// ui.get_by_label("Manual gearbox by default").scroll_to_me();
/// ui.run();
/// ui.get_by_label("Manual gearbox by default").click();
/// ui.run();
/// ```
///
/// Or give the harness a taller size so the whole tab fits.
pub fn harness_sized<'a, S>(
    palette: &Palette,
    metrics: &Metrics,
    size: egui::Vec2,
    state: S,
    app: impl FnMut(&mut Ui, &mut S) + 'a,
) -> Harness<'a, S> {
    let (palette, metrics) = (*palette, *metrics);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_theme(egui::Theme::Dark)
        .with_context_setup(move |ctx| theme::install_with(ctx, &palette, &metrics))
        .build_ui_state(app, state);
    harness.run();
    harness
}

/// Scrolls the control with `role` and `label` into view, clicks it and
/// runs the frames that follow: the safe click for anything that may be
/// below a tab's fold (see [`harness_sized`]).
pub fn click_in_view<S>(ui: &mut Harness<'_, S>, role: egui::accesskit::Role, label: &str) {
    ui.get_by_role_and_label(role, label).scroll_to_me();
    ui.run();
    ui.get_by_role_and_label(role, label).click();
    ui.run();
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
                    disabled: None,
                };
                if widgets::game_card(ui, vec2(180.0, 64.0), &card).clicked() {
                    form.game = game;
                }
            }
        });
        widgets::folder_field(
            ui,
            &mut form.folder,
            "Game folder",
            "The folder with game.exe",
            || None,
        );
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
        // The reason is on the window too: a pad player never hovers.
        assert!(ui.query_by_label("Glide: Needs the 3dfx build").is_some());
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
        let field = ui.get_by_role_and_label(Role::TextInput, "Game folder");
        assert_eq!(
            description(&field).as_deref(),
            Some("The folder with game.exe")
        );
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

    #[test]
    fn a_control_below_the_fold_is_scrolled_to_then_clicked() {
        let draw = |ui: &mut Ui, on: &mut bool| {
            widgets::tab_body(ui, "tab", |ui| {
                for row in 0..40 {
                    widgets::note(ui, &format!("Row {row}"));
                }
                widgets::toggle(ui, on, "Manual gearbox", None);
            });
        };
        // Without scrolling, the click lands outside the window.
        let mut ui = harness(&Palette::default(), &Metrics::DEFAULT, false, draw);
        ui.get_by_label("Manual gearbox").click();
        ui.run();
        assert!(!*ui.state(), "clicked below the fold: missed");
        // The recipe: scroll_to_me, run, query again, click, run.
        ui.get_by_label("Manual gearbox").scroll_to_me();
        ui.run();
        ui.get_by_label("Manual gearbox").click();
        ui.run();
        assert!(*ui.state());
        // The same in one call.
        click_in_view(&mut ui, Role::Switch, "Manual gearbox");
        assert!(!*ui.state());
    }

    // ------------------------------------------------- v0.2.1 additions

    #[derive(Debug, Default)]
    struct Extra {
        tab: u8,
        fps: u32,
        layout: u8,
    }

    fn extra(ui: &mut Ui, form: &mut Extra) {
        widgets::tabs_bar(
            ui,
            &[(0u8, "Play"), (1, "Picture"), (2, "Controls")],
            &mut form.tab,
        );
        let row = widgets::chips(ui, &[(60u32, "60"), (120, "120")], &mut form.fps, None);
        widgets::name_control(&row, Some("Frame rate"), Some("The game's frame cap."));
        ui.horizontal(|ui| {
            for (layout, title, disabled) in [
                (0u8, "Original", None),
                (1, "Custom", Some("Needs gamepad axes on")),
            ] {
                let card = LookCard {
                    title,
                    note: "Buttons",
                    selected: form.layout == layout,
                    disabled,
                    ..LookCard::default()
                };
                if widgets::look_card(ui, vec2(180.0, 90.0), &card).clicked() {
                    form.layout = layout;
                }
            }
            let card = GameCard {
                title: "Screamer Rally",
                subtitle: "1997",
                disabled: Some("Not in this folder"),
                ..GameCard::default()
            };
            widgets::game_card(ui, vec2(180.0, 64.0), &card);
        });
        let strip = egui::Rect::from_min_size(egui::pos2(0.0, 300.0), vec2(300.0, 300.0));
        widgets::photo_caption(
            ui,
            strip,
            "Colin McRae at a rally stage",
            &[
                ("Colin McRae", widgets::CaptionStyle::Title),
                (
                    "1968–2007, World Rally Champion 1995",
                    widgets::CaptionStyle::Text,
                ),
                (
                    "Photo: Carolyn Williams, CC BY 2.0",
                    widgets::CaptionStyle::Credit,
                ),
            ],
        );
        let footer = egui::Rect::from_min_size(egui::pos2(320.0, 620.0), vec2(600.0, 52.0));
        widgets::footer(ui, footer, "Play needs the game folder first.", false);
    }

    fn open_extra() -> Harness<'static, Extra> {
        // Screamer's taller window: the footer sits below 640.
        harness_sized(
            &OR,
            &OR_METRICS,
            vec2(980.0, 700.0),
            Extra::default(),
            extra,
        )
    }

    #[test]
    fn keyboard_switches_tabs_and_tabs_form_a_list() {
        let mut ui = open_extra();
        assert!(ui.query_by_role(Role::TabList).is_some());
        ui.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Tab);
        ui.run();
        assert_eq!(ui.state().tab, 1);
        ui.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::PageDown);
        ui.run();
        assert_eq!(ui.state().tab, 2);
        // Wraps forward, and back with Ctrl+Shift+Tab / Ctrl+PageUp.
        ui.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Tab);
        ui.run();
        assert_eq!(ui.state().tab, 0);
        ui.key_press_modifiers(
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            egui::Key::Tab,
        );
        ui.run();
        assert_eq!(ui.state().tab, 2);
        ui.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::PageUp);
        ui.run();
        assert_eq!(ui.state().tab, 1);
        assert!(checked(&ui.get_by_role_and_label(Role::Tab, "Picture")));
    }

    #[test]
    fn chip_groups_cards_captions_and_play_say_what_they_are() {
        let mut ui = open_extra();
        let group = ui.get_by_role_and_label(Role::RadioGroup, "Frame rate");
        assert_eq!(
            description(&group).as_deref(),
            Some("The game's frame cap.")
        );

        let custom = ui.get_by_label("Custom");
        assert!(disabled(&custom));
        assert_eq!(
            description(&custom).as_deref(),
            Some("Buttons. Needs gamepad axes on")
        );
        custom.click();
        ui.run();
        assert_eq!(ui.state().layout, 0, "a disabled card ignores clicks");
        let rally = ui.get_by_label("Screamer Rally");
        assert!(disabled(&rally));
        assert!(description(&rally).unwrap().ends_with("Not in this folder"));

        assert!(
            ui.query_by_role_and_label(Role::Image, "Colin McRae at a rally stage")
                .is_some()
        );
        assert!(
            ui.query_by_label_contains("Photo: Carolyn Williams, CC BY 2.0")
                .is_some()
        );

        // Below the design height: reachable in the taller harness.
        let play = ui.get_by_role_and_label(Role::Button, "Play");
        assert!(disabled(&play));
        assert_eq!(
            description(&play).as_deref(),
            Some("Play needs the game folder first.")
        );
    }
}

#[cfg(test)]
mod wrapped_text {
    use super::*;
    use egui::accesskit::Role;

    const TEXT: &str = "The launcher copies the game out of your installer files into a \
                        folder of its own; the installer itself is never run.";

    /// A label wrapped over several rows is one `Label` node with the whole
    /// text (found by label, a phrase across a wrap included); its
    /// `TextRun` children hold one row each, so a value query sees a row.
    #[test]
    fn a_phrase_across_a_wrap_is_found_on_the_label_not_its_rows() {
        let ui = harness(&Palette::default(), &Metrics::DEFAULT, (), |ui, _| {
            ui.allocate_ui(egui::vec2(200.0, 400.0), |ui| ui.label(TEXT));
        });
        let label = ui.get_by_label_contains("installer files into a folder of its own");
        assert_eq!(label.accesskit_node().role(), Role::Label);
        let rows: Vec<String> = label
            .children()
            .filter_map(|row| row.accesskit_node().value())
            .collect();
        assert!(rows.len() > 2, "wrapped: {rows:?}");
        assert_eq!(rows.concat(), TEXT, "the rows are the text, split");
        // A value query matches the whole text on the label, and a row only
        // on that row's node: a phrase across the wrap is in no row.
        assert_eq!(ui.query_all_by_value(TEXT).count(), 1);
        let row = ui.get_by_value(&rows[0]);
        assert_eq!(row.accesskit_node().role(), Role::TextRun);
        assert!(!rows.iter().any(|r| r.contains("into a folder of its own")));
    }
}
