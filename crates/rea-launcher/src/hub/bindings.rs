//! The pad layout a hub launcher's Controls page lists, and the player's
//! changes to it, saved in the data folder's `controls.cfg`
//! ([`super::HubPaths::controls_file`]) as `pad_<action> = <inputs>`
//! lines ([`PadMap::to_cfg`]).
//!
//! The rows: steering first (the left stick or the d-pad, fixed), then gas,
//! brake and every button the launcher's default layout binds, in that
//! order. The rows stay the same when the player moves a binding, so a
//! button whose input went to another row shows "-" and can be bound again.

use std::fs;
use std::io;
use std::path::Path;

use crate::fsx;
use crate::wheel::{Action, Control, PadInput, PadMap};

/// The steering row's words: what steers is fixed.
pub const STEER: (&str, &str) = ("Steer", "Left stick");

/// A launcher's pad layout with the player's changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings<B: Control> {
    defaults: PadMap<B>,
    map: PadMap<B>,
    /// The actions of rows 1…, in order.
    actions: Vec<Action<B>>,
}

impl<B: Control> Bindings<B> {
    /// The layout `defaults`, unchanged.
    pub fn new(defaults: PadMap<B>) -> Self {
        let actions = [Action::Accelerate, Action::Brake]
            .into_iter()
            .chain(
                B::ALL
                    .iter()
                    .map(|&b| Action::Press(b))
                    .filter(|&a| defaults.inputs(a).next().is_some()),
            )
            .collect();
        Bindings {
            map: defaults.clone(),
            defaults,
            actions,
        }
    }

    /// `defaults` with the changes saved in `text` (`controls.cfg`).
    pub fn from_cfg(text: &str, defaults: PadMap<B>) -> Self {
        let mut b = Self::new(defaults);
        b.map = PadMap::from_cfg(text, &b.defaults);
        b
    }

    /// `defaults` with the changes saved in `file`; a missing file is no
    /// change.
    pub fn load(file: &Path, defaults: PadMap<B>) -> io::Result<Self> {
        match fs::read_to_string(file) {
            Ok(text) => Ok(Self::from_cfg(&text, defaults)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new(defaults)),
            Err(e) => Err(e),
        }
    }

    /// Writes the layout into `file`, keeping its other lines.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        let existing = match fs::read_to_string(file) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fsx::write_atomic(file, self.to_cfg(&existing))
    }

    /// The layout written into a settings file's text, every other line
    /// kept ([`PadMap::to_cfg`]).
    pub fn to_cfg(&self, existing: &str) -> String {
        self.map.to_cfg(existing)
    }

    /// What the input thread reads the pad with.
    pub fn map(&self) -> &PadMap<B> {
        &self.map
    }

    /// The Controls page's rows, `(what, inputs)`: steering, then
    /// [`Self::rebindable`]'s.
    pub fn layout(&self) -> Vec<(String, String)> {
        std::iter::once((STEER.0.to_string(), STEER.1.to_string()))
            .chain(
                self.actions
                    .iter()
                    .map(|&a| (label(a).to_string(), self.map.describe(a))),
            )
            .collect()
    }

    /// The rows of [`Self::layout`] the player can change: all but
    /// steering.
    pub fn rebindable(&self) -> Vec<usize> {
        (1..=self.actions.len()).collect()
    }

    /// Binds layout row `row` to `input` alone (the input leaves any other
    /// row); what the page says. `None` for a row that can't change.
    pub fn rebind(&mut self, row: usize, input: PadInput) -> Option<String> {
        let action = *self.actions.get(row.checked_sub(1)?)?;
        let had: Vec<Action<B>> = self
            .actions
            .iter()
            .copied()
            .filter(|&a| a != action && self.map.inputs(a).any(|i| i == input))
            .collect();
        self.map.assign(action, input);
        let mut said = format!("{}: {}", label(action), input.label());
        // a row that kept another input of its own isn't worth a word
        for a in had
            .into_iter()
            .filter(|&a| self.map.inputs(a).next().is_none())
        {
            said.push_str(&format!(" ({} has none now)", label(a)));
        }
        Some(said)
    }

    /// Back to the launcher's layout.
    pub fn reset(&mut self) {
        self.map = self.defaults.clone();
    }

    /// [`Self::rebind`], then [`Self::save`] into `file`: what the page
    /// says, or why it can't. The layout in memory changes even when the
    /// file can't be written (the input thread can use it until the
    /// launcher closes).
    pub fn rebind_saved(
        &mut self,
        row: usize,
        input: PadInput,
        file: &Path,
    ) -> Result<String, String> {
        let said = self
            .rebind(row, input)
            .ok_or_else(|| "That row can't be changed.".to_string())?;
        self.saved(said, file)
    }

    /// [`Self::reset`], then [`Self::save`] into `file`, as
    /// [`Self::rebind_saved`].
    pub fn reset_saved(&mut self, file: &Path) -> Result<String, String> {
        self.reset();
        self.saved("Back to the launcher's pad layout".into(), file)
    }

    fn saved(&self, said: String, file: &Path) -> Result<String, String> {
        match self.save(file) {
            Ok(()) => Ok(format!("{said}. Saved.")),
            Err(e) => Err(format!(
                "{said}, until the launcher closes: {} can't be written ({e})",
                file.display()
            )),
        }
    }

    /// Whether the layout differs from the launcher's.
    pub fn changed(&self) -> bool {
        self.map != self.defaults
    }
}

fn label<B: Control>(a: Action<B>) -> &'static str {
    match a {
        Action::Accelerate => "Gas",
        Action::Brake => "Brake",
        Action::Press(b) => b.label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wheel::test_controls::Button;

    fn defaults() -> PadMap<Button> {
        PadMap::with(&[
            (Action::Accelerate, &[PadInput::RightTrigger]),
            (Action::Brake, &[PadInput::LeftTrigger]),
            (Action::Press(Button::ALL[0]), &[PadInput::A]),
        ])
    }

    #[test]
    fn the_rows_are_steering_then_the_bound_actions() {
        let b = Bindings::new(defaults());
        let rows = b.layout();
        assert_eq!(rows[0], ("Steer".into(), "Left stick".into()));
        assert_eq!(rows[1], ("Gas".into(), "RT".into()));
        assert_eq!(rows[2], ("Brake".into(), "LT".into()));
        assert_eq!(rows[3], (Button::ALL[0].label().into(), "A".into()));
        assert_eq!(rows.len(), 4, "unbound buttons are not listed");
        assert_eq!(b.rebindable(), [1, 2, 3]);
    }

    #[test]
    fn a_rebind_moves_the_input_and_keeps_the_rows() {
        let mut b = Bindings::new(defaults());
        assert_eq!(b.rebind(0, PadInput::B), None, "steering is fixed");
        assert_eq!(b.rebind(9, PadInput::B), None);
        assert_eq!(b.rebind(1, PadInput::B).as_deref(), Some("Gas: B"));
        assert!(b.changed());
        // A goes from the button to the gas: the button's row stays, unbound
        let said = b.rebind(1, PadInput::A).unwrap();
        assert_eq!(
            said,
            format!("Gas: A ({} has none now)", Button::ALL[0].label())
        );
        assert!(said.contains("has none now"), "{said}");
        let rows = b.layout();
        assert_eq!(rows[1].1, "A");
        assert_eq!(rows[3].1, "-");
        assert_eq!(rows.len(), 4);
        b.reset();
        assert!(!b.changed());
        assert_eq!(b.layout()[1].1, "RT");
    }

    #[test]
    fn a_row_that_keeps_another_input_is_not_called_empty() {
        let mut b = Bindings::new(PadMap::with(&[
            (Action::Accelerate, &[PadInput::RightTrigger]),
            (
                Action::Press(Button::ALL[0]),
                &[PadInput::Start, PadInput::A],
            ),
        ]));
        assert_eq!(b.rebind(1, PadInput::A).as_deref(), Some("Gas: A"));
        assert_eq!(b.layout()[3].1, "START");
    }

    #[test]
    fn the_layout_is_saved_and_read_back_with_the_files_other_lines() {
        let dir = std::env::temp_dir().join(format!("rea-bindings-{}", std::process::id()));
        let file = dir.join("controls.cfg");
        let _ = fs::remove_dir_all(&dir);
        let fresh = Bindings::load(&file, defaults()).unwrap();
        assert!(!fresh.changed(), "no file: the launcher's layout");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&file, "# mine\nother = 1\n").unwrap();
        let mut b = fresh;
        b.rebind(2, PadInput::X).unwrap();
        b.save(&file).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("# mine") && text.contains("other = 1"),
            "{text}"
        );
        let mut back = Bindings::load(&file, defaults()).unwrap();
        assert_eq!(back, b);
        assert_eq!(back.layout()[2].1, "X");
        assert_eq!(
            back.reset_saved(&file).as_deref(),
            Ok("Back to the launcher's pad layout. Saved.")
        );
        assert!(!Bindings::load(&file, defaults()).unwrap().changed());
        // a folder where the file should be: the change holds, unsaved
        let blocked = dir.join("blocked");
        fs::create_dir_all(&blocked).unwrap();
        let err = back.rebind_saved(1, PadInput::Y, &blocked).unwrap_err();
        assert!(
            err.starts_with("Gas: Y, until the launcher closes"),
            "{err}"
        );
        assert_eq!(back.layout()[1].1, "Y");
        assert_eq!(
            back.rebind_saved(0, PadInput::Y, &file),
            Err("That row can't be changed.".into())
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
