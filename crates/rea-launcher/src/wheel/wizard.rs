//! The wheel setup wizard: press each pedal and button when asked. From
//! rea-rad-mobile `wheel/wizard.rs`, generic over the launcher's buttons.
//!
//! A pure state machine fed with device snapshots, so it is tested without
//! hardware. Thresholds follow Wanszai's wizard: an axis is captured when
//! it moves more than [`ARM_DELTA`] from where it rested and is let go
//! again (back within [`RELEASE_DELTA`]); a button when it goes down.
//! Steps time out ([`AXIS_TIMEOUT_MS`], [`BUTTON_TIMEOUT_MS`]) and are
//! skipped; an armed pedal that never comes back (a sticky or offset
//! pedal, or its device unplugged) times out [`AXIS_TIMEOUT_MS`] after it
//! armed (rea-rad-mobile waited forever). Device 0 is the wheel; the others
//! are candidates for separate pedals or a button box. The prompts are the
//! launcher's.

use super::{Control, DeviceState, PedalAxis};

pub const ARM_DELTA: i32 = 16_000;
pub const RELEASE_DELTA: i32 = 8_000;
pub const AXIS_TIMEOUT_MS: u64 = 10_000;
pub const BUTTON_TIMEOUT_MS: u64 = 5_000;
/// Readings to wait before taking a step's baseline.
pub const WARM_UP_TICKS: u32 = 10;

/// One thing the wizard asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<B> {
    Accelerator,
    Brake,
    Button(B),
}

impl<B> Step<B> {
    fn timeout_ms(&self) -> u64 {
        match self {
            Step::Accelerator | Step::Brake => AXIS_TIMEOUT_MS,
            Step::Button(_) => BUTTON_TIMEOUT_MS,
        }
    }
}

/// What the wizard learned. Device indices refer to the snapshot order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Captures<B> {
    pub gas: Option<(usize, PedalAxis)>,
    pub brake: Option<(usize, PedalAxis)>,
    /// (button, device, button number)
    pub buttons: Vec<(B, usize, usize)>,
    pub skipped: Vec<Step<B>>,
}

impl<B> Default for Captures<B> {
    fn default() -> Self {
        Self {
            gas: None,
            brake: None,
            buttons: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Armed {
    device: usize,
    axis: usize,
    rest: i16,
    peak: i16,
    since_ms: u64,
}

/// The wizard's state.
#[derive(Debug, Clone)]
pub struct Wizard<B> {
    /// What to ask for, in order: every step for a full setup, one to
    /// remap a single control.
    steps: Vec<Step<B>>,
    step: usize,
    step_started_ms: Option<u64>,
    ticks: u32,
    baseline: Vec<DeviceState>,
    armed: Option<Armed>,
    /// The wheel's steering axis is never taken for a pedal.
    steer_axis: usize,
    result: Captures<B>,
}

/// What to show after a tick.
#[derive(Debug, Clone, PartialEq)]
pub enum Status<B> {
    WarmingUp,
    Waiting {
        step: Step<B>,
        index: usize,
        remaining_ms: u64,
    },
    /// A pedal is being pressed (0..1 of its travel so far).
    Pressing {
        step: Step<B>,
        index: usize,
        travel: f32,
    },
    Done(Captures<B>),
}

impl<B: Control> Wizard<B> {
    /// Ask for `steps` in order (Rad Mobile: accelerator, brake, lights,
    /// wiper, start, exit).
    pub fn new(steps: Vec<Step<B>>, steer_axis: usize) -> Self {
        Self {
            steps,
            step: 0,
            step_started_ms: None,
            ticks: 0,
            baseline: Vec::new(),
            armed: None,
            steer_axis,
            result: Captures::default(),
        }
    }

    /// Ask for one control only (a menu's remapping).
    pub fn single(step: Step<B>, steer_axis: usize) -> Self {
        Self::new(vec![step], steer_axis)
    }

    pub fn current(&self) -> Option<Step<B>> {
        self.steps.get(self.step).copied()
    }

    /// How many steps this run asks for.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Feed the readings of every device; `now_ms` is a monotonic clock.
    pub fn tick(&mut self, now_ms: u64, devices: &[DeviceState]) -> Status<B> {
        let Some(step) = self.current() else {
            return Status::Done(self.result.clone());
        };
        if self.step_started_ms.is_none() {
            self.ticks += 1;
            if self.ticks < WARM_UP_TICKS {
                return Status::WarmingUp;
            }
            self.baseline = devices.to_vec();
            self.step_started_ms = Some(now_ms);
            self.armed = None;
        }
        let started = self.step_started_ms.unwrap_or(now_ms);

        let captured = match step {
            Step::Accelerator | Step::Brake => self.track_axis(step, now_ms, devices),
            Step::Button(b) => self.find_button(devices).map(|(dev, btn)| {
                self.result.buttons.push((b, dev, btn));
            }),
        };
        if captured.is_some() {
            return self.advance(now_ms, devices);
        }
        // an armed pedal gets the full timeout from the moment it armed
        let since = self.armed.map_or(started, |a| a.since_ms.max(started));
        let elapsed = now_ms.saturating_sub(since);
        if elapsed >= step.timeout_ms() {
            self.result.skipped.push(step);
            return self.advance(now_ms, devices);
        }
        if let Some(a) = self.armed {
            let span = (i32::from(a.peak) - i32::from(a.rest)).abs() as f32;
            return Status::Pressing {
                step,
                index: self.step,
                travel: (span / 65_535.0).min(1.0),
            };
        }
        Status::Waiting {
            step,
            index: self.step,
            remaining_ms: step.timeout_ms() - elapsed,
        }
    }

    fn advance(&mut self, now_ms: u64, devices: &[DeviceState]) -> Status<B> {
        self.step += 1;
        self.step_started_ms = None;
        self.armed = None;
        self.ticks = WARM_UP_TICKS; // devices are already warm
        match self.current() {
            Some(_) => self.tick(now_ms, devices),
            None => Status::Done(self.result.clone()),
        }
    }

    fn base_axis(&self, device: usize, axis: usize) -> Option<i16> {
        self.baseline.get(device).and_then(|d| d.axis(axis))
    }

    /// Arm on a big move from the baseline, follow it to its peak, and
    /// capture when it comes back. The brake may share the accelerator's
    /// axis only in the opposite direction (a combined pedal axis).
    fn track_axis(&mut self, step: Step<B>, now_ms: u64, devices: &[DeviceState]) -> Option<()> {
        if let Some(mut a) = self.armed {
            let cur = devices.get(a.device)?.axis(a.axis)?;
            if (i32::from(cur) - i32::from(a.rest)).abs()
                > (i32::from(a.peak) - i32::from(a.rest)).abs()
            {
                a.peak = cur;
            }
            self.armed = Some(a);
            if (i32::from(cur) - i32::from(a.rest)).abs() < RELEASE_DELTA {
                let pedal = (
                    a.device,
                    PedalAxis {
                        axis: a.axis,
                        rest: a.rest,
                        peak: a.peak,
                    },
                );
                match step {
                    Step::Accelerator => self.result.gas = Some(pedal),
                    _ => self.result.brake = Some(pedal),
                }
                return Some(());
            }
            return None;
        }
        for (d, dev) in devices.iter().enumerate() {
            for (axis, &cur) in dev.axes.iter().enumerate() {
                if d == 0 && axis == self.steer_axis {
                    continue;
                }
                let Some(rest) = self.base_axis(d, axis) else {
                    continue;
                };
                let delta = i32::from(cur) - i32::from(rest);
                if delta.abs() <= ARM_DELTA {
                    continue;
                }
                if step == Step::Brake
                    && let Some((gd, g)) = self.result.gas
                    && gd == d
                    && g.axis == axis
                    && (i32::from(g.peak) - i32::from(g.rest)).signum() == delta.signum()
                {
                    continue; // that is the accelerator again
                }
                self.armed = Some(Armed {
                    device: d,
                    axis,
                    rest,
                    peak: cur,
                    since_ms: now_ms,
                });
                return None;
            }
        }
        None
    }

    fn find_button(&self, devices: &[DeviceState]) -> Option<(usize, usize)> {
        for (d, dev) in devices.iter().enumerate() {
            for (b, &down) in dev.buttons.iter().enumerate() {
                let was = self.baseline.get(d).is_some_and(|base| base.button(b));
                if down && !was {
                    return Some((d, b));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_controls::Button;
    use super::*;

    /// Rad Mobile's full setup.
    fn steps() -> Vec<Step<Button>> {
        vec![
            Step::Accelerator,
            Step::Brake,
            Step::Button(Button::Light),
            Step::Button(Button::Wiper),
            Step::Button(Button::Start),
            Step::Button(Button::Exit),
        ]
    }

    struct Rig {
        wiz: Wizard<Button>,
        now: u64,
        devs: Vec<DeviceState>,
    }

    impl Rig {
        fn new(devs: Vec<DeviceState>) -> Self {
            let mut rig = Rig {
                wiz: Wizard::new(steps(), 0),
                now: 0,
                devs,
            };
            for _ in 0..WARM_UP_TICKS {
                rig.tick();
            }
            rig
        }
        fn tick(&mut self) -> Status<Button> {
            self.now += 4;
            self.wiz.tick(self.now, &self.devs)
        }
        fn sweep(&mut self, d: usize, axis: usize, to: i16) -> Status<Button> {
            let from = self.devs[d].axes[axis];
            for v in [to / 2, to, to / 2, from] {
                self.devs[d].axes[axis] = v;
                self.tick();
            }
            self.devs[d].axes[axis] = from;
            self.tick()
        }
        fn press(&mut self, d: usize, b: usize) -> Status<Button> {
            self.devs[d].buttons[b] = true;
            let s = self.tick();
            self.devs[d].buttons[b] = false;
            self.tick();
            s
        }
    }

    fn wheel() -> DeviceState {
        DeviceState {
            axes: vec![0, 32_767, 32_767],
            buttons: vec![false; 12],
        }
    }

    #[test]
    fn a_single_step_asks_for_that_control_only() {
        let mut wiz = Wizard::single(Step::Button(Button::Coin), 0);
        assert_eq!(wiz.step_count(), 1);
        let mut devs = vec![wheel()];
        let mut now = 0;
        for _ in 0..WARM_UP_TICKS {
            now += 4;
            wiz.tick(now, &devs);
        }
        devs[0].buttons[7] = true;
        let Status::Done(result) = wiz.tick(now + 4, &devs) else {
            panic!("not done");
        };
        assert_eq!(result.buttons, vec![(Button::Coin, 0, 7)]);
        assert!(result.gas.is_none() && result.skipped.is_empty());
        assert_eq!(wiz.current(), None);
    }

    #[test]
    fn captures_pedals_and_buttons_on_the_wheel() {
        let mut rig = Rig::new(vec![wheel()]);
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Accelerator,
                ..
            }
        ));

        // turning the wheel is not a pedal
        rig.devs[0].axes[0] = 30_000;
        rig.tick();
        rig.devs[0].axes[0] = 0;
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Accelerator,
                ..
            }
        ));

        rig.devs[0].axes[1] = -30_000;
        assert!(matches!(
            rig.tick(),
            Status::Pressing {
                step: Step::Accelerator,
                ..
            }
        ));
        rig.devs[0].axes[1] = -32_768;
        rig.tick();
        rig.devs[0].axes[1] = 32_767;
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Brake,
                ..
            }
        ));

        rig.sweep(0, 2, -32_768);
        rig.press(0, 4);
        rig.press(0, 5);
        rig.press(0, 9);
        let Status::Done(result) = rig.press(0, 8) else {
            panic!("not done")
        };
        assert_eq!(
            result.gas,
            Some((
                0,
                PedalAxis {
                    axis: 1,
                    rest: 32_767,
                    peak: -32_768
                }
            ))
        );
        assert_eq!(
            result.brake,
            Some((
                0,
                PedalAxis {
                    axis: 2,
                    rest: 32_767,
                    peak: -32_768
                }
            ))
        );
        assert_eq!(
            result.buttons,
            vec![
                (Button::Light, 0, 4),
                (Button::Wiper, 0, 5),
                (Button::Start, 0, 9),
                (Button::Exit, 0, 8)
            ]
        );
        assert!(result.skipped.is_empty());
    }

    #[test]
    fn combined_axis_and_separate_pedal_device() {
        let pedals = DeviceState {
            axes: vec![0],
            buttons: vec![],
        };
        let mut rig = Rig::new(vec![
            DeviceState {
                axes: vec![0],
                buttons: vec![false; 4],
            },
            pedals,
        ]);
        rig.tick();
        rig.sweep(1, 0, 32_767);
        // pressing the accelerator again during the brake step is ignored
        rig.sweep(1, 0, 32_767);
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Brake,
                ..
            }
        ));
        rig.sweep(1, 0, -32_768);
        assert_eq!(
            rig.wiz.result.gas,
            Some((
                1,
                PedalAxis {
                    axis: 0,
                    rest: 0,
                    peak: 32_767
                }
            ))
        );
        assert_eq!(
            rig.wiz.result.brake,
            Some((
                1,
                PedalAxis {
                    axis: 0,
                    rest: 0,
                    peak: -32_768
                }
            ))
        );
    }

    #[test]
    fn steps_time_out_and_held_buttons_are_ignored() {
        let mut held = wheel();
        held.buttons[3] = true;
        let mut rig = Rig::new(vec![held]);
        rig.now += AXIS_TIMEOUT_MS;
        rig.tick();
        rig.now += AXIS_TIMEOUT_MS;
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Button(Button::Light),
                ..
            }
        ));
        // button 3 was already down when the step began
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Button(Button::Light),
                ..
            }
        ));
        for _ in 0..4 {
            rig.now += BUTTON_TIMEOUT_MS;
            rig.tick();
        }
        let Status::Done(r) = rig.tick() else {
            panic!()
        };
        assert_eq!(r.skipped.len(), steps().len());
        assert_eq!(r.gas, None);
    }

    #[test]
    fn an_armed_pedal_that_never_comes_back_times_out() {
        // a sticky pedal: pressed, then it stays far from where it rested
        let mut rig = Rig::new(vec![wheel()]);
        rig.tick();
        rig.devs[0].axes[1] = -30_000;
        assert!(matches!(rig.tick(), Status::Pressing { .. }));
        rig.now += AXIS_TIMEOUT_MS / 2;
        assert!(
            matches!(rig.tick(), Status::Pressing { .. }),
            "still within its time"
        );
        rig.now += AXIS_TIMEOUT_MS;
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Brake,
                ..
            }
        ));
        assert_eq!(rig.wiz.result.skipped, vec![Step::Accelerator]);
        assert_eq!(rig.wiz.result.gas, None);

        // the pedals' device unplugged while its pedal was down
        let mut rig = Rig::new(vec![
            wheel(),
            DeviceState {
                axes: vec![0],
                buttons: vec![],
            },
        ]);
        rig.tick();
        rig.devs[1].axes[0] = 30_000;
        assert!(matches!(rig.tick(), Status::Pressing { .. }));
        rig.devs.pop();
        rig.now += AXIS_TIMEOUT_MS;
        assert!(matches!(
            rig.tick(),
            Status::Waiting {
                step: Step::Brake,
                ..
            }
        ));
    }
}
