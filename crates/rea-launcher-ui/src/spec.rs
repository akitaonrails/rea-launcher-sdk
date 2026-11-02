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
//! Controls by kind: `Toggle` → [`widgets::toggle`]; `Choice` → chips;
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

/// One option's row; `changed()` when `value` changed. See the module
/// docs for the control each kind gets.
pub fn spec_row(ui: &mut Ui, spec: &OptionSpec, value: &mut String) -> Response {
    row(ui, spec, value, None)
}

/// [`spec_row`] for a `Path` option, with a Browse button that runs `pick`.
pub fn spec_path_row(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    pick: impl FnOnce() -> Option<PathBuf>,
) -> Response {
    row(ui, spec, value, Some(Box::new(pick)))
}

type Pick<'a> = Box<dyn FnOnce() -> Option<PathBuf> + 'a>;

fn row(ui: &mut Ui, spec: &OptionSpec, value: &mut String, pick: Option<Pick<'_>>) -> Response {
    mark_rendered(ui.ctx(), spec.key);
    let before = value.clone();
    let mut control: Option<Response> = None;
    let mut inner = ui.vertical(|ui| {
        match spec.kind {
            Kind::Toggle => {
                let mut on = matches!(spec.parse(value), Ok(Value::Toggle(true)));
                let response = widgets::toggle(ui, &mut on, spec.label, None);
                if response.changed() {
                    *value = toggle_text(on).to_owned();
                }
                control = Some(response);
            }
            Kind::Choice(table) => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    let mut current = match spec.parse(value) {
                        Ok(Value::Choice(choice)) => Some(choice),
                        _ => None,
                    };
                    let chips: Vec<Chip<'_, Option<&'static str>>> = table
                        .iter()
                        .map(|(v, label)| Chip::new(Some(*v), label))
                        .collect();
                    let response = widgets::chips_with(ui, &chips, &mut current);
                    if response.changed()
                        && let Some(choice) = current
                    {
                        *value = choice.to_owned();
                    }
                    control = Some(response);
                });
            }
            Kind::Range { min, max } => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    let mut number = match spec.parse(value) {
                        Ok(Value::Number(n)) => n,
                        _ => min,
                    };
                    let response = ui.add(egui::Slider::new(&mut number, min..=max));
                    if response.changed() {
                        *value = number.to_string();
                    }
                    control = Some(response);
                });
            }
            Kind::Size { .. } | Kind::Text => {
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    control = Some(text_field(ui, spec, value, 320.0));
                });
            }
            Kind::Path => match pick {
                Some(pick) => {
                    widgets::field(ui, spec.label);
                    ui.horizontal(|ui| {
                        control = Some(text_field(ui, spec, value, ui.available_width() - 100.0));
                        if widgets::outline_button(ui, "Browse…").clicked()
                            && let Some(dir) = pick()
                        {
                            *value = dir.display().to_string();
                        }
                    });
                }
                None => {
                    ui.horizontal_top(|ui| {
                        widgets::field(ui, spec.label);
                        control = Some(text_field(ui, spec, value, ui.available_width()));
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
        let (label, description) = (spec.label, spec.description);
        ui.ctx().accesskit_node_builder(control.id, |node| {
            node.set_label(label.to_owned());
            if !description.is_empty() {
                node.set_description(description.to_owned());
            }
        });
    }
    if *value != before {
        inner.response.mark_changed();
    }
    inner.response
}

fn text_field(ui: &mut Ui, spec: &OptionSpec, value: &mut String, width: f32) -> Response {
    let p = theme::palette(ui.ctx());
    let edit = egui::TextEdit::singleline(value)
        .hint_text(RichText::new(spec.placeholder()).color(p.muted))
        .font(body(theme::size::LABEL))
        .margin(egui::vec2(10.0, 8.0))
        .desired_width(width.min(ui.available_width()));
    ui.add(edit)
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
}
