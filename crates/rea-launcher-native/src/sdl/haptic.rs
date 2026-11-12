//! A wheel's force feedback through SDL's haptic API, set up as Wanszai's
//! wrappers do: autocentre off, gain 100 (the launcher scales strength
//! itself), a constant force plus damper and friction conditions (no
//! spring), parameters sent only when they change, the constant force
//! re-run every [`HapticPolicy::keepalive`] and stopped conditions revived
//! every [`HapticPolicy::revive_check`]. From rea-rad-mobile
//! `input/haptic.rs`.

use std::ptr::NonNull;
use std::time::Instant;

use rea_launcher::ffb::FfbOutput;
use sdl2::sys;

use super::{Device, error};
use crate::policy::{EffectKind, EffectParams, HapticPolicy, changed, effect_params};

/// Force feedback open on one device. Owned by [`super::Devices`], which
/// closes it before the joystick it came from.
pub struct Haptic {
    dev: NonNull<sys::SDL_Haptic>,
    instance: i32,
    policy: HapticPolicy,
    constant: i32,
    damper: Option<i32>,
    friction: Option<i32>,
    sent: FfbOutput,
    last_keepalive: Instant,
    last_check: Instant,
    description: String,
}

impl Haptic {
    /// Opens force feedback on `device`; refused when it has none, or no
    /// constant force (damper and friction alone can't centre a wheel).
    pub(super) fn open(device: &Device, policy: HapticPolicy) -> Result<Haptic, String> {
        if !device.haptic {
            return Err("no force feedback".into());
        }
        // SAFETY: the joystick is open (device lives); the haptic handle
        // is checked for null and closed on every failure path.
        unsafe {
            let dev = NonNull::new(sys::SDL_HapticOpenFromJoystick(device.raw()))
                .ok_or_else(|| format!("SDL_HapticOpenFromJoystick: {}", error()))?;
            let h = dev.as_ptr();
            let caps = sys::SDL_HapticQuery(h);
            if caps & sys::SDL_HAPTIC_AUTOCENTER != 0 {
                sys::SDL_HapticSetAutocenter(h, 0);
            }
            if caps & sys::SDL_HAPTIC_GAIN != 0 {
                sys::SDL_HapticSetGain(h, 100);
            }
            let make = |kind: EffectKind, flag: u32| -> Option<i32> {
                if caps & flag == 0 {
                    return None;
                }
                let mut effect = sdl_effect(&effect_params(kind, 0.0));
                let id = sys::SDL_HapticNewEffect(h, &mut effect);
                (id >= 0).then(|| {
                    sys::SDL_HapticRunEffect(h, id, 1);
                    id
                })
            };
            let Some(constant) = make(EffectKind::Constant, sys::SDL_HAPTIC_CONSTANT) else {
                let why = format!("no constant force (capabilities {caps:#06x}): {}", error());
                sys::SDL_HapticClose(h);
                return Err(why);
            };
            let damper = make(EffectKind::Damper, sys::SDL_HAPTIC_DAMPER);
            let friction = make(EffectKind::Friction, sys::SDL_HAPTIC_FRICTION);
            let description = format!(
                "constant{}{} (capabilities {caps:#06x})",
                if damper.is_some() { " + damper" } else { "" },
                if friction.is_some() {
                    " + friction"
                } else {
                    ""
                },
            );
            let now = Instant::now();
            Ok(Haptic {
                dev,
                instance: device.instance,
                policy,
                constant,
                damper,
                friction,
                sent: FfbOutput::default(),
                last_keepalive: now,
                last_check: now,
                description,
            })
        }
    }

    /// The SDL instance id of the device it drives.
    pub fn instance(&self) -> i32 {
        self.instance
    }

    /// "constant + damper + friction (capabilities 0x…)".
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Sends the new forces (only what changed) and keeps the effects
    /// alive.
    pub fn apply(&mut self, out: &FfbOutput) {
        let h = self.dev.as_ptr();
        let now = Instant::now();
        let eps = self.policy.epsilon;
        let updates = [
            (
                Some(self.constant),
                EffectKind::Constant,
                out.constant,
                self.sent.constant,
            ),
            (
                self.damper,
                EffectKind::Damper,
                out.damper,
                self.sent.damper,
            ),
            (
                self.friction,
                EffectKind::Friction,
                out.friction,
                self.sent.friction,
            ),
        ];
        for (id, kind, new, old) in updates {
            if let Some(id) = id
                && changed(new, old, eps)
            {
                let mut effect = sdl_effect(&effect_params(kind, new));
                // SAFETY: the device and effect ids are ours and open.
                unsafe { sys::SDL_HapticUpdateEffect(h, id, &mut effect) };
            }
        }
        self.sent = *out;
        if now.saturating_duration_since(self.last_keepalive) >= self.policy.keepalive {
            self.last_keepalive = now;
            // SAFETY: as above.
            unsafe { sys::SDL_HapticRunEffect(h, self.constant, 1) };
        }
        if now.saturating_duration_since(self.last_check) >= self.policy.revive_check {
            self.last_check = now;
            for id in [self.damper, self.friction].into_iter().flatten() {
                // SAFETY: as above. 0 = stopped (or the query is not
                // supported, where running again is harmless).
                unsafe {
                    if sys::SDL_HapticGetEffectStatus(h, id) == 0 {
                        sys::SDL_HapticRunEffect(h, id, 1);
                    }
                }
            }
        }
    }

    /// Releases the wheel: no force at all.
    pub fn stop(&mut self) {
        self.apply(&FfbOutput::default());
        // SAFETY: the device is open.
        unsafe { sys::SDL_HapticStopAll(self.dev.as_ptr()) };
    }
}

impl Drop for Haptic {
    fn drop(&mut self) {
        // SAFETY: we opened it; closing destroys the effects. Linux frees
        // them with the fd anyway; stopping first is for Windows drivers.
        unsafe {
            sys::SDL_HapticStopAll(self.dev.as_ptr());
            sys::SDL_HapticClose(self.dev.as_ptr());
        }
    }
}

/// SDL's effect for [`effect_params`]: infinite, cartesian, starting now.
fn sdl_effect(p: &EffectParams) -> sys::SDL_HapticEffect {
    // SAFETY: SDL_HapticEffect is a C union of plain data; zero is a valid
    // start, and the variant written below is the one its type names.
    let mut e: sys::SDL_HapticEffect = unsafe { std::mem::zeroed() };
    let direction = sys::SDL_HapticDirection {
        type_: sys::SDL_HAPTIC_CARTESIAN as u8,
        dir: p.direction,
    };
    match p.kind {
        EffectKind::Constant => {
            e.constant = sys::SDL_HapticConstant {
                type_: sys::SDL_HAPTIC_CONSTANT as u16,
                direction,
                length: sys::SDL_HAPTIC_INFINITY,
                delay: 0,
                button: 0,
                interval: 0,
                level: p.level,
                attack_length: 0,
                attack_level: 0,
                fade_length: 0,
                fade_level: 0,
            }
        }
        EffectKind::Damper | EffectKind::Friction => {
            let flag = if p.kind == EffectKind::Damper {
                sys::SDL_HAPTIC_DAMPER
            } else {
                sys::SDL_HAPTIC_FRICTION
            };
            e.condition = sys::SDL_HapticCondition {
                type_: flag as u16,
                direction,
                length: sys::SDL_HAPTIC_INFINITY,
                delay: 0,
                button: 0,
                interval: 0,
                right_sat: [0xffff; 3],
                left_sat: [0xffff; 3],
                right_coeff: [p.coeff; 3],
                left_coeff: [p.coeff; 3],
                deadband: [0; 3],
                center: [0; 3],
            }
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sdl_effect_says_the_force_comes_from_the_west() {
        let e = sdl_effect(&effect_params(EffectKind::Constant, 0.5));
        // SAFETY: sdl_effect filled the constant variant.
        let c = unsafe { e.constant };
        assert_eq!(u32::from(c.type_), sys::SDL_HAPTIC_CONSTANT);
        assert_eq!(c.direction.type_, sys::SDL_HAPTIC_CARTESIAN as u8);
        assert!(c.direction.dir[0] < 0, "from the west");
        assert!(c.level > 0, "so a positive level pushes right");
        assert_eq!(c.length, sys::SDL_HAPTIC_INFINITY);

        let e = sdl_effect(&effect_params(EffectKind::Friction, 1.0));
        // SAFETY: sdl_effect filled the condition variant.
        let f = unsafe { e.condition };
        assert_eq!(u32::from(f.type_), sys::SDL_HAPTIC_FRICTION);
        assert_eq!(f.left_coeff, [32767; 3]);
        // SAFETY: as above.
        let d = unsafe { sdl_effect(&effect_params(EffectKind::Damper, 0.0)).condition };
        assert_eq!(u32::from(d.type_), sys::SDL_HAPTIC_DAMPER);
    }
}
