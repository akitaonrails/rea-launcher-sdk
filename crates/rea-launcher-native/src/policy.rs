//! The decisions of the [`sdl`](crate) layer, pure and host-tested
//! (built without SDL): when to rescan devices, when to retry force
//! feedback, how often to rumble, and which SDL effect an output becomes.
//!
//! Each fixes something rea-rad-mobile 0.5.0 got wrong: a plug
//! event inside the rescan throttle was lost ([`Rescan`]); a haptic device
//! without a constant force was reopened, and logged, every 4 ms tick
//! ([`Retry`]); pad rumble went out every tick, an output report each on
//! Bluetooth pads ([`RumbleLimit`]). [`effect_params`] keeps its one hard
//! lesson: SDL's direction says where a force comes *from*.

use std::time::{Duration, Instant};

/// Devices are rescanned at most this often after a plug or unplug.
pub const RESCAN_EVERY: Duration = Duration::from_millis(500);

/// Rescans after device events, at most once per `every`, without
/// forgetting an event that arrives inside that window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rescan {
    every: Duration,
    pending: bool,
    last: Option<Instant>,
}

impl Rescan {
    pub fn new(every: Duration) -> Rescan {
        Rescan {
            every,
            pending: false,
            last: None,
        }
    }

    /// A device was added or removed (or one is no longer attached).
    pub fn note(&mut self, changed: bool) {
        self.pending |= changed;
    }

    /// Whether to rescan now; if so, the event is used up.
    pub fn due(&mut self, now: Instant) -> bool {
        let ready = self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= self.every);
        if self.pending && ready {
            self.pending = false;
            self.last = Some(now);
            return true;
        }
        false
    }

    /// A rescan happened anyway (the first one, say).
    pub fn done(&mut self, now: Instant) {
        self.pending = false;
        self.last = Some(now);
    }
}

/// How force feedback is kept alive, and retried.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HapticPolicy {
    /// The constant force is run again this often: Fanatec and
    /// Thrustmaster firmwares drop an idle effect.
    pub keepalive: Duration,
    /// Stopped damper and friction effects are restarted this often.
    pub revive_check: Duration,
    /// Smaller changes are not worth a USB transfer.
    pub epsilon: f32,
    /// After a device refused force feedback, wait this long before
    /// opening it again (a rescan forgets the failure).
    pub retry_after: Duration,
}

impl Default for HapticPolicy {
    /// rea-rad-mobile's 150 ms keepalive, 250 ms revive and 1/512 epsilon;
    /// a retry every 5 s.
    fn default() -> Self {
        HapticPolicy {
            keepalive: Duration::from_millis(150),
            revive_check: Duration::from_millis(250),
            epsilon: 1.0 / 512.0,
            retry_after: Duration::from_secs(5),
        }
    }
}

/// Remembers which device last refused force feedback, and when.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retry {
    failed: Option<(i32, Instant)>,
}

impl Retry {
    /// Whether device `instance` may be tried now.
    pub fn may_try(&self, instance: i32, now: Instant, after: Duration) -> bool {
        match self.failed {
            Some((i, at)) if i == instance => now.saturating_duration_since(at) >= after,
            _ => true,
        }
    }

    pub fn failed(&mut self, instance: i32, now: Instant) {
        self.failed = Some((instance, now));
    }

    /// Forget the failure (it opened, or the devices changed).
    pub fn clear(&mut self) {
        self.failed = None;
    }
}

/// Pad rumble is resent at most this often; each call lasts
/// [`RUMBLE_HOLD_MS`], so a steady rumble never gaps.
pub const RUMBLE_EVERY: Duration = Duration::from_millis(50);
/// How long one rumble call lasts.
pub const RUMBLE_HOLD_MS: u32 = 100;

/// Rate-limits rumble calls: a start or a stop goes out at once, a
/// running rumble is refreshed every `every`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RumbleLimit {
    every: Duration,
    last: Option<(u16, Instant)>,
}

impl RumbleLimit {
    pub fn new(every: Duration) -> RumbleLimit {
        RumbleLimit { every, last: None }
    }

    /// Whether to send `level` now (and remember it if so).
    pub fn due(&mut self, level: u16, now: Instant) -> bool {
        let send = match self.last {
            None => level > 0,
            Some((last, at)) => {
                (last == 0) != (level == 0)
                    || (level > 0 && now.saturating_duration_since(at) >= self.every)
            }
        };
        if send {
            self.last = Some((level, now));
        }
        send
    }
}

/// A rumble strength (0..1) as SDL's motor level.
pub fn rumble_level(strength: f32) -> u16 {
    (strength.clamp(0.0, 1.0) * 65535.0) as u16
}

/// The effects a wheel runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectKind {
    /// −1..1, positive pushes the wheel right.
    Constant,
    /// 0..1 coefficient.
    Damper,
    /// 0..1 coefficient.
    Friction,
}

/// SDL's cartesian direction of a force that comes from the west, so it
/// pushes the wheel east (right).
pub const FROM_WEST: [i32; 3] = [-1, 0, 0];

/// What an SDL effect is set to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectParams {
    pub kind: EffectKind,
    /// Cartesian: where the force comes *from* (SDL_haptic.h).
    pub direction: [i32; 3],
    /// The constant force's level, −32767..=32767.
    pub level: i16,
    /// The condition's coefficient, 0..=32767.
    pub coeff: i16,
}

/// The effect for an output value `v`: a level for the constant force, a
/// coefficient for the conditions, always "from the west", so a positive
/// level pushes right as `rea_launcher::ffb::FfbOutput::constant` means.
/// East (`[1, 0, 0]`), as rea-rad-mobile 0.4.0 shipped, turned every force
/// around: centring pushed away from the centre on a Fanatec CSL DD.
pub fn effect_params(kind: EffectKind, v: f32) -> EffectParams {
    let scale = |v: f32| (v * 32767.0) as i16;
    let (level, coeff) = match kind {
        EffectKind::Constant => (scale(v.clamp(-1.0, 1.0)), 0),
        EffectKind::Damper | EffectKind::Friction => (0, scale(v.clamp(0.0, 1.0))),
    };
    EffectParams {
        kind,
        direction: FROM_WEST,
        level,
        coeff,
    }
}

/// Whether a value moved enough to send it again.
pub fn changed(new: f32, old: f32, epsilon: f32) -> bool {
    (new - old).abs() > epsilon
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_plug_event_inside_the_throttle_is_kept_for_later() {
        let t0 = Instant::now();
        let mut r = Rescan::new(RESCAN_EVERY);
        r.done(t0);
        assert!(!r.due(t0 + ms(100)), "nothing happened");
        // pedals arrive 100 ms after the first rescan
        r.note(true);
        assert!(!r.due(t0 + ms(100)), "throttled");
        r.note(false);
        assert!(!r.due(t0 + ms(400)));
        assert!(r.due(t0 + ms(500)), "rescanned once the window passed");
        assert!(!r.due(t0 + ms(2000)), "used up");
        let mut fresh = Rescan::new(RESCAN_EVERY);
        fresh.note(true);
        assert!(fresh.due(t0), "the first event needs no wait");
    }

    #[test]
    fn a_refused_device_is_retried_only_after_a_while() {
        let t0 = Instant::now();
        let after = HapticPolicy::default().retry_after;
        let mut retry = Retry::default();
        assert!(retry.may_try(7, t0, after));
        retry.failed(7, t0);
        assert!(!retry.may_try(7, t0 + ms(4), after), "not every tick");
        assert!(retry.may_try(8, t0 + ms(4), after), "another device may");
        assert!(retry.may_try(7, t0 + after, after));
        retry.clear();
        assert!(retry.may_try(7, t0 + ms(8), after));
    }

    #[test]
    fn rumble_starts_and_stops_at_once_and_refreshes_at_a_steady_rate() {
        let t0 = Instant::now();
        let mut r = RumbleLimit::new(RUMBLE_EVERY);
        assert!(!r.due(0, t0), "nothing to stop yet");
        assert!(r.due(30_000, t0));
        assert!(!r.due(31_000, t0 + ms(4)), "not every tick");
        assert!(!r.due(32_000, t0 + ms(40)));
        assert!(r.due(32_000, t0 + ms(50)));
        assert!(r.due(0, t0 + ms(54)), "a stop goes out at once");
        assert!(!r.due(0, t0 + ms(200)));
        assert!(r.due(1, t0 + ms(204)), "and a start");
        assert_eq!(rumble_level(1.5), 65535);
        assert_eq!(rumble_level(-1.0), 0);
    }

    #[test]
    fn a_positive_force_comes_from_the_west_so_it_pushes_right() {
        let c = effect_params(EffectKind::Constant, 0.5);
        assert!(c.direction[0] < 0, "from the west");
        assert_eq!(c.direction, FROM_WEST);
        assert!(c.level > 0);
        assert_eq!(effect_params(EffectKind::Constant, -2.0).level, -32767);
        let d = effect_params(EffectKind::Damper, 2.0);
        assert_eq!((d.level, d.coeff), (0, 32767));
        assert_eq!(effect_params(EffectKind::Friction, -1.0).coeff, 0);
        assert!(changed(0.01, 0.0, HapticPolicy::default().epsilon));
        assert!(!changed(0.001, 0.0, HapticPolicy::default().epsilon));
    }
}
