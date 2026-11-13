//! The Wheel tab of a DirectInput game's settings window: the
//! `rea_wheel.ini` the in-game wheel layer reads
//! (`rea_launcher::wheel::dinput`), with a live check of what the game
//! will get, the pedal wizard and a force-feedback test.
//!
//! The launcher keeps a [`WheelTab`] and a [`WheelDevices`]
//! (`rea_launcher_win32::dinput::DirectInput` on Windows and Wine, a
//! `rig::Fixed` in tests), draws the tab inside `widgets::tab_body`, and on
//! Play writes [`WheelTab::save`]'s text. What the live check shows is
//! [`rig::virtual_controls`], the function the in-game layer calls.
//!
//! The order follows what a player does: wheel support, which wheel and
//! pedals, the live check, steering, pedals (with calibration), force
//! feedback. Every control is a chip row, switch or button of
//! [`crate::widgets`], so keyboard and pad focus walk them top to bottom
//! and screen readers hear each one's name and what it does; the meters
//! are AccessKit meters whose value is the text beside them.

use std::time::Duration;

use eframe::egui::{
    self, CornerRadius, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, Ui, WidgetInfo,
    vec2,
};
use rea_launcher::wheel::dinput::{Mode, NoButtons, Profile, REAL_AXES, Setup};
use rea_launcher::wheel::rig::{self, Device, FfbTest, Rig, WheelDevices};
use rea_launcher::wheel::wizard::{Status, Step, Wizard};
use rea_launcher::wheel::{DeviceId, DeviceState, Kind, PedalAxis, Source, SteeringLock};

use crate::theme::{self, body, size};
use crate::widgets;

/// The comment a new `rea_wheel.ini` starts with.
pub const HEADER: &str = "rea_wheel.ini - the wheel settings, written by the launcher";

/// How often the tab redraws while it is shown (the live check).
pub const REDRAW: Duration = Duration::from_millis(33);

/// Deadzone and curve chips; a saved value in between gets its own chip.
const DEADZONES: [u8; 6] = [0, 2, 4, 6, 8, 10];
const CURVES: [u8; 6] = [0, 20, 40, 60, 80, 100];
const STRENGTHS: [u8; 5] = [0, 25, 50, 75, 100];

/// The tab's state: what it edits and a pedal calibration in progress.
pub struct WheelTab {
    pub setup: Setup,
    /// The calibration read from the file or made by the pedal wizard;
    /// `None`: none yet (the tab and the game guess one each time, and
    /// nothing is saved).
    pub profile: Option<Profile>,
    calibration: Option<Calibration>,
    /// What the last calibration did.
    calibrated: Option<&'static str>,
}

/// A pedal wizard run.
struct Calibration {
    wizard: Wizard<NoButtons>,
    /// The devices it watches, the wheel first.
    order: Vec<DeviceId>,
    /// The calibration it adds to.
    base: Profile,
}

impl WheelTab {
    /// The tab for `rea_wheel.ini`'s text (`None`: no file yet).
    pub fn open(text: Option<&str>) -> WheelTab {
        let text = text.unwrap_or_default();
        WheelTab {
            setup: Setup::from_cfg(text),
            profile: Profile::from_cfg(text),
            calibration: None,
            calibrated: None,
        }
    }

    /// The new `rea_wheel.ini`: `existing` (the file as it is now, "" for
    /// none) with the tab's settings and calibration, every other line
    /// kept.
    pub fn save(&self, existing: &str) -> String {
        rig::save(existing, &self.setup, self.profile.as_ref(), HEADER)
    }

    /// Whether the pedal wizard is running (the launcher may hold Play).
    pub fn calibrating(&self) -> bool {
        self.calibration.is_some()
    }

    /// The tab, top to bottom.
    pub fn draw(&mut self, ui: &mut Ui, devices: &mut dyn WheelDevices) {
        ui.ctx().request_repaint_after(REDRAW);
        let list = devices.devices();
        self.mode_row(ui);
        if self.setup.mode == Mode::Off {
            return;
        }
        let rig = rig::pick(&list, &self.setup, self.profile.as_ref());
        let in_use = rig.map(|r| rig::profile(self.profile.as_ref(), &list, r));
        section(ui, "Wheel and pedals");
        self.device_rows(ui, &list, rig, in_use.as_ref(), devices);
        section(ui, "Live check");
        self.live_check(ui, &list, rig, in_use.as_ref());
        section(ui, "Steering");
        self.steering_rows(ui);
        section(ui, "Pedals");
        self.pedal_rows(ui, &list, rig, in_use);
        section(ui, "Force feedback");
        self.ffb_rows(ui, &list, rig, devices);
        ui.add_space(10.0);
        widgets::toggle(
            ui,
            &mut self.setup.log,
            "Keep a log for bug reports (rea_wheel.log)",
            None,
        );
    }

    fn mode_row(&mut self, ui: &mut Ui) {
        let note = match self.setup.mode {
            Mode::Auto => {
                "When a wheel is plugged in as the game starts, the game sees one wheel made from it and your pedals."
            }
            Mode::On => "The game always sees that wheel, centred until yours is plugged in.",
            Mode::Off => "The game sees your controllers as they are, without these settings.",
        };
        let options = [(Mode::Auto, "Auto"), (Mode::On, "On"), (Mode::Off, "Off")];
        row(ui, "Wheel support", note, |ui| {
            widgets::chips(ui, &options, &mut self.setup.mode, None)
        });
    }

    fn device_rows(
        &mut self,
        ui: &mut Ui,
        list: &[Device],
        rig: Option<Rig>,
        in_use: Option<&Profile>,
        devices: &mut dyn WheelDevices,
    ) {
        let name = |i: usize| list[i].id.name.as_str();
        let chosen = self.setup.wheel.device.clone();
        let chosen_here = list.iter().any(|d| d.id.guid == chosen);
        let wheel_note = match (rig, in_use) {
            _ if list.is_empty() => {
                "No controller is connected. Plug in your wheel: it shows up here within two seconds.".to_owned()
            }
            (Some(r), Some(p)) if !chosen.is_empty() && !chosen_here => format!(
                "The wheel you picked isn't connected. Using {}, {}° lock to lock, until it is.",
                name(r.wheel),
                p.rotation
            ),
            (Some(r), Some(p)) if chosen.is_empty() => {
                format!("Using {}, {}° lock to lock.", name(r.wheel), p.rotation)
            }
            (Some(_), Some(p)) => format!("{}° lock to lock.", p.rotation),
            _ => {
                let then = match self.setup.mode {
                    Mode::On => "The game sees a centred wheel until one is plugged in.",
                    _ => "The game sees your controllers as they are.",
                };
                format!("None of your controllers is a wheel. {then}")
            }
        };
        let mut wheels = vec![(String::new(), "Automatic".to_owned())];
        wheels.extend(labels(list, |d| d.kind != Kind::Pad));
        if !chosen.is_empty() && !chosen_here {
            wheels.push((chosen, "Not connected".to_owned()));
        }
        row(ui, "Wheel", &wheel_note, |ui| {
            widgets::chips(ui, &wheels, &mut self.setup.wheel.device, None)
        });

        let wheel_guid = rig.map(|r| list[r.wheel].id.guid.clone());
        let picked = self.setup.pedals.clone();
        let pedal_note = match rig.and_then(|r| r.pedals) {
            Some(i) => format!("Using {}.", name(i)),
            None if !picked.is_empty() => {
                "The pedals you picked aren't connected, so they read as released.".to_owned()
            }
            None => "On the wheel, or the pedals you calibrate.".to_owned(),
        };
        let mut pedals = vec![(String::new(), "Automatic".to_owned())];
        pedals.extend(labels(list, |d| {
            d.kind != Kind::Pad && Some(&d.id.guid) != wheel_guid.as_ref()
        }));
        if !picked.is_empty() && !pedals.iter().any(|(g, _)| *g == picked) {
            pedals.push((picked, "Not connected".to_owned()));
        }
        row(ui, "Pedals", &pedal_note, |ui| {
            widgets::chips(ui, &pedals, &mut self.setup.pedals, None)
        });
        ui.horizontal(|ui| {
            ui.add_space(theme::metrics(ui.ctx()).field_width);
            if widgets::outline_button(ui, "Look for devices again").clicked() {
                devices.refresh();
            }
        });
    }

    fn live_check(&self, ui: &mut Ui, list: &[Device], rig: Option<Rig>, in_use: Option<&Profile>) {
        let (Some(rig), Some(profile)) = (rig, in_use) else {
            widgets::note(ui, "The live check needs a wheel.");
            return;
        };
        let aux = rig.pedals.map(|i| &list[i].state);
        let c = rig::virtual_controls(&self.setup, profile, &list[rig.wheel].state, aux);
        steering_meter(ui, c.steer);
        pedal_meter(ui, "Throttle", c.accel);
        pedal_meter(ui, "Brake", c.brake);
        widgets::note(
            ui,
            "What the game will get: turn the wheel and press the pedals to check them.",
        );
    }

    fn steering_rows(&mut self, ui: &mut Ui) {
        let locks: Vec<(SteeringLock, String)> = SteeringLock::STEPS
            .iter()
            .map(|&l| (l, l.label()))
            .collect();
        row(
            ui,
            "Steering lock",
            "How far you turn the wheel for full lock in the game. Full is the wheel's whole rotation.",
            |ui| widgets::chips(ui, &locks, &mut self.setup.wheel.lock, None),
        );
        let deadzones = steps(&DEADZONES, self.setup.deadzone, |v| match v {
            0 => "None".to_owned(),
            v => format!("{v} %"),
        });
        row(
            ui,
            "Deadzone",
            "A small band in the centre that does nothing, for a wheel that drifts.",
            |ui| widgets::chips(ui, &deadzones, &mut self.setup.deadzone, None),
        );
        let curves = steps(&CURVES, self.setup.linearity, |v| {
            rea_launcher::wheel::linearity_label(v).to_owned()
        });
        row(
            ui,
            "Curve",
            "Linear steers as much as you turn; the others soften small turns near the centre.",
            |ui| widgets::chips(ui, &curves, &mut self.setup.linearity, None),
        );
    }

    fn pedal_rows(
        &mut self,
        ui: &mut Ui,
        list: &[Device],
        rig: Option<Rig>,
        in_use: Option<Profile>,
    ) {
        widgets::toggle(
            ui,
            &mut self.setup.wheel.invert_accel,
            "Invert throttle",
            None,
        );
        widgets::toggle(ui, &mut self.setup.wheel.invert_brake, "Invert brake", None);
        if let Some(profile) = &in_use {
            let device = |src: Source| match (src, &profile.aux) {
                (Source::Aux, Some(aux)) => aux.name.clone(),
                _ => "the wheel".to_owned(),
            };
            for (pedal, value) in [("Throttle", profile.gas), ("Brake", profile.brake)] {
                widgets::note(ui, &pedal_text(pedal, value, device));
            }
        }
        if self.calibration.is_some() {
            self.calibrate(ui, list);
            return;
        }
        match (rig, in_use) {
            (Some(rig), Some(base)) => {
                if widgets::outline_button(ui, "Calibrate pedals").clicked() {
                    self.start_calibration(list, rig, base);
                }
            }
            _ => {
                widgets::note(ui, "Calibrating the pedals needs a wheel.");
            }
        }
        if let Some(done) = self.calibrated {
            widgets::note(ui, done);
        }
    }

    fn start_calibration(&mut self, list: &[Device], rig: Rig, base: Profile) {
        let others = (0..list.len()).filter(|&i| i != rig.wheel && list[i].kind != Kind::Pad);
        let order = std::iter::once(rig.wheel)
            .chain(others)
            .map(|i| list[i].id.clone())
            .collect();
        self.calibration = Some(Calibration {
            wizard: Wizard::new(vec![Step::Accelerator, Step::Brake], base.steer_axis),
            order,
            base,
        });
        self.calibrated = None;
    }

    /// One frame of the wizard: feed it, then say what to do.
    fn calibrate(&mut self, ui: &mut Ui, list: &[Device]) {
        let Some(run) = &mut self.calibration else {
            return;
        };
        let states: Vec<DeviceState> = run
            .order
            .iter()
            .map(|id| {
                list.iter()
                    .find(|d| d.id.guid == id.guid)
                    .map(|d| d.state.clone())
                    .unwrap_or_default()
            })
            .collect();
        let now_ms = (ui.input(|i| i.time) * 1000.0) as u64;
        let pedal = |step: Step<NoButtons>| match step {
            Step::Brake => "brake",
            _ => "throttle",
        };
        match run.wizard.tick(now_ms, &states) {
            Status::WarmingUp => {
                prompt(ui, "Keep your feet off the pedals.");
            }
            Status::Waiting {
                step, remaining_ms, ..
            } => {
                prompt(
                    ui,
                    &format!("Press the {} all the way down, then let go.", pedal(step)),
                );
                widgets::note(
                    ui,
                    &format!(
                        "{} s left, then this pedal is skipped.",
                        remaining_ms.div_ceil(1000)
                    ),
                );
            }
            Status::Pressing { step, travel, .. } => {
                prompt(ui, &format!("Now let go of the {}.", pedal(step)));
                widgets::progress_bar(ui, travel);
            }
            Status::Done(captures) => {
                let mut profile = run.base.clone();
                let found = captures.gas.is_some() || captures.brake.is_some();
                if found {
                    rig::calibrate(&mut profile, &mut self.setup, &captures, &run.order);
                    self.profile = Some(profile);
                }
                self.calibrated = Some(if found {
                    "Pedals calibrated."
                } else {
                    "No pedal moved, so the calibration is unchanged."
                });
                self.calibration = None;
                return;
            }
        }
        if widgets::outline_button(ui, "Stop calibrating").clicked() {
            self.calibration = None;
        }
    }

    fn ffb_rows(
        &mut self,
        ui: &mut Ui,
        list: &[Device],
        rig: Option<Rig>,
        devices: &mut dyn WheelDevices,
    ) {
        let strengths = steps(&STRENGTHS, self.setup.ffb.strength, |v| match v {
            0 => "Off".to_owned(),
            v => format!("{v} %"),
        });
        row(
            ui,
            "Strength",
            "How hard the wheel pushes back in the game. Go lower on a direct-drive wheel.",
            |ui| widgets::chips(ui, &strengths, &mut self.setup.ffb.strength, None),
        );
        widgets::toggle(ui, &mut self.setup.ffb.invert, "Reverse the force", None);
        widgets::note(ui, "Only for a wheel that pulls the wrong way.");
        let wheel = rig.map(|r| &list[r.wheel]).filter(|w| w.ffb);
        let Some(wheel) = wheel else {
            widgets::note(ui, "The test needs a wheel with force feedback.");
            return;
        };
        let status = devices.ffb_status();
        let test = widgets::outline_button(ui, "Test force feedback");
        widgets::name_control(
            &test,
            None,
            Some("Pushes the wheel left, then right, for a second and a half."),
        );
        if test.clicked() && status != FfbTest::Playing {
            devices.ffb_test(&wheel.id.guid, self.setup.ffb);
        }
        match status {
            FfbTest::Idle => {}
            FfbTest::Playing => {
                widgets::note(ui, "Pushing left, then right…");
            }
            FfbTest::Done => {
                widgets::note(
                    ui,
                    "Did it push left first? If it went right, turn on Reverse the force.",
                );
            }
            FfbTest::Failed(why) => {
                widgets::error_note(ui, &why);
            }
        }
    }
}

/// A section heading with room above it.
fn section(ui: &mut Ui, text: &str) {
    ui.add_space(14.0);
    widgets::heading(ui, text);
    ui.add_space(4.0);
}

/// A labelled row: the field on the left; the control and `note` under it
/// on the right. The control is named by the label and described by the
/// note.
fn row(ui: &mut Ui, label: &str, note: &str, control: impl FnOnce(&mut Ui) -> Response) {
    ui.horizontal_top(|ui| {
        widgets::field(ui, label);
        ui.vertical(|ui| {
            let response = control(ui);
            widgets::name_control(&response, Some(label), Some(note));
            ui.add(egui::Label::new(
                RichText::new(note)
                    .font(body(size::NOTE))
                    .color(theme::palette(ui.ctx()).muted),
            ));
        });
    });
    ui.add_space(8.0);
}

/// What the wizard asks for now, in ink.
fn prompt(ui: &mut Ui, text: &str) {
    let ink = theme::palette(ui.ctx()).ink;
    ui.label(RichText::new(text).font(body(size::LABEL)).color(ink));
}

/// The chips for `list`'s devices that `keep` keeps: instance GUID and
/// name, numbered when two share a name (a wheel's twin collections).
fn labels(list: &[Device], keep: impl Fn(&Device) -> bool) -> Vec<(String, String)> {
    let kept: Vec<&Device> = list.iter().filter(|d| keep(d)).collect();
    kept.iter()
        .enumerate()
        .map(|(i, d)| {
            let twins = kept.iter().filter(|o| o.id.name == d.id.name).count();
            let label = if twins > 1 {
                let n = kept[..=i].iter().filter(|o| o.id.name == d.id.name).count();
                format!("{} ({n})", d.id.name)
            } else {
                d.id.name.clone()
            };
            (d.id.guid.clone(), label)
        })
        .collect()
}

/// Chips for `values`, plus `current` in order when it is none of them.
fn steps(values: &[u8], current: u8, label: impl Fn(u8) -> String) -> Vec<(u8, String)> {
    let mut all = values.to_vec();
    if !all.contains(&current) {
        all.push(current);
        all.sort_unstable();
    }
    all.into_iter().map(|v| (v, label(v))).collect()
}

/// "Throttle: axis Y on the wheel", or what to do when it has none.
fn pedal_text(
    pedal: &str,
    value: Option<(Source, PedalAxis)>,
    device: impl Fn(Source) -> String,
) -> String {
    match value {
        Some((src, axis)) => format!(
            "{pedal}: axis {} on {}",
            REAL_AXES.get(axis.axis).copied().unwrap_or("?"),
            device(src)
        ),
        None => format!("{pedal}: not found yet. Calibrate the pedals."),
    }
}

/// "Centre", "Left 35 %", "Right 100 %".
pub fn steer_text(steer: f32) -> String {
    let percent = (steer.abs() * 100.0).round() as u32;
    match percent {
        0 => "Centre".to_owned(),
        p if steer < 0.0 => format!("Left {p} %"),
        p => format!("Right {p} %"),
    }
}

/// "62 %".
pub fn pedal_value_text(value: f32) -> String {
    format!("{} %", (value.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// A labelled bar with its value as text beside it; `paint` fills the
/// track. Accessibility: a meter named `label` whose value is the text.
fn meter(
    ui: &mut Ui,
    label: &str,
    text: &str,
    paint: impl FnOnce(&egui::Painter, Rect, egui::Color32),
) {
    ui.horizontal_top(|ui| {
        widgets::field(ui, label);
        let p = theme::palette(ui.ctx());
        let width = (ui.available_width() - 100.0).max(80.0);
        let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
        let track = Rect::from_center_size(rect.center(), vec2(width, 12.0));
        let painter = ui.painter();
        painter.rect(
            track,
            CornerRadius::same(6),
            p.panel,
            Stroke::new(1.0, p.line),
            StrokeKind::Inside,
        );
        paint(painter, track, p.select);
        ui.add_space(10.0);
        ui.label(RichText::new(text).font(body(size::LABEL)).color(p.ink));
        response.widget_info(|| WidgetInfo::labeled(Role::Meter, true, label));
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.set_value(text.to_owned()));
    });
}

/// Steering: filled from the centre mark towards the side it turns.
fn steering_meter(ui: &mut Ui, steer: f32) {
    let muted = theme::palette(ui.ctx()).muted;
    meter(
        ui,
        "Steering",
        &steer_text(steer),
        |painter, track, fill| {
            let centre = track.center().x;
            let x = centre + steer.clamp(-1.0, 1.0) * track.width() / 2.0;
            let bar = Rect::from_x_y_ranges(centre.min(x)..=centre.max(x), track.y_range());
            painter.rect_filled(bar, CornerRadius::same(6), fill);
            painter.vline(
                centre,
                track.top() - 4.0..=track.bottom() + 4.0,
                Stroke::new(2.0, muted),
            );
        },
    );
}

/// A pedal: filled from the left as it is pressed.
fn pedal_meter(ui: &mut Ui, label: &str, value: f32) {
    meter(
        ui,
        label,
        &pedal_value_text(value),
        |painter, track, fill| {
            let width = track.width() * value.clamp(0.0, 1.0);
            let bar = Rect::from_min_size(track.min, vec2(width, track.height()));
            painter.rect_filled(bar, CornerRadius::same(6), fill);
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, Harness, NodeT, Queryable};
    use crate::theme::{Metrics, Palette};
    use rea_launcher::ffb::FfbSettings;
    use rea_launcher::wheel::rig::Fixed;

    type State = (WheelTab, Fixed);

    fn device(name: &str, vid: u16, pid: u16, guid: &str, kind: Kind, axes: Vec<i16>) -> Device {
        let state = DeviceState {
            axes,
            buttons: vec![false; 12],
        };
        Device {
            id: DeviceId {
                name: name.into(),
                vid,
                pid,
                guid: guid.into(),
            },
            kind,
            axes: 8,
            buttons: 12,
            ffb: kind == Kind::Wheel,
            initial: state.clone(),
            state,
        }
    }

    fn pad() -> Device {
        device("Xbox pad", 0x045e, 0x028e, "{pad}", Kind::Pad, vec![0; 8])
    }

    fn csl() -> Device {
        let axes = vec![0, 32_767, 32_767, 0, 0, 0, 0, 0];
        device("FANATEC CSL DD", 0x0eb7, 0x0020, "{csl}", Kind::Wheel, axes)
    }

    fn pedals() -> Device {
        let axes = vec![-32_768, -32_768, 0, 0, 0, 0, 0, 0];
        device(
            "Sprint pedals",
            0x30b7,
            0x1001,
            "{pedals}",
            Kind::Other,
            axes,
        )
    }

    fn open(devices: Vec<Device>, text: &str) -> Harness<'static, State> {
        let fixed = Fixed {
            devices,
            ..Fixed::default()
        };
        // Tall enough that the whole tab is in view.
        testing::harness_sized(
            &Palette::default(),
            &Metrics::DEFAULT,
            vec2(880.0, 1800.0),
            (WheelTab::open(Some(text)), fixed),
            |ui, (tab, devices)| tab.draw(ui, devices),
        )
    }

    fn click(ui: &mut Harness<'_, State>, role: Role, label: &str, nth: usize) {
        ui.get_all_by_role_and_label(role, label)
            .nth(nth)
            .unwrap_or_else(|| panic!("{label} #{nth}"))
            .click();
        ui.run();
    }

    fn saved(ui: &Harness<'_, State>) -> String {
        ui.state().0.save("")
    }

    #[test]
    fn every_control_changes_the_saved_file() {
        let mut ui = open(vec![pad(), csl(), pedals()], "");
        let clicks: [(Role, &str, usize, &str); 11] = [
            (Role::RadioButton, "On", 0, "mode = on"),
            (Role::RadioButton, "Sprint pedals", 0, "device = {pedals}"),
            (Role::RadioButton, "Automatic", 0, "device = \n"),
            (Role::RadioButton, "Sprint pedals", 1, "pedals = {pedals}"),
            (Role::RadioButton, "540°", 0, "lock = 540"),
            (Role::RadioButton, "4 %", 0, "deadzone = 4"),
            (Role::RadioButton, "Mild", 0, "linearity = 40"),
            (Role::Switch, "Invert throttle", 0, "invert_accel = on"),
            (Role::Switch, "Invert brake", 0, "invert_brake = on"),
            (Role::RadioButton, "75 %", 0, "ffb_strength = 75"),
            (Role::Switch, "Reverse the force", 0, "ffb_invert = on"),
        ];
        for (role, label, nth, line) in clicks {
            click(&mut ui, role, label, nth);
            assert!(
                saved(&ui).contains(line),
                "{label}: {line} in\n{}",
                saved(&ui)
            );
        }
        click(
            &mut ui,
            Role::Switch,
            "Keep a log for bug reports (rea_wheel.log)",
            0,
        );
        let text = saved(&ui);
        assert!(text.contains("log = on"));
        assert!(text.starts_with(&format!("# {HEADER}\n")));
        assert_eq!(Setup::from_cfg(&text), ui.state().0.setup);
        click(&mut ui, Role::RadioButton, "Off", 0);
        assert!(saved(&ui).contains("mode = off"));
        assert!(
            ui.query_by_label("Steering lock").is_none(),
            "Off shows wheel support only"
        );
    }

    #[test]
    fn the_file_round_trips_and_keeps_other_lines() {
        let existing = "# mine\nmode = on\nlock = 900\nfuture_key = 7\n";
        let ui = open(vec![csl()], existing);
        let text = ui.state().0.save(existing);
        assert!(text.starts_with("# mine\n") && text.contains("future_key = 7"));
        assert_eq!(Setup::from_cfg(&text), Setup::from_cfg(existing));
        assert!(
            !text.contains("wheel_vid"),
            "no calibration, nothing guessed saved"
        );
    }

    #[test]
    fn empty_states_say_what_to_do() {
        let ui = open(Vec::new(), "");
        ui.get_by_label_contains("No controller is connected");
        ui.get_by_label("The live check needs a wheel.");
        ui.get_by_label("Calibrating the pedals needs a wheel.");
        ui.get_by_label("The test needs a wheel with force feedback.");

        let ui = open(vec![pad()], "mode = on\n");
        ui.get_by_label_contains("None of your controllers is a wheel");
        ui.get_by_label_contains("centred wheel until one is plugged in");

        let ui = open(vec![csl()], "device = {gone}\npedals = {gone too}\n");
        ui.get_by_label_contains("The wheel you picked isn't connected. Using FANATEC CSL DD");
        ui.get_by_label_contains("The pedals you picked aren't connected");
        assert_eq!(
            ui.get_all_by_role_and_label(Role::RadioButton, "Not connected")
                .count(),
            2,
            "the saved picks stay chosen"
        );
    }

    #[test]
    fn the_live_check_shows_what_the_game_gets() {
        let mut wheel = csl();
        wheel.state.axes[0] = 4_000;
        wheel.state.axes[1] = 0;
        let text = "deadzone = 4\nlinearity = 40\n";
        let ui = open(vec![wheel.clone()], text);
        ui.get_by_label_contains("Using FANATEC CSL DD, 1080° lock to lock");
        let setup = Setup::from_cfg(text);
        let devices = [wheel];
        let rig = rig::pick(&devices, &setup, None).unwrap();
        let profile = rig::profile(None, &devices, rig);
        let c = rig::virtual_controls(&setup, &profile, &devices[0].state, None);
        let value = |label| testing::value(&ui.get_by_role_and_label(Role::Meter, label));
        assert_eq!(value("Steering"), Some(steer_text(c.steer)));
        assert_eq!(value("Throttle"), Some(pedal_value_text(c.accel)));
        assert_eq!(value("Brake").as_deref(), Some("0 %"));
        assert!(steer_text(c.steer).starts_with("Right"));
    }

    #[test]
    fn the_pedal_wizard_saves_a_calibration() {
        let mut ui = open(vec![pad(), csl(), pedals()], "");
        ui.get_by_label("Throttle: axis Y on the wheel");
        click(&mut ui, Role::Button, "Calibrate pedals", 0);
        assert!(ui.state().0.calibrating());
        ui.run_steps(12);
        ui.get_by_label("Press the throttle all the way down, then let go.");
        for (axis, peak, next) in [(1, 30_000, "brake"), (0, 31_000, "")] {
            ui.state_mut().1.devices[2].state.axes[axis] = peak;
            ui.run_steps(2);
            ui.state_mut().1.devices[2].state.axes[axis] = -32_768;
            ui.run_steps(2);
            if !next.is_empty() {
                ui.get_by_label(&format!("Press the {next} all the way down, then let go."));
            }
        }
        ui.run();
        ui.get_by_label("Pedals calibrated.");
        let (tab, _) = ui.state();
        assert!(!tab.calibrating());
        assert_eq!(tab.setup.pedals, "{pedals}");
        let text = tab.save("");
        assert!(text.contains("aux_guid = {pedals}") && text.contains("gas_device = aux"));
        assert!(text.contains("gas_axis = 1") && text.contains("brake_axis = 0"));
        ui.get_by_label("Throttle: axis Y on Sprint pedals");
        ui.get_by_label("Brake: axis X on Sprint pedals");
    }

    #[test]
    fn a_calibration_can_be_stopped_or_time_out_unchanged() {
        let mut ui = open(vec![csl()], "");
        click(&mut ui, Role::Button, "Calibrate pedals", 0);
        click(&mut ui, Role::Button, "Stop calibrating", 0);
        assert!(!ui.state().0.calibrating());
        click(&mut ui, Role::Button, "Calibrate pedals", 0);
        // two pedals × 10 s at a quarter second a frame
        ui.run_steps(100);
        ui.get_by_label("No pedal moved, so the calibration is unchanged.");
        assert!(ui.state().0.profile.is_none());
    }

    #[test]
    fn the_force_feedback_test_goes_to_the_wheel_in_use() {
        let mut ui = open(vec![pad(), csl()], "ffb_strength = 25\nffb_invert = on\n");
        click(&mut ui, Role::Button, "Test force feedback", 0);
        let expected = FfbSettings {
            strength: 25,
            invert: true,
            ..FfbSettings::default()
        };
        assert_eq!(ui.state().1.tests, vec![("{csl}".to_owned(), expected)]);
        ui.get_by_label("Pushing left, then right…");
        ui.state_mut().1.status = FfbTest::Failed("The wheel refused the test force.".into());
        ui.run();
        ui.get_by_label("The wheel refused the test force.");
        click(&mut ui, Role::Button, "Look for devices again", 0);
        assert_eq!(ui.state().1.refreshes, 1);
    }

    /// Tab (the pad's d-pad down) walks every control, top to bottom.
    #[test]
    fn keyboard_and_pad_reach_every_control_in_order() {
        let mut ui = open(vec![pad(), csl()], "");
        let mut seen: Vec<String> = Vec::new();
        for _ in 0..20 {
            ui.key_press(egui::Key::Tab);
            ui.run();
            if let Some(node) = ui.query_by(|n| n.is_focused()) {
                let label = node.accesskit_node().label().unwrap_or_default();
                if !seen.contains(&label) {
                    seen.push(label);
                }
            }
        }
        let expected = [
            "Wheel support",
            "Wheel",
            "Pedals",
            "Look for devices again",
            "Steering lock",
            "Deadzone",
            "Curve",
            "Invert throttle",
            "Invert brake",
            "Calibrate pedals",
            "Strength",
            "Reverse the force",
            "Test force feedback",
            "Keep a log for bug reports (rea_wheel.log)",
        ];
        assert_eq!(seen, expected);
    }

    #[test]
    fn twin_collections_get_numbered_chips_and_saved_values_their_own() {
        let mut twin = csl();
        twin.id.guid = "{csl2}".into();
        let chips = labels(&[csl(), twin, pad()], |d| d.kind != Kind::Pad);
        let names: Vec<&str> = chips.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(names, ["FANATEC CSL DD (1)", "FANATEC CSL DD (2)"]);
        let deadzones = steps(&DEADZONES, 3, |v| v.to_string());
        assert_eq!(deadzones[2], (3, "3".to_owned()));
        assert_eq!(steps(&DEADZONES, 4, |v| v.to_string()).len(), 6);
        assert_eq!(steer_text(-0.351), "Left 35 %");
        assert_eq!(steer_text(0.001), "Centre");
        assert_eq!(pedal_value_text(1.2), "100 %");
    }
}
