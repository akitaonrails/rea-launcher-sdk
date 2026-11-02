//! Window rows drawn from the core option table
//! (`rea_launcher::cfg::spec`), so the window and the command line come
//! from one list.
//!
//! A row edits the option's `launcher.cfg` text (`"on"`, `"fullscreen"`,
//! `"1920x1080"`, `"8"`, a path); `spec.parse(&text)` turns it into the
//! typed `Value` the launcher's `apply` takes, exactly as for the file and
//! the flags. Every row records its key, so a GUI test can check the
//! window against the table:
//!
//! ```text
//! // after clicking through every tab in a `testing::harness`:
//! rea_launcher::cfg::spec::assert_parity(SPECS, &spec::rendered_keys(&ui.ctx));
//! ```
//!
//! A removed row (`OptionSpec::removed`) draws nothing and is not recorded;
//! a window that draws rows from the table iterates
//! `rea_launcher::cfg::spec::active(specs)`. A refused value names what
//! the row takes (`OptionSpec::needs`, or its `with_needs` text).
//!
//! Controls by kind: `Toggle` → [`widgets::toggle`]; `Choice` → chips;
//! a `Toggle` or `Choice` with an unset label (`OptionSpec::unset_label`,
//! e.g. "As the game") → chips with that choice first, which writes the
//! empty text (the launcher then clears the key from `launcher.cfg`);
//! `Range` → a slider; `Size` and `Text` → a text field (a `Size` that the
//! row refuses shows what it needs); `Path` → [`widgets::folder_field`]
//! with [`spec_path_row`], a text field with [`spec_row`]. The label is the
//! row's [`widgets::field`] (the toggle's own label for a toggle), the
//! description a note under it; both are the control's accessible name and
//! description.

use std::collections::BTreeSet;
use std::path::PathBuf;

use eframe::egui::{self, Id, Response, RichText, Ui};
use rea_launcher::cfg::{Kind, OptionSpec, Value};

use crate::theme::{self, body};
use crate::widgets::{self, Chip};

fn keys_id() -> Id {
    Id::unique("rea-launcher-ui rendered option keys")
}

/// Records that the window has a control for `key` (for a control drawn
/// without [`spec_row`]).
pub fn mark_rendered(ctx: &egui::Context, key: &'static str) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<BTreeSet<&'static str>>(keys_id())
            .insert(key);
    });
}

/// Every key a row was drawn for since the context started (or
/// [`clear_rendered`]), sorted; for `assert_parity`.
pub fn rendered_keys(ctx: &egui::Context) -> Vec<&'static str> {
    ctx.data(|data| data.get_temp::<BTreeSet<&'static str>>(keys_id()))
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Forgets the recorded keys.
pub fn clear_rendered(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<BTreeSet<&'static str>>(keys_id()));
}

/// The canonical text of a toggle.
pub fn toggle_text(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// How a row is drawn beyond what its [`OptionSpec`] says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowOptions<'a> {
    /// Why the option can't be changed now: the control is disabled and
    /// shows the reason on hover and to screen readers (as chips do).
    pub disabled: Option<&'a str>,
    /// For a `Range`: chips with these `(value, label)` steps instead of a
    /// slider (OR's draw distance: `(0, "Game's own"), (4, "4"), (8, "8")`).
    pub steps: Option<&'a [(u32, &'a str)]>,
}

/// One option's row; `changed()` when `value` changed. See the module
/// docs for the control each kind gets.
pub fn spec_row(ui: &mut Ui, spec: &OptionSpec, value: &mut String) -> Response {
    row(ui, spec, value, RowOptions::default(), None)
}

/// [`spec_row`] with [`RowOptions`] (a disabled reason, Range steps).
pub fn spec_row_with(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    options: RowOptions<'_>,
) -> Response {
    row(ui, spec, value, options, None)
}

/// [`spec_row`] for a `Path` option, with a Browse button that runs `pick`.
pub fn spec_path_row(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    pick: impl FnOnce() -> Option<PathBuf>,
) -> Response {
    row(ui, spec, value, RowOptions::default(), Some(Box::new(pick)))
}

type Pick<'a> = Box<dyn FnOnce() -> Option<PathBuf> + 'a>;

fn row(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    options: RowOptions<'_>,
    pick: Option<Pick<'_>>,
) -> Response {
    // A removed key (`OptionSpec::removed`) has no control: draw nothing.
    if spec.removed.is_some() {
        return ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover());
    }
    mark_rendered(ui.ctx(), spec.key);
    let disabled = options.disabled;
    let before = value.clone();
    let mut control: Option<Response> = None;
    let mut inner = ui.vertical(|ui| {
        match spec.kind {
            Kind::Toggle if spec.unset.is_none() => {
                let mut on = matches!(spec.parse(value), Ok(Value::Toggle(true)));
                let response = widgets::toggle(ui, &mut on, spec.label, disabled);
                if response.changed() {
                    *value = toggle_text(on).to_owned();
                }
                control = Some(response);
            }
            Kind::Toggle | Kind::Choice(_) => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    let options = choices(spec);
                    // `None` is unset (the empty text).
                    let mut current: Option<&'static str> = match spec.parse(value) {
                        Ok(Value::Choice(choice)) => Some(choice),
                        Ok(Value::Toggle(on)) => Some(toggle_text(on)),
                        _ => None,
                    };
                    let chips: Vec<Chip<'_, Option<&'static str>>> = options
                        .iter()
                        .map(|(v, label)| Chip::new(*v, label).disabled(disabled))
                        .collect();
                    let response = widgets::chips_with(ui, &chips, &mut current);
                    if response.changed() {
                        *value = current.unwrap_or_default().to_owned();
                    }
                    control = Some(response);
                });
            }
            Kind::Range { min, max } => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    let parsed = match spec.parse(value) {
                        Ok(Value::Number(n)) => Some(n),
                        _ => None,
                    };
                    let response = match options.steps {
                        Some(steps) => {
                            let mut current = parsed;
                            let chips: Vec<Chip<'_, Option<u32>>> = steps
                                .iter()
                                .map(|(v, label)| Chip::new(Some(*v), label).disabled(disabled))
                                .collect();
                            let response = widgets::chips_with(ui, &chips, &mut current);
                            if response.changed()
                                && let Some(n) = current
                            {
                                *value = n.to_string();
                            }
                            response
                        }
                        None => {
                            let mut number = parsed.unwrap_or(min);
                            let response = ui.add_enabled(
                                disabled.is_none(),
                                egui::Slider::new(&mut number, min..=max),
                            );
                            if response.changed() {
                                *value = number.to_string();
                            }
                            response
                        }
                    };
                    control = Some(response);
                });
            }
            Kind::Size { .. } | Kind::Text => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    control = Some(text_field(ui, spec, value, 320.0, disabled.is_none()));
                });
            }
            Kind::Path => match pick {
                Some(pick) => {
                    widgets::field(ui, spec.label);
                    ui.horizontal(|ui| {
                        control = Some(text_field(
                            ui,
                            spec,
                            value,
                            ui.available_width() - 100.0,
                            disabled.is_none(),
                        ));
                        if disabled.is_none()
                            && widgets::outline_button(ui, "Browse…").clicked()
                            && let Some(dir) = pick()
                        {
                            *value = dir.display().to_string();
                        }
                    });
                }
                None => {
                    ui.horizontal_top(|ui| {
                        widgets::field(ui, spec.label);
                        let width = ui.available_width();
                        control = Some(text_field(ui, spec, value, width, disabled.is_none()));
                    });
                }
            },
        }
        if !value.trim().is_empty() && spec.parse(value).is_err() {
            widgets::error_note(ui, &format!("{}: needs {}", spec.label, spec.needs()));
        }
        if !spec.description.is_empty() {
            widgets::note(ui, spec.description);
        }
    });
    if let Some(control) = &control {
        // The row's description, and why it is disabled when it is.
        let description = match (disabled, spec.description) {
            (Some(why), "") => why.to_owned(),
            (Some(why), what) => format!("{what} {why}"),
            (None, what) => what.to_owned(),
        };
        widgets::name_control(control, Some(spec.label), Some(&description));
        if let Some(why) = disabled
            && !matches!(spec.kind, Kind::Toggle | Kind::Choice(_))
            && options.steps.is_none()
        {
            control.clone().on_disabled_hover_text(why);
        }
    }
    if *value != before {
        inner.response.mark_changed();
    }
    inner.response
}

/// The chips a `Toggle` or `Choice` row offers: the unset choice first
/// (value `None`, written as an empty text) when the row has one, then
/// On/Off or the table.
pub fn choices(spec: &OptionSpec) -> Vec<(Option<&'static str>, &'static str)> {
    let mut options: Vec<(Option<&'static str>, &'static str)> =
        spec.unset.map(|label| (None, label)).into_iter().collect();
    match spec.kind {
        Kind::Toggle => options.extend([(Some("on"), "On"), (Some("off"), "Off")]),
        Kind::Choice(table) => options.extend(table.iter().map(|(v, label)| (Some(*v), *label))),
        _ => {}
    }
    options
}

fn text_field(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    width: f32,
    enabled: bool,
) -> Response {
    let p = theme::palette(ui.ctx());
    let edit = egui::TextEdit::singleline(value)
        .hint_text(RichText::new(spec.placeholder()).color(p.muted))
        .font(body(theme::size::LABEL))
        .margin(egui::vec2(10.0, 8.0))
        .desired_width(width.min(ui.available_width()));
    ui.add_enabled(enabled, edit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, Queryable};
    use crate::theme::{Metrics, Palette};
    use egui::Role;
    use rea_launcher::cfg::spec::{self as core_spec, OptionSpec};

    const MODES: &[(&str, &str)] = &[("fullscreen", "Fullscreen"), ("window", "Window")];

    const SPECS: &[OptionSpec] = &[
        OptionSpec::new(
            "vsync",
            Kind::Toggle,
            "VSync",
            "Waits for the screen's refresh.",
        ),
        OptionSpec::new("display_mode", Kind::Choice(MODES), "Display", ""),
        OptionSpec::new(
            "draw_distance",
            Kind::Range { min: 1, max: 8 },
            "Distance",
            "",
        ),
        OptionSpec::new("name", Kind::Text, "Profile", "The driver's name."),
        OptionSpec::new("runtime", Kind::Path, "Runtime", "").launcher_only(),
    ];

    #[derive(Debug)]
    struct Form {
        values: Vec<String>,
        tab: u8,
    }

    fn window(ui: &mut Ui, form: &mut Form) {
        widgets::tabs_bar(ui, &[(0u8, "Picture"), (1, "Profile")], &mut form.tab);
        // One tab at a time, as a real window draws it.
        let shown: &[usize] = if form.tab == 0 { &[0, 1, 2] } else { &[3] };
        for &i in shown {
            spec_row(ui, &SPECS[i], &mut form.values[i]);
        }
    }

    fn open() -> testing::Harness<'static, Form> {
        let values = ["off", "window", "4", "", ""].map(str::to_owned).to_vec();
        testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            Form { values, tab: 0 },
            window,
        )
    }

    #[test]
    fn rows_edit_the_cfg_text() {
        let mut ui = open();
        let vsync = ui.get_by_role_and_label(Role::Switch, "VSync");
        assert_eq!(
            testing::description(&vsync).as_deref(),
            Some("Waits for the screen's refresh.")
        );
        vsync.click();
        ui.run();
        assert_eq!(ui.state().values[0], "on");
        assert_eq!(
            SPECS[0].parse(&ui.state().values[0]),
            Ok(Value::Toggle(true))
        );

        // The chip row is named by the option's label.
        let display = ui.get_by_role_and_label(Role::RadioGroup, "Display");
        assert_eq!(testing::value(&display).as_deref(), Some("Window"));
        ui.get_by_label("Fullscreen").click();
        ui.run();
        assert_eq!(ui.state().values[1], "fullscreen");

        ui.get_by_role_and_label(Role::Tab, "Profile").click();
        ui.run();
        let name = ui.get_by_role_and_label(Role::TextInput, "Profile");
        assert_eq!(
            testing::description(&name).as_deref(),
            Some("The driver's name.")
        );
        name.click();
        ui.run();
        ui.get_by_role_and_label(Role::TextInput, "Profile")
            .type_text("Colin");
        ui.run();
        assert_eq!(ui.state().values[3], "Colin");
    }

    #[test]
    fn a_refused_value_says_what_it_needs() {
        let mut ui = open();
        ui.state_mut().values[1] = "borderless".to_owned();
        ui.run();
        assert!(ui.query_by_label_contains("Display: needs").is_some());
    }

    #[test]
    fn the_window_matches_the_table_once_every_tab_was_shown() {
        let mut ui = open();
        // Only the first tab so far: the profile name is missing.
        let problems = core_spec::parity_problems(
            SPECS,
            rea_launcher::cfg::KeyStyle::Underscore,
            &rendered_keys(&ui.ctx),
        );
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].starts_with("name"));
        ui.get_by_role_and_label(Role::Tab, "Profile").click();
        ui.run();
        core_spec::assert_parity(SPECS, &rendered_keys(&ui.ctx));
        clear_rendered(&ui.ctx);
        assert!(rendered_keys(&ui.ctx).is_empty());
    }

    const UNSET_SPECS: &[OptionSpec] = &[
        OptionSpec::new("shadows", Kind::Toggle, "Shadows", "").unset_label("As the game"),
        OptionSpec::new("display_mode", Kind::Choice(MODES), "Display", "")
            .unset_label("As the game"),
        OptionSpec::new(
            "draw_distance",
            Kind::Range { min: 0, max: 8 },
            "Distance",
            "",
        ),
    ];

    const STEPS: &[(u32, &str)] = &[(0, "Game's own"), (4, "4"), (8, "8")];

    struct Unset {
        values: Vec<String>,
        locked: bool,
    }

    fn unset_window(ui: &mut Ui, form: &mut Unset) {
        spec_row(ui, &UNSET_SPECS[0], &mut form.values[0]);
        let disabled = form.locked.then_some("Needs DXVK");
        spec_row_with(
            ui,
            &UNSET_SPECS[1],
            &mut form.values[1],
            RowOptions {
                disabled,
                ..RowOptions::default()
            },
        );
        spec_row_with(
            ui,
            &UNSET_SPECS[2],
            &mut form.values[2],
            RowOptions {
                steps: Some(STEPS),
                ..RowOptions::default()
            },
        );
    }

    #[test]
    fn removed_rows_draw_nothing_and_say_what_values_need() {
        const REMOVED: OptionSpec = OptionSpec::removed("widescreen", "the patch is automatic");
        const FPS: OptionSpec =
            OptionSpec::new("fps", Kind::Range { min: 10, max: 240 }, "FPS", "")
                .with_needs("10–240 or unlimited");
        let mut ui = testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            (String::from("on"), String::from("999")),
            |ui, (old, fps): &mut (String, String)| {
                for spec in core_spec::active(&[REMOVED, FPS]) {
                    let value = if spec.key == "fps" {
                        &mut *fps
                    } else {
                        &mut *old
                    };
                    spec_row(ui, spec, value);
                }
                spec_row(ui, &REMOVED, old);
            },
        );
        assert_eq!(rendered_keys(&ui.ctx), ["fps"]);
        assert!(
            ui.query_by_label_contains("FPS: needs 10–240 or unlimited")
                .is_some()
        );
        ui.run();
    }

    #[test]
    fn unset_is_a_choice_that_clears_the_value() {
        assert_eq!(
            choices(&UNSET_SPECS[0]),
            [
                (None, "As the game"),
                (Some("on"), "On"),
                (Some("off"), "Off")
            ]
        );
        assert_eq!(choices(&SPECS[1])[0], (Some("fullscreen"), "Fullscreen"));
        let mut ui = testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            Unset {
                values: vec![String::new(), "window".into(), "4".into()],
                locked: false,
            },
            unset_window,
        );
        // Empty text: "As the game" is chosen in the tri-state row.
        let shadows = ui.get_by_role_and_label(Role::RadioGroup, "Shadows");
        assert_eq!(testing::value(&shadows).as_deref(), Some("As the game"));
        let rows: Vec<_> = ui.get_all_by_label("As the game").collect();
        assert_eq!(rows.len(), 2);
        ui.get_all_by_label("On").next().unwrap().click();
        ui.run();
        assert_eq!(ui.state().values[0], "on");
        // Back to unset: the key is cleared.
        ui.get_all_by_label("As the game").nth(1).unwrap().click();
        ui.run();
        assert_eq!(ui.state().values[1], "");
    }

    #[test]
    fn rows_can_be_disabled_and_ranges_drawn_as_steps() {
        let mut ui = testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            Unset {
                values: vec![String::new(), "window".into(), "4".into()],
                locked: true,
            },
            unset_window,
        );
        let display = ui.get_by_role_and_label(Role::RadioGroup, "Display");
        assert_eq!(
            testing::description(&display).as_deref(),
            Some("Needs DXVK")
        );
        let window = ui.get_by_label("Window");
        assert!(testing::disabled(&window));
        ui.get_by_label("Fullscreen").click();
        ui.run();
        assert_eq!(
            ui.state().values[1],
            "window",
            "disabled rows ignore clicks"
        );

        // The range is chips, not a slider.
        assert!(ui.query_by_role(Role::Slider).is_none());
        let distance = ui.get_by_role_and_label(Role::RadioGroup, "Distance");
        assert_eq!(testing::value(&distance).as_deref(), Some("4"));
        ui.get_by_label("Game's own").click();
        ui.run();
        assert_eq!(ui.state().values[2], "0");
    }
}
