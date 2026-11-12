//! Gamepads as racing controls: which pad buttons and triggers drive each
//! [`Action`], how a map is saved, how a new binding is captured in a menu,
//! and [`read_pad`]. From rea-rad-mobile `pad_map.rs` and `wheel/mod.rs`
//! (`PadState`, `PadSettings`, `read_pad`).
//!
//! Steering always comes from the left stick and the d-pad. Every other
//! action can be moved; the default layout is the launcher's
//! ([`PadMap::with`]). Back + Start together always means the launcher's
//! exit button, whatever is bound ([`Exit`]).

use std::marker::PhantomData;

use super::{Action, Control, Controls, Keys, RAW_MAX, shape, with_header};
use crate::{cfg, pad};

/// A gamepad as SDL's game-controller layer sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PadState {
    pub left_x: i16,
    /// SDL's Y axis points down.
    pub left_y: i16,
    /// 0..=32767
    pub left_trigger: i16,
    pub right_trigger: i16,
    pub a: bool,
    pub b: bool,
    pub x: bool,
    pub y: bool,
    pub start: bool,
    pub back: bool,
    pub guide: bool,
    pub left_shoulder: bool,
    pub right_shoulder: bool,
    pub left_stick: bool,
    pub right_stick: bool,
    pub dpad_left: bool,
    pub dpad_right: bool,
    pub dpad_up: bool,
    pub dpad_down: bool,
}

impl PadState {
    /// The pad in XInput's button bits ([`crate::pad`]), with the left
    /// stick leaning past [`pad::STICK_THRESHOLD`] as the d-pad, so
    /// [`pad::PadNav`] and `rea_launcher_ui::pad` drive a menu from SDL
    /// pads too.
    pub fn nav_bits(&self) -> u16 {
        [
            (
                self.dpad_up || self.left_y < -pad::STICK_THRESHOLD,
                pad::DPAD_UP,
            ),
            (
                self.dpad_down || self.left_y > pad::STICK_THRESHOLD,
                pad::DPAD_DOWN,
            ),
            (
                self.dpad_left || self.left_x < -pad::STICK_THRESHOLD,
                pad::DPAD_LEFT,
            ),
            (
                self.dpad_right || self.left_x > pad::STICK_THRESHOLD,
                pad::DPAD_RIGHT,
            ),
            (self.start, pad::START),
            (self.back, pad::BACK),
            (self.left_stick, pad::LEFT_THUMB),
            (self.right_stick, pad::RIGHT_THUMB),
            (self.left_shoulder, pad::LEFT_SHOULDER),
            (self.right_shoulder, pad::RIGHT_SHOULDER),
            (self.a, pad::A),
            (self.b, pad::B),
            (self.x, pad::X),
            (self.y, pad::Y),
        ]
        .into_iter()
        .filter(|(down, _)| *down)
        .fold(0, |bits, (_, bit)| bits | bit)
    }
}

/// A wheel's hat, (x, y) each −1, 0 or 1 with y down, as d-pad bits.
pub fn hat_bits((x, y): (i8, i8)) -> u16 {
    let mut bits = 0;
    if y < 0 {
        bits |= pad::DPAD_UP;
    }
    if y > 0 {
        bits |= pad::DPAD_DOWN;
    }
    if x < 0 {
        bits |= pad::DPAD_LEFT;
    }
    if x > 0 {
        bits |= pad::DPAD_RIGHT;
    }
    bits
}

/// A pad button or trigger, as SDL's controller layer names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PadInput {
    A,
    B,
    X,
    Y,
    LeftShoulder,
    RightShoulder,
    LeftTrigger,
    RightTrigger,
    Back,
    Start,
    Guide,
    LeftStick,
    RightStick,
}

/// A trigger pulled past half way counts as a press.
pub const TRIGGER_PRESS: f32 = 0.5;

impl PadInput {
    pub const ALL: [PadInput; 13] = [
        PadInput::A,
        PadInput::B,
        PadInput::X,
        PadInput::Y,
        PadInput::LeftShoulder,
        PadInput::RightShoulder,
        PadInput::LeftTrigger,
        PadInput::RightTrigger,
        PadInput::Back,
        PadInput::Start,
        PadInput::Guide,
        PadInput::LeftStick,
        PadInput::RightStick,
    ];

    /// The name a menu shows ("RT", "L3").
    pub fn label(self) -> &'static str {
        match self {
            PadInput::A => "A",
            PadInput::B => "B",
            PadInput::X => "X",
            PadInput::Y => "Y",
            PadInput::LeftShoulder => "LB",
            PadInput::RightShoulder => "RB",
            PadInput::LeftTrigger => "LT",
            PadInput::RightTrigger => "RT",
            PadInput::Back => "BACK",
            PadInput::Start => "START",
            PadInput::Guide => "GUIDE",
            PadInput::LeftStick => "L3",
            PadInput::RightStick => "R3",
        }
    }

    /// The name in settings files ("rt", "l3").
    pub fn key(self) -> &'static str {
        match self {
            PadInput::A => "a",
            PadInput::B => "b",
            PadInput::X => "x",
            PadInput::Y => "y",
            PadInput::LeftShoulder => "lb",
            PadInput::RightShoulder => "rb",
            PadInput::LeftTrigger => "lt",
            PadInput::RightTrigger => "rt",
            PadInput::Back => "back",
            PadInput::Start => "start",
            PadInput::Guide => "guide",
            PadInput::LeftStick => "l3",
            PadInput::RightStick => "r3",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        Self::ALL.into_iter().find(|i| i.key() == s)
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }

    /// How far a trigger is pulled (0..=1, raw), or `None` for a button.
    pub fn trigger(self, pad: &PadState) -> Option<f32> {
        let raw = match self {
            PadInput::LeftTrigger => pad.left_trigger,
            PadInput::RightTrigger => pad.right_trigger,
            _ => return None,
        };
        Some((f32::from(raw.max(0)) / RAW_MAX).min(1.0))
    }

    pub fn is_down(self, pad: &PadState) -> bool {
        if let Some(pull) = self.trigger(pad) {
            return pull > TRIGGER_PRESS;
        }
        match self {
            PadInput::A => pad.a,
            PadInput::B => pad.b,
            PadInput::X => pad.x,
            PadInput::Y => pad.y,
            PadInput::LeftShoulder => pad.left_shoulder,
            PadInput::RightShoulder => pad.right_shoulder,
            PadInput::Back => pad.back,
            PadInput::Start => pad.start,
            PadInput::Guide => pad.guide,
            PadInput::LeftStick => pad.left_stick,
            PadInput::RightStick => pad.right_stick,
            PadInput::LeftTrigger | PadInput::RightTrigger => false,
        }
    }
}

/// For every [`Action`], the pad inputs that drive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PadMap<B> {
    /// [`PadInput`] bits, per [`Action::index`].
    bits: Vec<u16>,
    controls: PhantomData<B>,
}

impl<B: Control> Default for PadMap<B> {
    /// Nothing bound.
    fn default() -> Self {
        PadMap {
            bits: vec![0; Action::<B>::count()],
            controls: PhantomData,
        }
    }
}

impl<B: Control> PadMap<B> {
    /// The launcher's default layout. Rad Mobile's:
    /// `(Accelerate, [RightTrigger, A]), (Brake, [LeftTrigger, B]), …`.
    pub fn with(bindings: &[(Action<B>, &[PadInput])]) -> Self {
        let mut map = PadMap::default();
        for (action, inputs) in bindings {
            map.set(*action, inputs);
        }
        map
    }

    /// The inputs bound to `action`, in [`PadInput::ALL`] order.
    pub fn inputs(&self, action: Action<B>) -> impl Iterator<Item = PadInput> + '_ {
        let bits = self.bits[action.index()];
        PadInput::ALL
            .into_iter()
            .filter(move |i| bits & i.bit() != 0)
    }

    pub fn set(&mut self, action: Action<B>, inputs: &[PadInput]) {
        self.bits[action.index()] = inputs.iter().fold(0, |b, i| b | i.bit());
    }

    /// Bind `input` to `action` alone, taking it away from any other
    /// action so one press never does two things.
    pub fn assign(&mut self, action: Action<B>, input: PadInput) {
        for bits in &mut self.bits {
            *bits &= !input.bit();
        }
        self.bits[action.index()] = input.bit();
    }

    /// Whether any input bound to `action` is held.
    pub fn is_down(&self, action: Action<B>, pad: &PadState) -> bool {
        self.inputs(action).any(|i| i.is_down(pad))
    }

    /// What a menu shows: "A / RT", or "-" when unbound.
    pub fn describe(&self, action: Action<B>) -> String {
        let names: Vec<_> = self.inputs(action).map(PadInput::label).collect();
        if names.is_empty() {
            "-".into()
        } else {
            names.join(" / ")
        }
    }

    /// The map written into a settings file's text: `pad_<action> = a,rt`
    /// (or `none`) for every action (rea-rad-mobile's keys); every other
    /// line stays.
    pub fn to_cfg(&self, existing: &str) -> String {
        let mut text = with_header(existing, "");
        for action in Action::<B>::all() {
            let keys: Vec<_> = self.inputs(action).map(PadInput::key).collect();
            let value = if keys.is_empty() {
                "none".to_owned()
            } else {
                keys.join(",")
            };
            text = cfg::set_key(&text, &format!("pad_{}", action.key()), &value);
        }
        text
    }

    /// The map from a settings file: actions without a `pad_<action>` key
    /// keep `defaults`; unknown input names are skipped.
    pub fn from_cfg(text: &str, defaults: &Self) -> Self {
        let keys = Keys::of(text);
        let mut map = defaults.clone();
        for action in Action::<B>::all() {
            if let Some(value) = keys.get(&format!("pad_{}", action.key())) {
                let inputs: Vec<_> = value
                    .split(',')
                    .filter_map(|s| PadInput::parse(s.trim()))
                    .collect();
                map.set(action, &inputs);
            }
        }
        map
    }
}

/// How a gamepad drives the controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PadSettings<B> {
    /// Of 32767.
    pub stick_deadzone: u16,
    pub stick_linearity: u8,
    /// Of 255.
    pub trigger_deadzone: u8,
    pub trigger_linearity: u8,
    pub map: PadMap<B>,
}

impl<B: Control> PadSettings<B> {
    /// rea-rad-mobile's defaults (stick deadzone 3000, trigger deadzone
    /// 12, both linear) with the launcher's `map`.
    pub fn new(map: PadMap<B>) -> Self {
        Self {
            stick_deadzone: 3000,
            stick_linearity: 0,
            trigger_deadzone: 12,
            trigger_linearity: 0,
            map,
        }
    }
}

/// The launcher's way out of the game on a pad: `button` is pressed by its
/// own binding or by Back + Start together, and while it is, the buttons
/// in `suppress` (Rad Mobile's Start and Coin, which Back + Start would
/// otherwise press too) are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit<'a, B> {
    pub button: B,
    pub suppress: &'a [B],
}

/// A gamepad's controls. The left stick (or the d-pad, which wins) steers
/// through the deadzone and curve; everything else follows the map. A
/// trigger is a pedal through its deadzone and curve; a button floors it.
pub fn read_pad<B: Control>(
    settings: &PadSettings<B>,
    pad: &PadState,
    exit: Option<Exit<'_, B>>,
) -> Controls {
    let stick = shape(
        f32::from(pad.left_x) / RAW_MAX,
        f32::from(settings.stick_deadzone) / RAW_MAX,
        settings.stick_linearity,
    );
    let dpad = match (pad.dpad_left, pad.dpad_right) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => 0.0,
    };
    let pedal = |action: Action<B>| {
        settings
            .map
            .inputs(action)
            .map(|input| match input.trigger(pad) {
                Some(pull) => shape(
                    pull,
                    f32::from(settings.trigger_deadzone) / 255.0,
                    settings.trigger_linearity,
                ),
                None if input.is_down(pad) => 1.0,
                None => 0.0,
            })
            .fold(0.0, f32::max)
    };
    let mut controls = Controls {
        steer: if dpad != 0.0 { dpad } else { stick },
        accel: pedal(Action::Accelerate),
        brake: pedal(Action::Brake),
        buttons: 0,
    };
    let exiting = exit.is_some_and(|e| {
        (pad.back && pad.start) || settings.map.is_down(Action::Press(e.button), pad)
    });
    for &button in B::ALL {
        let pressed = match exit {
            Some(e) if e.button == button => exiting,
            Some(e) if e.suppress.contains(&button) => {
                !exiting && settings.map.is_down(Action::Press(button), pad)
            }
            _ => settings.map.is_down(Action::Press(button), pad),
        };
        if pressed {
            controls.press(button);
        }
    }
    controls
}

/// Waiting in a menu for the pad input an action should move to.
#[derive(Debug, Clone, Default)]
pub struct PadCapture {
    started_ms: Option<u64>,
    /// Every input was let go since the capture began (the button that
    /// opened it is still down at first).
    released: bool,
}

/// How long a menu waits for a press before giving up.
pub const CAPTURE_TIMEOUT_MS: u64 = 6_000;

/// What a [`PadCapture`] tick saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture<T> {
    /// Waiting for every input to be let go first.
    Releasing,
    Waiting {
        remaining_ms: u64,
    },
    Got(T),
    TimedOut,
}

impl PadCapture {
    /// Feed every pad's state; `now_ms` is a monotonic clock.
    pub fn tick(&mut self, now_ms: u64, pads: &[PadState]) -> Capture<PadInput> {
        let started = *self.started_ms.get_or_insert(now_ms);
        let elapsed = now_ms.saturating_sub(started);
        let held = pads
            .iter()
            .find_map(|pad| PadInput::ALL.into_iter().find(|i| i.is_down(pad)));
        if !self.released {
            if held.is_some() {
                return if elapsed >= CAPTURE_TIMEOUT_MS {
                    Capture::TimedOut
                } else {
                    Capture::Releasing
                };
            }
            self.released = true;
        }
        if let Some(input) = held {
            return Capture::Got(input);
        }
        if elapsed >= CAPTURE_TIMEOUT_MS {
            Capture::TimedOut
        } else {
            Capture::Waiting {
                remaining_ms: CAPTURE_TIMEOUT_MS - elapsed,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_controls::Button;
    use super::*;

    type Map = PadMap<Button>;

    /// Rad Mobile's default layout.
    fn rad_default() -> Map {
        use PadInput::*;
        PadMap::with(&[
            (Action::Accelerate, &[RightTrigger, A]),
            (Action::Brake, &[LeftTrigger, B]),
            (Action::Press(Button::Light), &[X, LeftShoulder]),
            (Action::Press(Button::Wiper), &[Y, RightShoulder]),
            (Action::Press(Button::Start), &[Start]),
            (Action::Press(Button::Coin), &[Back]),
            (Action::Press(Button::Exit), &[Guide]),
        ])
    }

    const EXIT: Option<Exit<'static, Button>> = Some(Exit {
        button: Button::Exit,
        suppress: &[Button::Start, Button::Coin],
    });

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn defaults_are_the_documented_layout() {
        let map = rad_default();
        assert_eq!(map.describe(Action::Accelerate), "A / RT");
        assert_eq!(map.describe(Action::Brake), "B / LT");
        assert_eq!(map.describe(Action::Press(Button::Light)), "X / LB");
        assert_eq!(map.describe(Action::Press(Button::Wiper)), "Y / RB");
        assert_eq!(map.describe(Action::Press(Button::Start)), "START");
        assert_eq!(map.describe(Action::Press(Button::Coin)), "BACK");
        assert_eq!(map.describe(Action::Press(Button::Exit)), "GUIDE");
        assert_eq!(map.describe(Action::Press(Button::Pause)), "-");
        assert_eq!(Map::default().describe(Action::Accelerate), "-");
    }

    #[test]
    fn every_action_has_its_own_slot_and_key() {
        let mut seen = std::collections::HashSet::new();
        for a in Action::<Button>::all() {
            assert!(seen.insert(a.index()), "{a:?}");
        }
        assert_eq!(seen.len(), Action::<Button>::count());
        let keys: std::collections::HashSet<_> = Action::<Button>::all().map(|a| a.key()).collect();
        assert_eq!(keys.len(), Action::<Button>::count());
    }

    #[test]
    fn assigning_moves_the_input_away_from_other_controls() {
        let mut map = rad_default();
        map.assign(Action::Press(Button::Light), PadInput::A);
        assert_eq!(map.describe(Action::Press(Button::Light)), "A");
        assert_eq!(map.describe(Action::Accelerate), "RT");
    }

    #[test]
    fn maps_round_trip_through_cfg() {
        let mut map = rad_default();
        map.assign(Action::Press(Button::Pause), PadInput::RightStick);
        map.set(Action::Press(Button::Coin), &[]);
        let text = map.to_cfg("# settings.ini\ndifficulty = 3\n");
        assert!(text.starts_with("# settings.ini\ndifficulty = 3\n"));
        assert!(text.contains("pad_coin = none\n"));
        assert!(text.contains("pad_accel = a,rt\n"));
        assert_eq!(Map::from_cfg(&text, &rad_default()), map);
        // rea-rad-mobile's own form reads the same
        let rad = "pad_coin=none\nPAD_ACCEL=a,RT\n";
        let read = Map::from_cfg(rad, &rad_default());
        assert_eq!(read.describe(Action::Accelerate), "A / RT");
        assert_eq!(read.describe(Action::Press(Button::Coin)), "-");
    }

    #[test]
    fn missing_or_unknown_entries_keep_defaults() {
        let map = Map::from_cfg("pad_light=x,bogus\n", &rad_default());
        assert_eq!(map.describe(Action::Press(Button::Light)), "X");
        assert_eq!(map.describe(Action::Accelerate), "A / RT");
    }

    #[test]
    fn a_trigger_counts_as_pressed_past_half_way() {
        let mut pad = PadState {
            right_trigger: 10_000,
            ..Default::default()
        };
        assert!(!PadInput::RightTrigger.is_down(&pad));
        pad.right_trigger = 30_000;
        assert!(PadInput::RightTrigger.is_down(&pad));
        assert_eq!(PadInput::A.trigger(&pad), None);
    }

    #[test]
    fn capture_waits_for_the_opening_press_to_be_let_go() {
        let mut cap = PadCapture::default();
        let a = PadState {
            a: true,
            ..Default::default()
        };
        let idle = PadState::default();
        assert_eq!(cap.tick(0, &[a]), Capture::Releasing);
        assert!(matches!(cap.tick(10, &[idle]), Capture::Waiting { .. }));
        let lb = PadState {
            left_shoulder: true,
            ..Default::default()
        };
        assert_eq!(
            cap.tick(20, &[idle, lb]),
            Capture::Got(PadInput::LeftShoulder)
        );
    }

    #[test]
    fn capture_gives_up_after_a_while() {
        let mut cap = PadCapture::default();
        cap.tick(0, &[PadState::default()]);
        assert_eq!(
            cap.tick(CAPTURE_TIMEOUT_MS, &[PadState::default()]),
            Capture::TimedOut
        );
        // a button stuck down from the start times out too
        let mut stuck = PadCapture::default();
        let a = PadState {
            a: true,
            ..Default::default()
        };
        stuck.tick(0, &[a]);
        assert_eq!(stuck.tick(CAPTURE_TIMEOUT_MS, &[a]), Capture::TimedOut);
    }

    #[test]
    fn gamepad_layout() {
        let s = PadSettings::new(rad_default());
        let mut pad = PadState {
            left_x: -32_767,
            right_trigger: 32_767,
            x: true,
            ..Default::default()
        };
        let c = read_pad(&s, &pad, EXIT);
        assert!(approx(c.steer, -1.0));
        assert!(approx(c.accel, 1.0));
        assert!(c.is_pressed(Button::Light));

        pad = PadState {
            b: true,
            dpad_right: true,
            start: true,
            ..Default::default()
        };
        let c = read_pad(&s, &pad, EXIT);
        assert_eq!(c.steer, 1.0);
        assert_eq!(c.brake, 1.0);
        assert!(c.is_pressed(Button::Start) && !c.is_pressed(Button::Exit));

        pad.back = true;
        let c = read_pad(&s, &pad, EXIT);
        assert!(
            c.is_pressed(Button::Exit)
                && !c.is_pressed(Button::Start)
                && !c.is_pressed(Button::Coin)
        );
        // without an exit rule, Back + Start are just Coin and Start
        let c = read_pad(&s, &pad, None);
        assert!(
            !c.is_pressed(Button::Exit)
                && c.is_pressed(Button::Start)
                && c.is_pressed(Button::Coin)
        );

        // the stick's deadzone keeps a resting stick centred
        let drift = PadState {
            left_x: 2_000,
            ..Default::default()
        };
        assert_eq!(read_pad(&s, &drift, EXIT).steer, 0.0);
    }

    #[test]
    fn a_remapped_pad_follows_its_map() {
        let mut s = PadSettings::new(rad_default());
        s.map.assign(Action::Accelerate, PadInput::RightShoulder);
        s.map.assign(Action::Press(Button::Light), PadInput::A);
        let pad = PadState {
            right_shoulder: true,
            a: true,
            right_trigger: 32_767,
            ..Default::default()
        };
        let c = read_pad(&s, &pad, EXIT);
        // RB is the accelerator now, A the lights; RT does nothing
        assert_eq!(c.accel, 1.0);
        assert!(c.is_pressed(Button::Light) && !c.is_pressed(Button::Wiper));
        let trigger_only = PadState {
            right_trigger: 32_767,
            ..Default::default()
        };
        assert_eq!(read_pad(&s, &trigger_only, EXIT).accel, 0.0);
    }

    #[test]
    fn back_and_start_leave_the_game_whatever_is_bound() {
        let mut s = PadSettings::new(rad_default());
        s.map.set(Action::Press(Button::Exit), &[]);
        let pad = PadState {
            back: true,
            start: true,
            ..Default::default()
        };
        let c = read_pad(&s, &pad, EXIT);
        assert!(c.is_pressed(Button::Exit) && !c.is_pressed(Button::Start));
    }

    #[test]
    fn pads_and_hats_navigate_in_xinput_bits() {
        let p = PadState {
            a: true,
            left_y: 30_000, // SDL: down
            left_x: -1_000,
            ..Default::default()
        };
        assert_eq!(p.nav_bits(), pad::A | pad::DPAD_DOWN);
        let p = PadState {
            dpad_up: true,
            b: true,
            start: true,
            back: true,
            left_shoulder: true,
            right_shoulder: true,
            x: true,
            y: true,
            left_stick: true,
            right_stick: true,
            left_x: 32_767,
            ..Default::default()
        };
        assert_eq!(
            p.nav_bits(),
            pad::DPAD_UP
                | pad::DPAD_RIGHT
                | pad::B
                | pad::START
                | pad::BACK
                | pad::LEFT_SHOULDER
                | pad::RIGHT_SHOULDER
                | pad::X
                | pad::Y
                | pad::LEFT_THUMB
                | pad::RIGHT_THUMB
        );
        assert_eq!(hat_bits((0, 0)), 0);
        assert_eq!(hat_bits((-1, -1)), pad::DPAD_LEFT | pad::DPAD_UP);
        assert_eq!(hat_bits((1, 1)), pad::DPAD_RIGHT | pad::DPAD_DOWN);
    }
}
