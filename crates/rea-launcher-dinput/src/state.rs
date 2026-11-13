//! The virtual wheel's state: finished controls in, the game's axis
//! units out.
//!
//! Every axis is a fraction −1..1 of its range ([`Reading`]) until the
//! game's `DIPROP_RANGE`, `DIPROP_DEADZONE` and `DIPROP_SATURATION`
//! ([`AxisProps`]) turn it into the number it reads.

use rea_launcher::wheel::Controls;
use rea_launcher::wheel::dinput::Axis;

use crate::format::{POV_CENTRED, Values};

/// DirectInput's full scale for deadzone and saturation.
pub const FULL: u32 = 10_000;

/// One axis's properties, as the game set them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisProps {
    pub min: i32,
    pub max: i32,
    /// Around the centre, 0..=[`FULL`].
    pub deadzone: u32,
    /// From the centre, 0..=[`FULL`]: beyond it the axis reads its end.
    pub saturation: u32,
}

impl Default for AxisProps {
    /// A joystick's defaults: 0..65535, no deadzone, full saturation.
    fn default() -> Self {
        Self {
            min: 0,
            max: 65_535,
            deadzone: 0,
            saturation: FULL,
        }
    }
}

impl AxisProps {
    /// `v` (−1 at the minimum, 0 centred, +1 at the maximum) in this
    /// range, after the deadzone and saturation. A value halfway between
    /// two integers takes the lower one, so a centred axis rests at 127
    /// of 0..255, as on a real device (CMR04 reads 128 as one step right),
    /// and at −1 of −32768..32767.
    pub fn value(&self, v: f32) -> i32 {
        let dz = self.deadzone.min(FULL) as f32 / FULL as f32;
        let sat = (self.saturation.min(FULL) as f32 / FULL as f32).max(dz);
        let m = v.clamp(-1.0, 1.0).abs();
        let m = if m <= dz {
            0.0
        } else if m >= sat {
            1.0
        } else {
            (m - dz) / (sat - dz)
        };
        let (min, max) = (f64::from(self.min), f64::from(self.max));
        let centre = (min + max) / 2.0;
        let at = centre + f64::from(m.copysign(v)) * (max - min) / 2.0;
        (at - 0.5).ceil().clamp(min, max) as i32
    }
}

/// The virtual wheel's controls before the game's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    /// Each axis in [`Axis::ALL`] order, −1 at its minimum … +1 at its
    /// maximum.
    pub axes: [f32; 4],
    /// Bit n: button n is down.
    pub buttons: u32,
    pub pov: u32,
}

impl Reading {
    /// Centred steering, pedals up, nothing pressed.
    pub const NEUTRAL: Reading = Reading {
        axes: [0.0, 0.0, -1.0, -1.0],
        buttons: 0,
        pov: POV_CENTRED,
    };

    /// The axes of `controls` (steering after lock and curve, pedals
    /// 0..1): X steering, Y throttle minus brake, Z throttle and Rz brake
    /// each released at their minimum.
    pub fn new(controls: &Controls, buttons: u32, pov: u32) -> Reading {
        let pedal = |p: f32| p.clamp(0.0, 1.0) * 2.0 - 1.0;
        Reading {
            axes: [
                controls.steer.clamp(-1.0, 1.0),
                (controls.accel - controls.brake).clamp(-1.0, 1.0),
                pedal(controls.accel),
                pedal(controls.brake),
            ],
            buttons,
            pov,
        }
    }

    pub fn axis(&self, axis: Axis) -> f32 {
        self.axes[Axis::ALL.iter().position(|a| *a == axis).unwrap_or(0)]
    }

    /// In the game's units.
    pub fn values(&self, props: &[AxisProps; 4]) -> Values {
        Values {
            axes: std::array::from_fn(|i| props[i].value(self.axes[i])),
            buttons: self.buttons,
            pov: self.pov,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(min: i32, max: i32) -> AxisProps {
        AxisProps {
            min,
            max,
            ..AxisProps::default()
        }
    }

    #[test]
    fn the_cmr_range_has_256_steps() {
        let cmr = range(0, 255);
        assert_eq!(
            [-1.0, -0.5, 0.0, 0.5, 1.0].map(|v| cmr.value(v)),
            [0, 64, 127, 191, 255]
        );
        assert_eq!(cmr.value(-7.0), 0, "clamped");
    }

    #[test]
    fn signed_and_default_ranges() {
        let signed = range(-32_768, 32_767);
        assert_eq!(
            [-1.0, 0.0, 1.0].map(|v| signed.value(v)),
            [-32_768, -1, 32_767]
        );
        let small = range(-100, 100);
        assert_eq!([-1.0, 0.25, 1.0].map(|v| small.value(v)), [-100, 25, 100]);
        let default = AxisProps::default();
        assert_eq!([-1.0, 1.0].map(|v| default.value(v)), [0, 65_535]);
    }

    #[test]
    fn deadzone_and_saturation_work_from_the_centre() {
        let props = AxisProps {
            deadzone: 1_000,
            saturation: 9_000,
            ..range(-1000, 1000)
        };
        assert_eq!(props.value(0.09), 0);
        assert_eq!(props.value(-0.09), 0);
        assert_eq!(props.value(0.5), 500);
        assert_eq!(props.value(-0.95), -1000);
        // A saturation inside the deadzone is a switch, never a division
        // by zero.
        let switch = AxisProps {
            deadzone: 5_000,
            saturation: 1_000,
            ..range(0, 10)
        };
        assert_eq!([0.4, 0.6].map(|v| switch.value(v)), [5, 10]);
    }

    #[test]
    fn pedals_rest_at_the_minimum_and_combine_on_y() {
        let both = Controls {
            steer: -0.25,
            accel: 1.0,
            brake: 0.25,
            buttons: 0,
        };
        let reading = Reading::new(&both, 0b10, 4500);
        assert_eq!(reading.axes, [-0.25, 0.75, 1.0, -0.5]);
        let cmr = [range(0, 255); 4];
        let values = reading.values(&cmr);
        assert_eq!(values.axes, [96, 223, 255, 64]);
        assert_eq!((values.buttons, values.pov), (0b10, 4500));
        let rest = Reading::NEUTRAL.values(&cmr);
        assert_eq!(rest.axes, [127, 127, 0, 0]);
        assert_eq!(
            Reading::new(&Controls::default(), 0, POV_CENTRED),
            Reading::NEUTRAL
        );
        assert_eq!(Reading::NEUTRAL.axis(Axis::Rz), -1.0);
    }
}
