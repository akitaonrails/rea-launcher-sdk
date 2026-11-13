//! Force feedback effects: what the game asks of the virtual wheel, and
//! what the real wheel is given.
//!
//! The game's effect may name several axes and a direction (CMR: Spring,
//! Damper, a periodic and a constant force, cartesian on X and Y). A wheel
//! has one actuator, so the effect becomes a one-axis effect on the
//! wheel's X: its force is the share of the direction along X, scaled by
//! the player's strength and flipped on invert ([`Effect::for_wheel`]); a
//! condition is mirrored on invert, never made negative.

use rea_launcher::ffb::FfbSettings;
use rea_launcher::wheel::dinput::Axis;

/// `DI_FFNOMINALMAX`: full force, full coefficient.
pub const NOMINAL_MAX: i32 = 10_000;

/// The standard effects, by `GUID_*` (`13541C20` … `13541C2B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ConstantForce,
    RampForce,
    Square,
    Sine,
    Triangle,
    SawtoothUp,
    SawtoothDown,
    Spring,
    Damper,
    Inertia,
    Friction,
    CustomForce,
}

/// What the effect's type-specific parameters are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Constant,
    Ramp,
    Periodic,
    Condition,
    Custom,
}

impl Kind {
    pub const ALL: [Kind; 12] = [
        Kind::ConstantForce,
        Kind::RampForce,
        Kind::Square,
        Kind::Sine,
        Kind::Triangle,
        Kind::SawtoothUp,
        Kind::SawtoothDown,
        Kind::Spring,
        Kind::Damper,
        Kind::Inertia,
        Kind::Friction,
        Kind::CustomForce,
    ];

    /// The `Data1` of its GUID; the rest is `8E33-11D0-9AD0-00A0C9A06E35`.
    pub fn data1(self) -> u32 {
        0x1354_1C20 + Kind::ALL.iter().position(|k| *k == self).unwrap_or(0) as u32
    }

    pub fn family(self) -> Family {
        match self {
            Kind::ConstantForce => Family::Constant,
            Kind::RampForce => Family::Ramp,
            Kind::Square | Kind::Sine | Kind::Triangle | Kind::SawtoothUp | Kind::SawtoothDown => {
                Family::Periodic
            }
            Kind::Spring | Kind::Damper | Kind::Inertia | Kind::Friction => Family::Condition,
            Kind::CustomForce => Family::Custom,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::ConstantForce => "constant",
            Kind::RampForce => "ramp",
            Kind::Square => "square",
            Kind::Sine => "sine",
            Kind::Triangle => "triangle",
            Kind::SawtoothUp => "sawtooth up",
            Kind::SawtoothDown => "sawtooth down",
            Kind::Spring => "spring",
            Kind::Damper => "damper",
            Kind::Inertia => "inertia",
            Kind::Friction => "friction",
            Kind::CustomForce => "custom",
        }
    }
}

/// `DICONDITION`, one per axis.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Condition {
    pub offset: i32,
    pub positive_coefficient: i32,
    pub negative_coefficient: i32,
    pub positive_saturation: u32,
    pub negative_saturation: u32,
    pub deadband: i32,
}

/// The type-specific parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Params {
    Constant {
        magnitude: i32,
    },
    Ramp {
        start: i32,
        end: i32,
    },
    Periodic {
        magnitude: u32,
        offset: i32,
        /// Hundredths of a degree.
        phase: u32,
        period: u32,
    },
    Condition(Vec<Condition>),
    /// Not passed on (a custom force's samples).
    None,
}

/// `DIENVELOPE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Envelope {
    pub attack_level: u32,
    pub attack_time: u32,
    pub fade_level: u32,
    pub fade_time: u32,
}

/// How `DIEFFECT::rglDirection` is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coordinates {
    Cartesian,
    /// Two axes: hundredths of a degree clockwise from north (−Y).
    Polar,
    /// Angles from +X towards the next axes.
    Spherical,
}

/// An effect as the game set it, with its axes on the virtual wheel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub kind: Kind,
    pub duration: u32,
    pub sample_period: u32,
    pub gain: u32,
    pub start_delay: u32,
    /// The virtual wheel's axes, `None` for one it doesn't have.
    pub axes: Vec<Option<Axis>>,
    pub coordinates: Coordinates,
    pub direction: Vec<i32>,
    pub envelope: Option<Envelope>,
    pub params: Params,
}

impl Effect {
    /// A new effect of `kind` with DirectInput's defaults, as
    /// `CreateEffect` without parameters makes it.
    pub fn new(kind: Kind) -> Effect {
        Effect {
            kind,
            duration: u32::MAX,
            sample_period: 0,
            gain: NOMINAL_MAX as u32,
            start_delay: 0,
            axes: Vec::new(),
            coordinates: Coordinates::Cartesian,
            direction: Vec::new(),
            envelope: None,
            params: Params::None,
        }
    }

    /// The share of the effect's direction along X: −1..1, 0 when it has
    /// no X. One axis: its sign alone (DirectInput ignores the size).
    pub fn x_share(&self) -> f32 {
        let Some(x) = self.axes.iter().position(|a| *a == Some(Axis::X)) else {
            return 0.0;
        };
        if self.axes.len() == 1 {
            return if self.direction.first().is_some_and(|d| *d < 0) {
                -1.0
            } else {
                1.0
            };
        }
        let vector = self.direction_vector();
        let norm = vector.iter().map(|c| c * c).sum::<f32>().sqrt();
        if norm < f32::EPSILON {
            0.0
        } else {
            vector.get(x).copied().unwrap_or(0.0) / norm
        }
    }

    /// The direction as a cartesian vector, one component per axis.
    fn direction_vector(&self) -> Vec<f32> {
        let n = self.axes.len();
        let angle =
            |i: usize| (self.direction.get(i).copied().unwrap_or(0) as f32 / 100.0).to_radians();
        match self.coordinates {
            Coordinates::Cartesian => (0..n)
                .map(|i| self.direction.get(i).copied().unwrap_or(0) as f32)
                .collect(),
            Coordinates::Polar => {
                let a = angle(0);
                let mut v = vec![a.sin(), -a.cos()];
                v.resize(n, 0.0);
                v
            }
            Coordinates::Spherical => (0..n)
                .map(|i| {
                    let lead = if i == 0 {
                        angle(0).cos()
                    } else if i + 1 == n {
                        angle(i - 1).sin()
                    } else {
                        angle(i - 1).sin() * angle(i).cos()
                    };
                    (i + 1..n.saturating_sub(1)).fold(lead, |c, j| c * angle(j).cos())
                })
                .collect(),
        }
    }

    /// The effect for the wheel: one axis, positive direction, its force
    /// the X share × strength, flipped on invert. Conditions keep their
    /// X centre and are scaled in their coefficients and saturations.
    pub fn for_wheel(&self, ffb: &FfbSettings) -> Effect {
        let strength = f32::from(ffb.strength.min(100)) / 100.0;
        let share = self.x_share();
        let sign = if ffb.invert { -1.0 } else { 1.0 };
        let force = |v: i32| scale(v, strength * share * sign);
        let level = |v: u32| scale_u(v, strength * share.abs());
        let params = match &self.params {
            Params::Constant { magnitude } => Params::Constant {
                magnitude: force(*magnitude),
            },
            Params::Ramp { start, end } => Params::Ramp {
                start: force(*start),
                end: force(*end),
            },
            Params::Periodic {
                magnitude,
                offset,
                phase,
                period,
            } => Params::Periodic {
                magnitude: level(*magnitude),
                offset: force(*offset),
                phase: if share * sign < 0.0 {
                    (phase + 18_000) % 36_000
                } else {
                    *phase
                },
                period: *period,
            },
            Params::Condition(conditions) => {
                Params::Condition(vec![self.x_condition(conditions, strength, ffb.invert)])
            }
            Params::None => Params::None,
        };
        Effect {
            axes: vec![Some(Axis::X)],
            coordinates: Coordinates::Cartesian,
            direction: vec![1],
            envelope: self.envelope.map(|e| Envelope {
                attack_level: level(e.attack_level),
                fade_level: level(e.fade_level),
                ..e
            }),
            params,
            ..self.clone()
        }
    }

    /// X's condition: its own when there is one per axis, else the one
    /// condition weighted by the X share of the direction; its
    /// coefficients and saturations scaled by `strength`. `mirror` (the
    /// player's invert) mirrors it: the centre negated, the two sides
    /// swapped. A coefficient never turns negative, which would push the
    /// wheel to full lock.
    fn x_condition(&self, conditions: &[Condition], strength: f32, mirror: bool) -> Condition {
        let x = self.axes.iter().position(|a| *a == Some(Axis::X));
        let (condition, weight) = match x {
            Some(i) if conditions.len() == self.axes.len() && conditions.len() > 1 => {
                (conditions[i], 1.0)
            }
            Some(_) => (
                conditions.first().copied().unwrap_or_default(),
                self.x_share().abs(),
            ),
            None => (Condition::default(), 0.0),
        };
        let f = strength * weight;
        let scaled = Condition {
            positive_coefficient: scale(condition.positive_coefficient, f),
            negative_coefficient: scale(condition.negative_coefficient, f),
            positive_saturation: scale_u(condition.positive_saturation, f),
            negative_saturation: scale_u(condition.negative_saturation, f),
            ..condition
        };
        if !mirror {
            return scaled;
        }
        Condition {
            offset: -scaled.offset,
            positive_coefficient: scaled.negative_coefficient,
            negative_coefficient: scaled.positive_coefficient,
            positive_saturation: scaled.negative_saturation,
            negative_saturation: scaled.positive_saturation,
            deadband: scaled.deadband,
        }
    }

    /// One line for the log and the tests.
    pub fn describe(&self) -> String {
        let params = match &self.params {
            Params::Constant { magnitude } => format!("magnitude={magnitude}"),
            Params::Ramp { start, end } => format!("start={start} end={end}"),
            Params::Periodic {
                magnitude,
                offset,
                phase,
                period,
            } => format!("magnitude={magnitude} offset={offset} phase={phase} period={period}"),
            Params::Condition(c) => c
                .iter()
                .map(|c| {
                    format!(
                        "offset={} coefficient={}/{} saturation={}/{} deadband={}",
                        c.offset,
                        c.positive_coefficient,
                        c.negative_coefficient,
                        c.positive_saturation,
                        c.negative_saturation,
                        c.deadband
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
            Params::None => "-".into(),
        };
        let envelope = self.envelope.map_or(String::new(), |e| {
            format!(
                " attack={}/{} fade={}/{}",
                e.attack_level, e.attack_time, e.fade_level, e.fade_time
            )
        });
        format!(
            "{} {params} duration={} gain={} direction={:?}{envelope}",
            self.kind.name(),
            self.duration as i32,
            self.gain,
            self.direction
        )
    }
}

fn scale(v: i32, f: f32) -> i32 {
    ((v as f32 * f).round() as i32).clamp(-NOMINAL_MAX, NOMINAL_MAX)
}

fn scale_u(v: u32, f: f32) -> u32 {
    ((v as f32 * f).round() as u32).min(NOMINAL_MAX as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(strength: u8, invert: bool) -> FfbSettings {
        FfbSettings {
            strength,
            invert,
            ..FfbSettings::default()
        }
    }

    fn on(axes: &[Axis], direction: &[i32], params: Params, kind: Kind) -> Effect {
        Effect {
            axes: axes.iter().copied().map(Some).collect(),
            direction: direction.to_vec(),
            params,
            ..Effect::new(kind)
        }
    }

    #[test]
    fn guids_follow_directinputs_numbering() {
        assert_eq!(Kind::ConstantForce.data1(), 0x1354_1C20);
        assert_eq!(Kind::Sine.data1(), 0x1354_1C23);
        assert_eq!(Kind::Spring.data1(), 0x1354_1C27);
        assert_eq!(Kind::CustomForce.data1(), 0x1354_1C2B);
        assert_eq!(Kind::Friction.family(), Family::Condition);
        assert_eq!(Kind::SawtoothDown.family(), Family::Periodic);
    }

    #[test]
    fn a_constant_force_is_scaled_and_inverted() {
        let push = on(
            &[Axis::X],
            &[-1],
            Params::Constant { magnitude: 8_000 },
            Kind::ConstantForce,
        );
        let wheel = push.for_wheel(&settings(50, false));
        assert_eq!(wheel.params, Params::Constant { magnitude: -4_000 });
        assert_eq!(
            (wheel.axes.clone(), wheel.direction.clone()),
            (vec![Some(Axis::X)], vec![1])
        );
        let inverted = push.for_wheel(&settings(50, true));
        assert_eq!(inverted.params, Params::Constant { magnitude: 4_000 });
        let off = push.for_wheel(&settings(0, false));
        assert_eq!(off.params, Params::Constant { magnitude: 0 });
    }

    #[test]
    fn two_axis_directions_keep_their_x_share() {
        let c = |axes: &[Axis], coordinates, direction: &[i32]| Effect {
            coordinates,
            ..on(
                axes,
                direction,
                Params::Constant { magnitude: 10_000 },
                Kind::ConstantForce,
            )
        };
        let xy = [Axis::X, Axis::Y];
        let share = |e: Effect| e.x_share();
        assert!((share(c(&xy, Coordinates::Cartesian, &[1, 0])) - 1.0).abs() < 1e-6);
        assert!((share(c(&xy, Coordinates::Cartesian, &[-3, 4])) + 0.6).abs() < 1e-6);
        assert_eq!(share(c(&xy, Coordinates::Cartesian, &[0, 0])), 0.0);
        // Polar: east (+X) is 90°, west 270°, north none.
        assert!((share(c(&xy, Coordinates::Polar, &[9_000, 0])) - 1.0).abs() < 1e-6);
        assert!((share(c(&xy, Coordinates::Polar, &[27_000, 0])) + 1.0).abs() < 1e-6);
        assert!(share(c(&xy, Coordinates::Polar, &[0, 0])).abs() < 1e-6);
        // Spherical: 0° is +X, 180° −X.
        assert!((share(c(&xy, Coordinates::Spherical, &[0])) - 1.0).abs() < 1e-6);
        assert!((share(c(&xy, Coordinates::Spherical, &[18_000])) + 1.0).abs() < 1e-6);
        // Y alone, or X in second place.
        assert_eq!(share(c(&[Axis::Y], Coordinates::Cartesian, &[1])), 0.0);
        assert!(
            (share(c(&[Axis::Y, Axis::X], Coordinates::Cartesian, &[0, -2])) + 1.0).abs() < 1e-6
        );
        let half = c(&xy, Coordinates::Cartesian, &[-3, 4]).for_wheel(&settings(100, false));
        assert_eq!(half.params, Params::Constant { magnitude: -6_000 });
    }

    #[test]
    fn periodic_effects_flip_their_phase_not_their_magnitude() {
        let sine = on(
            &[Axis::X, Axis::Y],
            &[-1, 0],
            Params::Periodic {
                magnitude: 6_000,
                offset: 1_000,
                phase: 9_000,
                period: 100_000,
            },
            Kind::Sine,
        );
        let wheel = sine.for_wheel(&settings(50, false));
        assert_eq!(
            wheel.params,
            Params::Periodic {
                magnitude: 3_000,
                offset: -500,
                phase: 27_000,
                period: 100_000
            }
        );
        let back = sine.for_wheel(&settings(50, true));
        assert_eq!(
            back.params,
            Params::Periodic {
                magnitude: 3_000,
                offset: 500,
                phase: 9_000,
                period: 100_000
            }
        );
    }

    #[test]
    fn conditions_take_xs_and_scale_coefficients() {
        let x = Condition {
            offset: 500,
            positive_coefficient: 10_000,
            negative_coefficient: 8_000,
            positive_saturation: 10_000,
            negative_saturation: 6_000,
            deadband: 100,
        };
        let y = Condition {
            positive_coefficient: 1,
            ..x
        };
        let spring = on(
            &[Axis::Y, Axis::X],
            &[0, 0],
            Params::Condition(vec![y, x]),
            Kind::Spring,
        );
        let wheel = spring.for_wheel(&settings(40, false));
        let expected = Condition {
            positive_coefficient: 4_000,
            negative_coefficient: 3_200,
            positive_saturation: 4_000,
            negative_saturation: 2_400,
            ..x
        };
        assert_eq!(wheel.params, Params::Condition(vec![expected]));
        // Inverted: the same spring seen in a mirror, never a negative
        // coefficient (that pushes a direct-drive base to full lock).
        let inverted = spring.for_wheel(&settings(40, true));
        assert_eq!(
            inverted.params,
            Params::Condition(vec![Condition {
                offset: -500,
                positive_coefficient: 3_200,
                negative_coefficient: 4_000,
                positive_saturation: 2_400,
                negative_saturation: 4_000,
                deadband: 100,
            }])
        );
        // One condition along a direction: weighted by its X share.
        let along = on(
            &[Axis::X, Axis::Y],
            &[0, 1],
            Params::Condition(vec![x]),
            Kind::Damper,
        );
        assert_eq!(
            along.for_wheel(&settings(100, false)).params,
            Params::Condition(vec![Condition {
                positive_coefficient: 0,
                negative_coefficient: 0,
                positive_saturation: 0,
                negative_saturation: 0,
                ..x
            }])
        );
        let single = on(&[Axis::X], &[1], Params::Condition(vec![x]), Kind::Damper);
        assert_eq!(
            single.for_wheel(&settings(100, false)).params,
            Params::Condition(vec![x])
        );
    }

    #[test]
    fn envelopes_and_ramps_scale_too() {
        let ramp = Effect {
            envelope: Some(Envelope {
                attack_level: 10_000,
                attack_time: 5,
                fade_level: 2_000,
                fade_time: 7,
            }),
            ..on(
                &[Axis::X],
                &[1],
                Params::Ramp {
                    start: -10_000,
                    end: 4_000,
                },
                Kind::RampForce,
            )
        };
        let wheel = ramp.for_wheel(&settings(50, true));
        assert_eq!(
            wheel.params,
            Params::Ramp {
                start: 5_000,
                end: -2_000
            }
        );
        assert_eq!(
            wheel.envelope,
            Some(Envelope {
                attack_level: 5_000,
                attack_time: 5,
                fade_level: 1_000,
                fade_time: 7
            })
        );
        assert_eq!(
            wheel.describe(),
            "ramp start=5000 end=-2000 duration=-1 gain=10000 direction=[1] attack=5000/5 fade=1000/7"
        );
    }
}
