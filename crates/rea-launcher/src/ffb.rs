//! Synthetic force feedback for games that had no steering motor, made
//! from the car's state the way Wanszai's Ridge Racer and Scud Race
//! wrappers do (docs/playbooks/wheels-and-ffb.md). From rea-rad-mobile
//! `ffb.rs`: the engine, settings and output; the game's telemetry stays in
//! the launcher, which maps it to a normalised [`CarState`] each frame.
//!
//! - **Centring:** a spring toward the centre that stiffens with speed
//!   (the cabinet's own spring when stopped), slew-limited like VR's 120 ms.
//! - **Damping and friction:** Scud Race's speed-dependent damper and a
//!   little friction, so the wheel has weight.
//! - **Wall scrape:** pushed away from the wall with a buzz, scaled by
//!   speed.
//! - **Collisions:** a sudden loss of speed gives a jolt held for four
//!   frames, armed only after the first second of a race (Ridge Racer
//!   Collection).
//!
//! Outside a race only the gentle centring spring remains. The numbers are
//! [`Tuning::ARCADE`]'s; a launcher may start from them and change some.
//!
//! ```
//! use rea_launcher::ffb::{CarState, FfbSettings, SyntheticFfb, Tuning};
//! let mut ffb = SyntheticFfb::new(Tuning::ARCADE);
//! let car = CarState { racing: true, speed: 1.0, contact: None };
//! let mut out = Default::default();
//! for _ in 0..250 {
//!     out = ffb.update(1.0 / 250.0, 0.5, Some(car), &FfbSettings::default());
//! }
//! assert!(out.constant < 0.0, "steered right, pulled back left");
//! ```

use std::f32::consts::TAU;

/// The player's force feedback options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FfbSettings {
    /// 0 (off) ..= 100.
    pub strength: u8,
    /// The last resort for a wheel whose driver reports its axis backwards.
    pub invert: bool,
    /// Gamepad rumble, 0 ..= 100.
    pub rumble: u8,
}

impl Default for FfbSettings {
    fn default() -> Self {
        Self {
            strength: 50,
            invert: false,
            rumble: 100,
        }
    }
}

/// Forces for the device, each already scaled by the settings.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FfbOutput {
    /// −1 pushes the wheel left, +1 right.
    pub constant: f32,
    /// 0..1 damper coefficient.
    pub damper: f32,
    /// 0..1 friction coefficient.
    pub friction: f32,
    /// 0..1 for gamepads.
    pub rumble: f32,
}

/// A side of the car.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// −1 for left, 1 for right, as the wheel counts.
    pub fn sign(self) -> f32 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
        }
    }
}

/// What the launcher learnt from the game this frame, normalised.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CarState {
    /// A player's race is on (not the attract demo or a test menu).
    pub racing: bool,
    /// 0..1 of the game's top speed (more is clamped).
    pub speed: f32,
    /// The wall the car is scraping, if any.
    pub contact: Option<Side>,
}

/// The engine's numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// Centring spring when stopped and at top speed, in a race.
    pub spring_stopped: f32,
    pub spring_top: f32,
    /// Centring spring outside a race.
    pub spring_idle: f32,
    pub damper_stopped: f32,
    pub damper_top: f32,
    pub friction: f32,
    /// Full-scale changes of the constant force per second.
    pub slew_per_sec: f32,
    /// Smoothed speed loss, in top speeds per second, that counts as a hit
    /// (hard braking stays far below it).
    pub hit_decel: f32,
    /// How long a jolt is held.
    pub jolt_hold_sec: f32,
    /// No jolts for this long after a race starts (the speed jumps from
    /// the demo's to zero).
    pub jolt_arm_sec: f32,
    /// Push away from a wall, and the buzz on top, at top speed.
    pub scrape_force: f32,
    pub scrape_buzz: f32,
    pub scrape_hz: f32,
    /// Speed is smoothed over this time before the hit test.
    pub speed_window_sec: f32,
}

impl Tuning {
    /// rea-rad-mobile's numbers: Ridge Racer Collection's jolts, Scud
    /// Race's damper, Virtua Racing's slew. `hit_decel` was 600 game units
    /// a second against Rad Mobile's top speed of 430.
    pub const ARCADE: Tuning = Tuning {
        spring_stopped: 0.25,
        spring_top: 1.0,
        spring_idle: 0.2,
        damper_stopped: 0.05,
        damper_top: 0.30,
        friction: 0.04,
        slew_per_sec: 1.0 / 0.120,
        hit_decel: 600.0 / 430.0,
        jolt_hold_sec: 4.0 / 60.0,
        jolt_arm_sec: 1.0,
        scrape_force: 0.3,
        scrape_buzz: 0.2,
        scrape_hz: 14.0,
        speed_window_sec: 0.05,
    };
}

/// Below this speed (of top speed) a wall contact doesn't scrape.
const SCRAPE_MIN_SPEED: f32 = 0.05;
/// Pad rumble per unit of scrape.
const SCRAPE_RUMBLE: f32 = 0.6;

/// The synthetic force feedback engine; one per race session.
#[derive(Debug, Clone)]
pub struct SyntheticFfb {
    tuning: Tuning,
    constant: f32,
    smoothed_speed: Option<f32>,
    jolt: f32,
    jolt_left: f32,
    phase: f32,
    racing_for: f32,
}

impl SyntheticFfb {
    pub fn new(tuning: Tuning) -> Self {
        Self {
            tuning,
            constant: 0.0,
            smoothed_speed: None,
            jolt: 0.0,
            jolt_left: 0.0,
            phase: 0.0,
            racing_for: 0.0,
        }
    }

    /// Advance by `dt` seconds. `steer` is the wheel's position within the
    /// steering lock (−1..1); `car` is `None` when the game is not
    /// reporting (not running, or stale).
    pub fn update(
        &mut self,
        dt: f32,
        steer: f32,
        car: Option<CarState>,
        s: &FfbSettings,
    ) -> FfbOutput {
        let t = self.tuning;
        let dt = dt.clamp(0.0, 0.1);
        let steer = steer.clamp(-1.0, 1.0);
        let racing = car.filter(|c| c.racing);

        let (target, damper, scrape) = match racing {
            Some(c) => {
                let speed = c.speed.clamp(0.0, 1.0);
                let spring = t.spring_stopped + (t.spring_top - t.spring_stopped) * speed;
                let mut target = -steer * spring;
                let side = c.contact.map_or(0.0, Side::sign);
                let scrape = if side != 0.0 && speed > SCRAPE_MIN_SPEED {
                    speed
                } else {
                    0.0
                };
                if scrape > 0.0 {
                    self.phase = (self.phase + dt * t.scrape_hz).fract();
                    let buzz = (self.phase * TAU).sin() * t.scrape_buzz * scrape;
                    target += -side * t.scrape_force * scrape + buzz;
                }
                (
                    target,
                    t.damper_stopped + (t.damper_top - t.damper_stopped) * speed,
                    scrape,
                )
            }
            None => (-steer * t.spring_idle, t.damper_stopped, 0.0),
        };

        self.detect_hit(dt, steer, racing);

        let max_step = t.slew_per_sec * dt;
        self.constant += (target - self.constant).clamp(-max_step, max_step);

        let mut constant = self.constant;
        let mut rumble = scrape * SCRAPE_RUMBLE;
        if self.jolt_left > 0.0 {
            self.jolt_left -= dt;
            constant = self.jolt;
            rumble = rumble.max(self.jolt.abs());
        }

        let gain = f32::from(s.strength.min(100)) / 100.0;
        let sign = if s.invert { -1.0 } else { 1.0 };
        FfbOutput {
            constant: (constant * gain * sign).clamp(-1.0, 1.0),
            damper: (damper * gain).clamp(0.0, 1.0),
            friction: (t.friction * gain).clamp(0.0, 1.0),
            rumble: (rumble * f32::from(s.rumble.min(100)) / 100.0).clamp(0.0, 1.0),
        }
    }

    fn detect_hit(&mut self, dt: f32, steer: f32, racing: Option<CarState>) {
        let t = self.tuning;
        let Some(car) = racing else {
            self.smoothed_speed = None;
            self.racing_for = 0.0;
            return;
        };
        self.racing_for += dt;
        let prev = self.smoothed_speed.unwrap_or(car.speed);
        let alpha = if dt > 0.0 {
            (dt / t.speed_window_sec).min(1.0)
        } else {
            0.0
        };
        let now = prev + (car.speed - prev) * alpha;
        self.smoothed_speed = Some(now);
        if dt <= 0.0 || self.jolt_left > 0.0 || self.racing_for < t.jolt_arm_sec {
            return;
        }
        let decel = (prev - now) / dt;
        if decel > t.hit_decel {
            let size = (decel / (t.hit_decel * 4.0)).min(1.0);
            let magnitude = (size * 0.5 + 0.5) * 0.8;
            // throw the wheel against the way it is turned
            let dir = if steer.abs() < 0.05 {
                1.0
            } else {
                -steer.signum()
            };
            self.jolt = dir * magnitude;
            self.jolt_left = t.jolt_hold_sec;
        }
    }
}

/// How long [`test_pattern`] runs.
pub const TEST_PATTERN_MS: u64 = 1_500;

/// A menu's "test force feedback": half a second pushing left, half a
/// second right, half a second at rest, with rumble, at the player's
/// strength (25 at least, so it is felt). `None` once it is over. The
/// launcher replaces the engine's constant force and rumble with it.
pub fn test_pattern(elapsed_ms: u64, s: &FfbSettings) -> Option<FfbOutput> {
    if elapsed_ms >= TEST_PATTERN_MS {
        return None;
    }
    let push = f32::from(s.strength.clamp(25, 100)) / 100.0 * 0.6;
    let constant = match elapsed_ms {
        0..500 => -push,
        500..1000 => push,
        _ => 0.0,
    };
    Some(FfbOutput {
        constant,
        rumble: 0.6,
        ..FfbOutput::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 250.0;
    /// rea-rad-mobile's top speed, in its game units.
    const RAD_TOP: f32 = 430.0;

    fn run(
        e: &mut SyntheticFfb,
        secs: f32,
        steer: f32,
        car: Option<CarState>,
        s: &FfbSettings,
    ) -> FfbOutput {
        let mut out = FfbOutput::default();
        for _ in 0..(secs / DT) as usize {
            out = e.update(DT, steer, car, s);
        }
        out
    }

    fn engine() -> SyntheticFfb {
        SyntheticFfb::new(Tuning::ARCADE)
    }

    /// A race at `speed` in rea-rad-mobile's units.
    fn racing(speed: f32) -> Option<CarState> {
        Some(CarState {
            racing: true,
            speed: speed / RAD_TOP,
            contact: None,
        })
    }

    fn full() -> FfbSettings {
        FfbSettings {
            strength: 100,
            ..Default::default()
        }
    }

    #[test]
    fn the_hit_threshold_is_rad_mobiles_in_top_speeds() {
        // 600 units/s at a top speed of 430
        assert!((Tuning::ARCADE.hit_decel * RAD_TOP - 600.0).abs() < 1e-3);
        assert!((Tuning::ARCADE.hit_decel - 1.395).abs() < 1e-3);
    }

    #[test]
    fn centring_stiffens_with_speed() {
        let slow = run(&mut engine(), 0.5, 0.5, racing(0.0), &full());
        let fast = run(&mut engine(), 0.5, 0.5, racing(RAD_TOP), &full());
        assert!(
            slow.constant < 0.0 && fast.constant < 0.0,
            "pulls back to the left"
        );
        assert!((slow.constant + 0.125).abs() < 1e-3);
        assert!((fast.constant + 0.5).abs() < 1e-3);
        assert!(fast.damper > slow.damper);
        assert!((fast.friction - Tuning::ARCADE.friction).abs() < 1e-6);
    }

    #[test]
    fn the_force_is_slew_limited() {
        let first = engine().update(0.01, 1.0, racing(RAD_TOP), &full());
        assert!((first.constant + 0.01 * Tuning::ARCADE.slew_per_sec).abs() < 1e-4);
    }

    #[test]
    fn strength_and_invert_scale_everything() {
        let with = |s: FfbSettings| run(&mut engine(), 0.5, 0.5, racing(200.0), &s);
        let full_out = with(full());
        let half = with(FfbSettings {
            strength: 50,
            ..Default::default()
        });
        let inv = with(FfbSettings {
            strength: 100,
            invert: true,
            rumble: 100,
        });
        let off = with(FfbSettings {
            strength: 0,
            ..Default::default()
        });
        assert!((half.constant - full_out.constant / 2.0).abs() < 1e-4);
        assert!((half.damper - full_out.damper / 2.0).abs() < 1e-4);
        assert!((inv.constant + full_out.constant).abs() < 1e-4);
        assert_eq!(off.constant, 0.0);
        assert_eq!(off.damper, 0.0);
    }

    #[test]
    fn outside_a_race_only_the_idle_spring_remains() {
        let demo = Some(CarState {
            racing: false,
            speed: 1.0,
            contact: Some(Side::Right),
        });
        let out = run(&mut engine(), 0.5, 1.0, demo, &full());
        assert!((out.constant + Tuning::ARCADE.spring_idle).abs() < 1e-3);
        assert_eq!(out.rumble, 0.0);
        let none = run(&mut engine(), 0.5, 1.0, None, &full());
        assert_eq!(none, out);
    }

    #[test]
    fn a_crash_jolts_but_braking_does_not() {
        let s = full();
        let mut e = engine();
        run(&mut e, 0.5, 0.3, racing(400.0), &s);
        // hard braking: about 80 units a second
        let mut speed = 400.0;
        for _ in 0..250 {
            speed -= 80.0 * DT;
            let out = e.update(DT, 0.3, racing(speed), &s);
            assert!(out.rumble == 0.0, "braking must not jolt");
        }
        // a wall: 250 units lost in a tenth of a second
        let mut jolted = false;
        for _ in 0..25 {
            speed = (speed - 10.0).max(50.0);
            let out = e.update(DT, 0.3, racing(speed), &s);
            jolted |= out.rumble > 0.5 && out.constant < -0.4;
        }
        assert!(jolted, "the hit throws the wheel against the turn");
    }

    #[test]
    fn a_new_race_does_not_jolt() {
        let s = full();
        let mut e = engine();
        // the attract demo drives fast, then a player's race starts from zero
        let demo = Some(CarState {
            racing: false,
            speed: 400.0 / RAD_TOP,
            contact: None,
        });
        run(&mut e, 1.0, 0.0, demo, &s);
        let out = run(&mut e, 0.5, 0.0, racing(0.0), &s);
        assert_eq!(out.rumble, 0.0);
        assert!(out.constant.abs() < 0.01);
    }

    #[test]
    fn scraping_the_wall_pushes_away_and_buzzes() {
        let s = full();
        let right_wall = Some(CarState {
            racing: true,
            speed: 300.0 / RAD_TOP,
            contact: Some(Side::Right),
        });
        let out = run(&mut engine(), 0.3, 0.0, right_wall, &s);
        assert!(out.rumble > 0.3);
        let mut e = engine();
        let mut sum = 0.0;
        for _ in 0..250 {
            sum += e.update(DT, 0.0, right_wall, &s).constant;
        }
        assert!(
            sum / 250.0 < -0.1,
            "on average pushed left, away from the wall"
        );
        assert_eq!(Side::Left.sign(), -1.0);
    }

    #[test]
    fn the_test_pattern_pushes_left_then_right_then_rests() {
        let s = FfbSettings {
            strength: 10,
            ..Default::default()
        };
        let at = |ms| test_pattern(ms, &s).map(|o| o.constant);
        let push = 0.25 * 0.6; // at least 25 %
        assert!((at(0).unwrap() + push).abs() < 1e-6);
        assert!((at(499).unwrap() + push).abs() < 1e-6);
        assert!((at(500).unwrap() - push).abs() < 1e-6);
        assert_eq!(at(1200), Some(0.0));
        assert_eq!(at(TEST_PATTERN_MS), None);
        assert_eq!(test_pattern(0, &s).unwrap().rumble, 0.6);
        let strong = test_pattern(600, &FfbSettings { strength: 100, ..s }).unwrap();
        assert!((strong.constant - 0.6).abs() < 1e-6);
    }
}
