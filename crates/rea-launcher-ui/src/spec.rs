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
//! `Range` → a slider, or chips with `RowOptions::steps` (the unset chip
//! first when the spec has one, see [`step_choices`]); `Size` → a text
//! field, or with `RowOptions::sizes` chips of the screen's and common
//! sizes plus Custom ([`size_choices`]); `Text` → a text field (a value
//! the row refuses shows what it needs); `Path` → [`widgets::folder_field`]
//! with [`spec_path_row`], a text field with [`spec_row`]. The label is the
//! row's [`widgets::field`] (the toggle's own label for a toggle), the
//! description a note under it (the chosen value's note instead with
//! `RowOptions::notes`, the disabled reason while disabled); both are the control's accessible name and
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
    /// Why the option can't be changed now: the control is disabled (still
    /// showing the saved value) and the reason is written in place of the
    /// description, where a pad player sees it; also on hover and, after
    /// the description, to screen readers.
    pub disabled: Option<&'a str>,
    /// For a `Range`: chips with these `(value, label)` steps instead of a
    /// slider (OR's draw distance: `(0, "Game's own"), (4, "4"), (8, "8")`).
    /// With an `unset_label` the unset chip comes first (CMR3's field of
    /// view: "As the game", 60°, 75°…); a saved value between the steps
    /// gets its own chip ([`step_choices`]).
    pub steps: Option<&'a [(u32, &'a str)]>,
    /// What each value does, by its `launcher.cfg` text (`""` for unset):
    /// the chosen value's note is shown in place of the row's description
    /// and added to the accessible description (CMR3's gamepad presets).
    pub notes: Option<&'a [(&'a str, &'a str)]>,
    /// For a `Size`: chips of sizes from the screen instead of a text
    /// field ([`size_choices`]), plus Custom for any other size.
    pub sizes: Option<SizeChips>,
}

/// How a `Size` row offers sizes ([`RowOptions::sizes`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SizeChips {
    /// The screen in pixels (`window::WindowFit::screen`), either
    /// orientation; `None` until known.
    pub screen: Option<(u32, u32)>,
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
                let response = widgets::toggle_noted(ui, &mut on, spec.label, disabled, false);
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
                    let response = quiet_chips(ui, &chips, &mut current);
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
                            let labels = step_choices(spec, steps, parsed);
                            let chips: Vec<Chip<'_, Option<u32>>> = labels
                                .iter()
                                .map(|(v, label)| Chip::new(*v, label.as_str()).disabled(disabled))
                                .collect();
                            let response = quiet_chips(ui, &chips, &mut current);
                            if response.changed() {
                                // `None` is the unset chip: the empty text.
                                *value = current.map(|n| n.to_string()).unwrap_or_default();
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
            Kind::Size { .. } if options.sizes.is_some() => {
                let screen = options.sizes.and_then(|sizes| sizes.screen);
                ui.horizontal_top(|ui| {
                    widgets::field(ui, spec.label);
                    control = Some(size_chips(ui, spec, value, screen, disabled));
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
        // A pad never hovers: the reason takes the description's place, so
        // the row keeps its height when it is disabled.
        match disabled.filter(|why| !why.is_empty()) {
            Some(why) => {
                widgets::note(ui, why);
            }
            None => {
                let shown = value_note(options.notes, value).unwrap_or(spec.description);
                if !shown.is_empty() {
                    widgets::note(ui, shown);
                }
            }
        }
    });
    if let Some(control) = &control {
        // The row's description, and why it is disabled when it is.
        let described = match value_note(options.notes, value) {
            Some(note) => widgets::join_sentences(spec.description, note),
            None => spec.description.to_owned(),
        };
        let description = widgets::join_sentences(&described, disabled.unwrap_or_default());
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

/// The note for `value` in `notes` (matched without case or spaces).
fn value_note<'a>(notes: Option<&[(&str, &'a str)]>, value: &str) -> Option<&'a str> {
    notes?
        .iter()
        .find(|(v, _)| v.eq_ignore_ascii_case(value.trim()))
        .map(|(_, note)| *note)
}

/// The chips of a stepped `Range` row: the unset chip first when the
/// spec has an `unset_label` (value `None`), the steps, then a `saved`
/// value that is none of them (its number as the label).
pub fn step_choices(
    spec: &OptionSpec,
    steps: &[(u32, &str)],
    saved: Option<u32>,
) -> Vec<(Option<u32>, String)> {
    let mut choices: Vec<(Option<u32>, String)> = spec
        .unset
        .map(|label| (None, label.to_owned()))
        .into_iter()
        .collect();
    choices.extend(
        steps
            .iter()
            .map(|(n, label)| (Some(*n), (*label).to_owned())),
    );
    if let Some(n) = saved
        && !steps.iter().any(|(step, _)| *step == n)
    {
        choices.push((Some(n), n.to_string()));
    }
    choices
}

/// Common screen sizes a `Size` row offers, with their short names.
pub const COMMON_SIZES: [((u32, u32), &str); 3] = [
    ((1920, 1080), "1080p"),
    ((2560, 1440), "1440p"),
    ((3840, 2160), "4K"),
];

/// One chip of a `Size` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SizeChoice {
    /// The spec's unset chip (`unset_label`): the empty text.
    Unset,
    /// A `launcher.cfg` text (`1920x1080`, or a saved alias word).
    Text(String),
    /// Any other size, typed in a field under the chips.
    Custom,
}

/// The chips of a `Size` row: the unset chip (with an `unset_label`),
/// "Screen W×H" (the landscape screen size, unless it is a common one),
/// [`COMMON_SIZES`], a `saved` text that is none of them (shown as it
/// is, `×` for `x`), then Custom. Sizes the spec refuses are left out.
/// This is SRR's, OR's and CMR3's resolution row; the launcher's own
/// words (CMR3's "Original 640×480") stay launcher code.
pub fn size_choices(
    spec: &OptionSpec,
    screen: Option<(u32, u32)>,
    saved: &str,
) -> Vec<(SizeChoice, String)> {
    let text = |(w, h): (u32, u32)| format!("{w}x{h}");
    let valid = |size: (u32, u32)| spec.parse(&text(size)).is_ok();
    let mut choices: Vec<(SizeChoice, String)> = spec
        .unset
        .map(|label| (SizeChoice::Unset, label.to_owned()))
        .into_iter()
        .collect();
    let screen = screen.and_then(rea_launcher::display::landscape);
    if let Some(size) = screen.filter(|&size| valid(size))
        && !COMMON_SIZES.iter().any(|(common, _)| *common == size)
    {
        choices.push((
            SizeChoice::Text(text(size)),
            format!("Screen {}×{}", size.0, size.1),
        ));
    }
    for (size, label) in COMMON_SIZES {
        if valid(size) {
            choices.push((SizeChoice::Text(text(size)), label.to_owned()));
        }
    }
    let saved = saved.trim();
    if !saved.is_empty()
        && spec.parse(saved).is_ok()
        && !choices
            .iter()
            .any(|(choice, _)| matches!(choice, SizeChoice::Text(t) if same_size(t, saved)))
    {
        choices.push((
            SizeChoice::Text(saved.to_owned()),
            saved.replace(['x', 'X'], "×"),
        ));
    }
    choices.push((SizeChoice::Custom, "Custom".to_owned()));
    choices
}

fn same_size(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// A `Size` row's chips, and the Custom field while Custom is chosen.
fn size_chips(
    ui: &mut Ui,
    spec: &OptionSpec,
    value: &mut String,
    screen: Option<(u32, u32)>,
    disabled: Option<&str>,
) -> Response {
    let custom_id = ui.next_auto_id().with("rea-size-custom");
    let mut custom = ui
        .data(|data| data.get_temp::<bool>(custom_id))
        .unwrap_or(false);
    let choices = size_choices(spec, screen, value);
    let listed = |value: &str| {
        choices.iter().find_map(|(choice, _)| match choice {
            SizeChoice::Unset if value.trim().is_empty() => Some(SizeChoice::Unset),
            SizeChoice::Text(t) if same_size(t, value) => Some(choice.clone()),
            _ => None,
        })
    };
    let mut current = if custom {
        SizeChoice::Custom
    } else {
        listed(value).unwrap_or(SizeChoice::Custom)
    };
    let chips: Vec<Chip<'_, SizeChoice>> = choices
        .iter()
        .map(|(choice, label)| Chip::new(choice.clone(), label.as_str()).disabled(disabled))
        .collect();
    let response = ui
        .vertical(|ui| {
            let response = quiet_chips(ui, &chips, &mut current);
            if response.changed() {
                custom = current == SizeChoice::Custom;
                match &current {
                    SizeChoice::Unset => value.clear(),
                    SizeChoice::Text(text) => text.clone_into(value),
                    SizeChoice::Custom => {}
                }
            }
            if current == SizeChoice::Custom {
                let field = text_field(ui, spec, value, 200.0, disabled.is_none());
                widgets::name_control(&field, Some(spec.label), Some("Any size, as WIDTHxHEIGHT."));
            }
            response
        })
        .inner;
    ui.data_mut(|data| data.insert_temp(custom_id, custom));
    response
}

/// Chips whose disabled reasons the row writes itself.
fn quiet_chips<T: PartialEq + Clone>(
    ui: &mut Ui,
    chips: &[Chip<'_, T>],
    current: &mut T,
) -> Response {
    widgets::chips_noted(ui, chips, current, widgets::ChipSize::Normal, false)
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
    fn a_disabled_row_writes_its_reason_in_place_of_the_description() {
        const ROWS: &[OptionSpec] = &[
            OptionSpec::new(
                "vsync",
                Kind::Toggle,
                "VSync",
                "Waits for the screen's refresh.",
            ),
            OptionSpec::new(
                "display_mode",
                Kind::Choice(MODES),
                "Display",
                "How the game shows.",
            ),
            OptionSpec::new("name", Kind::Text, "Profile", "The driver's name."),
        ];
        let ui = testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            vec![String::from("on"), "window".into(), "Colin".into()],
            |ui, values: &mut Vec<String>| {
                for (spec, value) in ROWS.iter().zip(values.iter_mut()) {
                    spec_row_with(
                        ui,
                        spec,
                        value,
                        RowOptions {
                            disabled: Some("Needs DXVK."),
                            ..RowOptions::default()
                        },
                    );
                }
            },
        );
        // Every row shows the reason once, and not its description.
        assert_eq!(ui.get_all_by_label("Needs DXVK.").count(), 3);
        assert!(ui.query_by_label("How the game shows.").is_none());
        // The switch still shows the saved value.
        let vsync = ui.get_by_role_and_label(Role::Switch, "VSync");
        assert!(testing::checked(&vsync));
        assert!(testing::disabled(&vsync));
        // Screen readers get both, as two sentences.
        assert_eq!(
            testing::description(&vsync).as_deref(),
            Some("Waits for the screen's refresh. Needs DXVK.")
        );
    }

    const RESOLUTION: OptionSpec = OptionSpec::new(
        "resolution",
        Kind::Size {
            min: (640, 480),
            max: (3840, 2160),
            aliases: rea_launcher::cfg::SIZES_ONLY,
        },
        "Resolution",
        "The game's picture size.",
    )
    .unset_label("Auto");

    const FOV: OptionSpec = OptionSpec::new(
        "fov",
        Kind::Range { min: 30, max: 150 },
        "Field of view",
        "",
    )
    .unset_label("As the game");

    const FOVS: &[(u32, &str)] = &[(60, "60°"), (75, "75°"), (90, "90°")];

    const PADS: &[(&str, &str)] = &[("rally", "Rally"), ("standard", "Standard")];
    const PAD: OptionSpec = OptionSpec::new(
        "gamepad",
        Kind::Choice(PADS),
        "Gamepad",
        "How the pad is mapped.",
    );
    const PAD_NOTES: &[(&str, &str)] = &[
        ("rally", "Triggers steer the throttle and brake."),
        ("standard", "Sticks only, like a modern game."),
    ];

    #[test]
    fn size_chips_come_from_the_screen() {
        let labels = |screen, saved| -> Vec<String> {
            size_choices(&RESOLUTION, screen, saved)
                .into_iter()
                .map(|(_, label)| label)
                .collect()
        };
        // A 16:10 laptop: its own size first, then the common ones.
        assert_eq!(
            labels(Some((2560, 1600)), ""),
            ["Auto", "Screen 2560×1600", "1080p", "1440p", "4K", "Custom"]
        );
        // A common screen size is not listed twice; a portrait screen
        // counts in landscape.
        assert_eq!(
            labels(Some((1920, 1080)), ""),
            ["Auto", "1080p", "1440p", "4K", "Custom"]
        );
        assert_eq!(labels(Some((1200, 1920)), "")[1], "Screen 1920×1200");
        // A saved size that is none of them keeps its own chip.
        assert_eq!(labels(None, "1280x720")[4], "1280×720");
        assert_eq!(
            labels(None, "1920X1080").len(),
            5,
            "the same size, any case"
        );
        // Sizes the spec refuses are left out.
        const SMALL: OptionSpec = OptionSpec::new(
            "resolution",
            Kind::Size {
                min: (640, 480),
                max: (1920, 1200),
                aliases: rea_launcher::cfg::SIZES_ONLY,
            },
            "Resolution",
            "",
        );
        assert_eq!(
            size_choices(&SMALL, Some((3840, 2160)), "")
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>(),
            ["1080p", "Custom"]
        );
    }

    #[test]
    fn step_rows_offer_unset_first_and_keep_a_saved_value() {
        assert_eq!(
            step_choices(&FOV, FOVS, Some(82)),
            [
                (None, "As the game".to_owned()),
                (Some(60), "60°".to_owned()),
                (Some(75), "75°".to_owned()),
                (Some(90), "90°".to_owned()),
                (Some(82), "82".to_owned()),
            ]
        );
        assert_eq!(step_choices(&SPECS[2], FOVS, Some(60)).len(), 3);
    }

    #[derive(Default)]
    struct Rows {
        resolution: String,
        fov: String,
        pad: String,
    }

    fn rows_window(ui: &mut Ui, rows: &mut Rows) {
        spec_row_with(
            ui,
            &RESOLUTION,
            &mut rows.resolution,
            RowOptions {
                sizes: Some(SizeChips {
                    screen: Some((2560, 1600)),
                }),
                ..RowOptions::default()
            },
        );
        spec_row_with(
            ui,
            &FOV,
            &mut rows.fov,
            RowOptions {
                steps: Some(FOVS),
                ..RowOptions::default()
            },
        );
        spec_row_with(
            ui,
            &PAD,
            &mut rows.pad,
            RowOptions {
                notes: Some(PAD_NOTES),
                ..RowOptions::default()
            },
        );
    }

    #[test]
    fn size_step_and_noted_rows_end_to_end() {
        let mut ui = testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            Rows {
                resolution: "1920x1080".into(),
                fov: "75".into(),
                pad: "rally".into(),
            },
            rows_window,
        );
        let row = ui.get_by_role_and_label(Role::RadioGroup, "Resolution");
        assert_eq!(testing::value(&row).as_deref(), Some("1080p"));
        ui.get_by_label("Screen 2560×1600").click();
        ui.run();
        assert_eq!(ui.state().resolution, "2560x1600");
        ui.get_by_label("Auto").click();
        ui.run();
        assert_eq!(ui.state().resolution, "");
        // Custom opens a field for any size.
        assert!(ui.query_by_role(Role::TextInput).is_none());
        ui.get_by_label("Custom").click();
        ui.run();
        let field = ui.get_by_role_and_label(Role::TextInput, "Resolution");
        field.click();
        ui.run();
        ui.get_by_role_and_label(Role::TextInput, "Resolution")
            .type_text("1600x900");
        ui.run();
        assert_eq!(ui.state().resolution, "1600x900");
        assert!(testing::checked(&ui.get_by_label("Custom")));

        // The unset chip of a stepped row writes the empty text.
        let fov = ui.get_by_role_and_label(Role::RadioGroup, "Field of view");
        assert_eq!(testing::value(&fov).as_deref(), Some("75°"));
        ui.get_by_label("As the game").click();
        ui.run();
        assert_eq!(ui.state().fov, "");
        ui.get_by_label("90°").click();
        ui.run();
        assert_eq!(ui.state().fov, "90");

        // The chosen value's note replaces the description.
        assert!(
            ui.query_by_label("Triggers steer the throttle and brake.")
                .is_some()
        );
        assert!(ui.query_by_label("How the pad is mapped.").is_none());
        ui.get_by_label("Standard").click();
        ui.run();
        assert!(
            ui.query_by_label("Sticks only, like a modern game.")
                .is_some()
        );
        let pad = ui.get_by_role_and_label(Role::RadioGroup, "Gamepad");
        assert_eq!(
            testing::description(&pad).as_deref(),
            Some("How the pad is mapped. Sticks only, like a modern game.")
        );
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
        // Written on the window, once (not only on hover).
        assert_eq!(ui.get_all_by_label("Needs DXVK").count(), 1);
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
