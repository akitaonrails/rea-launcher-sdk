//! The settings window's state: one `launcher.cfg` text per option row,
//! exactly as the file and the flags spell it. The window edits these
//! texts (`rea_launcher_ui::spec::spec_row`), Play saves them with
//! [`Form::save`], and the same parser as the file and the command line
//! reads them back, so the window, the file and the flags cannot drift.

use rea_launcher::cfg::{self, spec};

use crate::config::{self, Look, OPTIONS, Picture, Settings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// One text per row of [`OPTIONS`], in order; empty is unset.
    values: Vec<String>,
}

impl Form {
    /// The values in a `launcher.cfg` text (the last active line wins, as
    /// when the file is parsed; old key names count).
    pub fn from_text(text: &str) -> Form {
        let mut values = vec![String::new(); OPTIONS.len()];
        for entry in cfg::entries(text) {
            if let Some(i) = OPTIONS.iter().position(|row| row.answers_to(entry.key)) {
                values[i] = entry.value.trim().to_owned();
            }
        }
        Form { values }
    }

    fn index(key: &str) -> usize {
        OPTIONS
            .iter()
            .position(|row| row.key == key)
            .unwrap_or_else(|| panic!("no option row {key}"))
    }

    /// The text of `key` ("" when unset).
    pub fn get(&self, key: &str) -> &str {
        &self.values[Form::index(key)]
    }

    /// The text of `key`, for a window row to edit.
    pub fn value_mut(&mut self, key: &str) -> &mut String {
        &mut self.values[Form::index(key)]
    }

    /// The look chosen, if its values still apply.
    pub fn look(&self) -> Option<Look> {
        Look::from_key(self.get("look"))
    }

    /// A look card was chosen: the look and all of its values.
    pub fn choose_look(&mut self, look: Look, screen: Option<(u32, u32)>) {
        *self.value_mut("look") = look.key().to_owned();
        for (key, value) in look.picture(screen).values() {
            *self.value_mut(key) = value.unwrap_or_default();
        }
    }

    /// A picture option changed by hand: once the values differ from the
    /// look's, no look card stays selected.
    pub fn picture_edited(&mut self, screen: Option<(u32, u32)>) {
        let settings = self.settings();
        let current = Picture {
            resolution: settings.resolution,
            vsync: settings.vsync,
        };
        if let Some(look) = settings.look
            && look.picture(screen) != current
        {
            self.value_mut("look").clear();
        }
    }

    /// Every value the window cannot save, as `Label: needs …`.
    pub fn problems(&self) -> Vec<String> {
        OPTIONS
            .iter()
            .zip(&self.values)
            .filter(|(_, value)| !value.trim().is_empty())
            .filter_map(|(row, value)| {
                row.parse(value)
                    .err()
                    .map(|_| format!("{}: needs {}", row.label, row.needs()))
            })
            .collect()
    }

    /// `text` (the saved `launcher.cfg`, or the template) with the window's
    /// values: each set key written in place, each emptied key commented
    /// out; comments and unknown keys stay.
    pub fn save(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (row, value) in OPTIONS.iter().zip(&self.values) {
            let value = value.trim();
            if !value.is_empty() {
                out = spec::set_key(&out, row, value);
            } else if cfg::entries(&out).any(|e| row.answers_to(e.key)) {
                out = cfg::clear_key(&out, row.key);
                for alias in row.aliases {
                    out = cfg::clear_key(&out, alias);
                }
            }
        }
        out
    }

    /// The settings these values give, read by the launcher's own parser.
    pub fn settings(&self) -> Settings {
        config::parse_file(&self.save(&config::template())).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::cfg::Resolution;

    #[test]
    fn values_round_trip_through_the_file() {
        let text = "# mine\nresolution = 1280x720\nunknown = kept\n";
        let mut form = Form::from_text(text);
        assert_eq!(form.get("resolution"), "1280x720");
        *form.value_mut("vsync") = "off".into();
        form.value_mut("resolution").clear();
        let saved = form.save(text);
        assert!(saved.contains("# mine\n") && saved.contains("unknown = kept\n"), "{saved}");
        assert!(saved.contains("#resolution =\n"), "{saved}");
        assert!(saved.contains("vsync = off\n"), "{saved}");
        assert_eq!(Form::from_text(&saved), form);
    }

    #[test]
    fn a_look_card_sets_its_values_until_one_is_changed() {
        let mut form = Form::from_text("");
        form.choose_look(Look::SteamDeck, None);
        assert_eq!(form.look(), Some(Look::SteamDeck));
        assert_eq!(form.settings().picture(None).resolution, Some(Resolution::Size(1280, 800)));
        form.picture_edited(None);
        assert_eq!(form.look(), Some(Look::SteamDeck), "still the look's values");
        *form.value_mut("vsync") = "off".into();
        form.picture_edited(None);
        assert_eq!(form.look(), None);
        assert_eq!(form.settings().vsync, Some(false));
    }

    #[test]
    fn values_the_window_cannot_save_are_named() {
        let mut form = Form::from_text("");
        *form.value_mut("resolution") = "huge".into();
        assert_eq!(form.problems(), ["Resolution: needs WIDTHxHEIGHT or original"]);
    }
}
