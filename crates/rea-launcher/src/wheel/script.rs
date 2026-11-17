//! A timed input script for headless tests (a hub's `--input-script`):
//! `<seconds> <inputs…>` per line, the inputs held until the next line; `-`
//! releases everything; `#` starts a comment. Inputs are the launcher's
//! buttons by [`Control::key`], `accel`, `brake`, `steer=<-1..1>` and
//! `pitch=<-1..1>` (a handle's up/down, or a gun's crosshair height). From
//! the TeknoParrot hubs' `input.rs`.

use super::{Control, Controls};

#[derive(Debug, Clone, PartialEq)]
pub struct Script {
    /// (from, controls, pitch), in time order.
    steps: Vec<(f64, Controls, f32)>,
}

impl Script {
    pub fn parse<B: Control>(text: &str) -> Result<Script, String> {
        let mut steps = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let bad = |w: &str| format!("line {}: {w}", n + 1);
            let mut words = line.split_whitespace();
            let t: f64 = words
                .next()
                .and_then(|w| w.parse().ok())
                .ok_or_else(|| bad("no time"))?;
            let (mut r, mut pitch) = (Controls::default(), 0.0f32);
            for w in words.filter(|w| *w != "-") {
                let number = |v: &str| v.parse::<f32>().map_err(|_| bad(w));
                if w == "accel" {
                    r.accel = 1.0;
                } else if w == "brake" {
                    r.brake = 1.0;
                } else if let Some(v) = w.strip_prefix("steer=") {
                    r.steer = number(v)?;
                } else if let Some(v) = w.strip_prefix("pitch=") {
                    pitch = number(v)?;
                } else {
                    let b = B::ALL
                        .iter()
                        .find(|b| b.key() == w)
                        .ok_or_else(|| bad(&format!("unknown input {w}")))?;
                    r.press(*b);
                }
            }
            steps.push((t, r, pitch));
        }
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Script { steps })
    }

    /// The controls and the pitch held at `seconds`.
    pub fn at(&self, seconds: f64) -> (Controls, f32) {
        self.steps
            .iter()
            .rev()
            .find(|(t, _, _)| *t <= seconds)
            .map(|&(_, r, p)| (r, p))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Button {
        Start,
        View,
    }

    impl Control for Button {
        const ALL: &'static [Button] = &[Button::Start, Button::View];
        fn key(self) -> &'static str {
            match self {
                Button::Start => "start",
                Button::View => "view",
            }
        }
        fn label(self) -> &'static str {
            self.key()
        }
        fn bit(self) -> u32 {
            1 << self as u32
        }
    }

    #[test]
    fn a_script_holds_each_step_until_the_next() {
        let s = Script::parse::<Button>(
            "# boot\n10 start\n10.5 -\n20 accel steer=-0.5 pitch=-0.25 view # go\n",
        )
        .unwrap();
        assert_eq!(s.at(5.0), (Controls::default(), 0.0));
        assert_eq!(s.at(10.2).0.buttons, Button::Start.bit());
        assert_eq!(s.at(12.0).0, Controls::default());
        let (r, pitch) = s.at(25.0);
        assert_eq!((r.accel, r.brake, r.steer, pitch), (1.0, 0.0, -0.5, -0.25));
        assert_eq!(r.buttons, Button::View.bit());
        // written out of order: read in time order
        let s = Script::parse::<Button>("2 brake\n1 start\n").unwrap();
        assert_eq!(s.at(1.5).0.buttons, Button::Start.bit());
        assert_eq!(s.at(2.0).0.brake, 1.0);
    }

    #[test]
    fn a_bad_script_says_which_line() {
        assert_eq!(
            Script::parse::<Button>("1 jump").unwrap_err(),
            "line 1: unknown input jump"
        );
        assert_eq!(
            Script::parse::<Button>("\nsoon start").unwrap_err(),
            "line 2: no time"
        );
        assert_eq!(
            Script::parse::<Button>("1 steer=left").unwrap_err(),
            "line 1: steer=left"
        );
    }
}
