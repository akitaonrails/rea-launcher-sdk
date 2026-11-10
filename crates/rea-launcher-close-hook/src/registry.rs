//! Registry redirect: the game's `HKLM` key answered from `HKCU`.
//!
//! An unelevated launcher can't write `HKLM`. Its writes land in the HKCU
//! VirtualStore, and the game sees that copy only while UAC registry
//! virtualization is on for the game process, which Windows doesn't
//! promise: off by policy, per key, and always off for an executable whose
//! manifest has a `requestedExecutionLevel` (Sega Rally Revo's `asInvoker`).
//! A CMR1 player's Windows 11 showed the game nothing and it crashed
//! (rea-colin-mcrae#6). So the launcher writes the game's values to `HKCU`
//! and this DLL, in the game's import table, sends the game's opens of its
//! `HKLM` key there.
//!
//! Configured by [`ENV`] on the game process: rules separated by `;` or new
//! lines, each `FROM -> TO` (root names `HKLM`, `HKCU` or their long
//! `HKEY_…` forms):
//!
//! ```text
//! HKLM\SOFTWARE\Codemasters\Colin McRae Rally 04 -> HKCU\Software\Codemasters\Colin McRae Rally 04
//! walk HKLM\SOFTWARE\Codemasters\Rally\1.00 -> HKCU\Software\Codemasters\Rally\1.00
//! ```
//!
//! A plain rule matches an open of `FROM` or of a key below it (whole
//! components, any case) and opens the same place under `TO` instead.
//! `walk` is for games that open `SOFTWARE` from `HKLM` and go down one
//! level at a time (CMR1): an open of an ancestor of `FROM` on its path
//! (`HKLM\SOFTWARE`, `HKLM\SOFTWARE\Codemasters`, …) opens the matching
//! ancestor of `TO`, and the hook remembers the handle; an open relative
//! to it that leaves `FROM`'s path (`SOFTWARE` → `Microsoft`) goes to the
//! original `HKLM` key, so only `FROM`'s path changes. A `walk` rule's
//! `FROM` and `TO` have the same number of components.
//!
//! The launcher checks its rules with the same parser
//! (`rea_close_hook::registry::Config::parse`) and sets
//! [`Config::env_value`] on the game.

use std::fmt;

/// The environment variable the DLL reads when it loads.
pub const ENV: &str = "REA_REGISTRY";

/// A registry root a rule can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    LocalMachine,
    CurrentUser,
}

impl Root {
    fn parse(name: &str) -> Option<Root> {
        match name.to_ascii_uppercase().as_str() {
            "HKLM" | "HKEY_LOCAL_MACHINE" => Some(Root::LocalMachine),
            "HKCU" | "HKEY_CURRENT_USER" => Some(Root::CurrentUser),
            _ => None,
        }
    }

    /// `HKLM` or `HKCU`.
    pub fn name(self) -> &'static str {
        match self {
            Root::LocalMachine => "HKLM",
            Root::CurrentUser => "HKCU",
        }
    }
}

/// A key: a root and the path below it (`\`-separated, no empty
/// components; empty for the root itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub root: Root,
    pub path: String,
}

impl Key {
    /// `path` under `root`, with empty components (`\\`, a leading or
    /// trailing `\`) dropped.
    pub fn new(root: Root, path: &str) -> Key {
        Key {
            root,
            path: components(path).collect::<Vec<_>>().join("\\"),
        }
    }

    /// `HKLM\SOFTWARE\…`.
    pub fn parse(text: &str) -> Result<Key, ConfigError> {
        let text = text.trim();
        let (root, path) = text.split_once('\\').unwrap_or((text, ""));
        let root = Root::parse(root.trim())
            .ok_or_else(|| ConfigError(format!("unknown registry root in {text:?}")))?;
        Ok(Key::new(root, path))
    }

    /// The key `subkey` below this one, as an open relative to a handle of
    /// this key reaches it.
    pub fn join(&self, subkey: &str) -> Key {
        Key::new(self.root, &format!("{}\\{subkey}", self.path))
    }

    fn parts(&self) -> Vec<&str> {
        components(&self.path).collect()
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str(self.root.name())
        } else {
            write!(f, "{}\\{}", self.root.name(), self.path)
        }
    }
}

fn components(path: &str) -> impl Iterator<Item = &str> {
    path.split('\\').filter(|part| !part.is_empty())
}

/// One `FROM -> TO` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub from: Key,
    pub to: Key,
    /// Ancestors of `from` on its path open ancestors of `to` (games that
    /// walk down from `HKLM\SOFTWARE`).
    pub walk: bool,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let walk = if self.walk { "walk " } else { "" };
        write!(f, "{walk}{} -> {}", self.from, self.to)
    }
}

/// Where an open goes instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub key: Key,
    /// An ancestor opened for a `walk` rule: the hook remembers the handle
    /// so the next step down can be matched against the rule again.
    pub walk: bool,
}

/// The redirect's rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub rules: Vec<Rule>,
}

/// A rule the parser refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "registry redirect: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Parses rules separated by `;` or new lines (`#` starts a comment).
    /// A source that is a root alone, a `walk` rule whose sides differ in
    /// depth and a rule mapping a key onto itself are errors.
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        let mut rules = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            for item in line.split(';').map(str::trim).filter(|i| !i.is_empty()) {
                rules.push(parse_rule(item)?);
            }
        }
        Ok(Config { rules })
    }

    /// The rules as one line for [`ENV`]; parses back to `self`.
    pub fn env_value(&self) -> String {
        self.rules
            .iter()
            .map(Rule::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// Where an open of `key` goes instead; `None` leaves it alone. The
    /// first rule that matches wins.
    pub fn map(&self, key: &Key) -> Option<Target> {
        let parts = key.parts();
        self.rules.iter().find_map(|rule| {
            if rule.from.root != key.root {
                return None;
            }
            let from = rule.from.parts();
            let to = rule.to.parts();
            let common = from
                .iter()
                .zip(&parts)
                .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                .count();
            if common == from.len() {
                // FROM itself or below it.
                let path = to.iter().chain(&parts[common..]).copied();
                return Some(Target {
                    key: Key::new(rule.to.root, &path.collect::<Vec<_>>().join("\\")),
                    walk: false,
                });
            }
            (rule.walk && common == parts.len() && !parts.is_empty()).then(|| Target {
                key: Key::new(rule.to.root, &to[..common].join("\\")),
                walk: true,
            })
        })
    }
}

fn parse_rule(item: &str) -> Result<Rule, ConfigError> {
    let (walk, item) = match item.split_once(char::is_whitespace) {
        Some((word, rest)) if word.eq_ignore_ascii_case("walk") => (true, rest),
        _ => (false, item),
    };
    let (from, to) = item
        .split_once("->")
        .ok_or_else(|| ConfigError(format!("not FROM -> TO: {item:?}")))?;
    let rule = Rule {
        from: Key::parse(from)?,
        to: Key::parse(to)?,
        walk,
    };
    if rule.from.path.is_empty() {
        return Err(ConfigError(format!(
            "{} is a whole root; name its key",
            rule.from
        )));
    }
    if rule.from.root == rule.to.root && rule.from.path.eq_ignore_ascii_case(&rule.to.path) {
        return Err(ConfigError(format!("{rule} maps a key onto itself")));
    }
    if walk && rule.from.parts().len() != rule.to.parts().len() {
        return Err(ConfigError(format!(
            "a walk rule's sides need the same depth: {rule}"
        )));
    }
    Ok(rule)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMR4: &str = r"HKLM\SOFTWARE\Codemasters\Colin McRae Rally 04 -> HKCU\Software\Codemasters\Colin McRae Rally 04";
    const CMR1: &str =
        r"walk HKLM\SOFTWARE\Codemasters\Rally\1.00 -> HKCU\Software\Codemasters\Rally\1.00";

    fn hklm(path: &str) -> Key {
        Key::new(Root::LocalMachine, path)
    }

    fn hkcu(path: &str) -> Key {
        Key::new(Root::CurrentUser, path)
    }

    fn to(path: &str, walk: bool) -> Option<Target> {
        Some(Target {
            key: hkcu(path),
            walk,
        })
    }

    #[test]
    fn a_full_path_open_goes_to_the_players_hive() {
        let config = Config::parse(CMR4).unwrap();
        // CMR4 and CMR2005: RegCreateKeyExA(HKLM, full path), any case.
        assert_eq!(
            config.map(&hklm(r"SOFTWARE\CODEMASTERS\colin mcrae rally 04")),
            to(r"Software\Codemasters\Colin McRae Rally 04", false)
        );
        // Below it, the rest keeps the game's spelling.
        assert_eq!(
            config.map(&hklm(r"\Software\Codemasters\Colin McRae Rally 04\Video\")),
            to(r"Software\Codemasters\Colin McRae Rally 04\Video", false)
        );
    }

    #[test]
    fn other_keys_are_left_alone() {
        let config = Config::parse(CMR4).unwrap();
        for key in [
            hklm(r"SOFTWARE\Codemasters\Colin McRae Rally 04 Demo"),
            hklm(r"SOFTWARE\Codemasters"),
            hklm("SOFTWARE"),
            hklm(""),
            hklm(r"SOFTWARE\Microsoft\DirectX"),
            hkcu(r"SOFTWARE\Codemasters\Colin McRae Rally 04"),
        ] {
            assert_eq!(config.map(&key), None, "{key}");
        }
    }

    #[test]
    fn a_walk_maps_only_the_rules_path() {
        let config = Config::parse(CMR1).unwrap();
        // Rally.exe: HKLM + "SOFTWARE", then one component at a time.
        assert_eq!(config.map(&hklm("SOFTWARE")), to("Software", true));
        let software = hklm("SOFTWARE");
        let codemasters = software.join("Codemasters");
        assert_eq!(config.map(&codemasters), to(r"Software\Codemasters", true));
        assert_eq!(
            config.map(&codemasters.join(r"Rally")),
            to(r"Software\Codemasters\Rally", true)
        );
        // The key itself: an ordinary match, nothing more to remember.
        assert_eq!(
            config.map(&codemasters.join(r"Rally\1.00")),
            to(r"Software\Codemasters\Rally\1.00", false)
        );
        // Leaving the path: the hook opens the HKLM key as asked.
        assert_eq!(config.map(&software.join(r"Microsoft\DirectX")), None);
        assert_eq!(config.map(&codemasters.join("Colin McRae Rally 2.0")), None);
        assert_eq!(config.map(&hklm("")), None);
        assert_eq!(config.map(&hklm("SYSTEM")), None);
        // Without `walk` the ancestors stay HKLM's.
        let plain = Config::parse(&CMR1[5..]).unwrap();
        assert_eq!(plain.map(&hklm("SOFTWARE")), None);
    }

    #[test]
    fn several_rules_long_roots_and_comments() {
        let text = format!(
            "# CMR4\n{CMR4}\n\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Sega\\SEGA Rally -> HKEY_CURRENT_USER\\Software\\Sega\\SEGA Rally ; {CMR1}  # CMR1\n"
        );
        let config = Config::parse(&text).unwrap();
        assert_eq!(config.rules.len(), 3);
        assert_eq!(
            config.map(&hklm(r"SOFTWARE\Sega\SEGA Rally")),
            to(r"Software\Sega\SEGA Rally", false)
        );
        assert!(config.rules[2].walk);
        assert_eq!(Config::parse("").unwrap(), Config::default());
        assert_eq!(
            Config::parse(" ; \n# nothing\n").unwrap(),
            Config::default()
        );
    }

    #[test]
    fn env_value_parses_back() {
        let config = Config::parse(&format!("{CMR4}\n{CMR1}")).unwrap();
        let value = config.env_value();
        assert_eq!(
            value,
            format!("{CMR4}; {CMR1}"),
            "one line, as the variable holds it"
        );
        assert_eq!(Config::parse(&value).unwrap(), config);
    }

    #[test]
    fn mistakes_are_reported() {
        for text in [
            r"HKLM\SOFTWARE\X",
            r"HKLM\SOFTWARE\X => HKCU\Software\X",
            r"HKXX\SOFTWARE\X -> HKCU\Software\X",
            r"HKLM\SOFTWARE\X -> Software\X",
            r"HKLM -> HKCU",
            r"HKLM\SOFTWARE\X -> hklm\software\x",
            r"walk HKLM\SOFTWARE\X -> HKCU\Software\Y\X",
        ] {
            assert!(Config::parse(text).is_err(), "{text}");
        }
        assert_eq!(
            Config::parse(r"HKLM\SOFTWARE\X").unwrap_err().to_string(),
            "registry redirect: not FROM -> TO: \"HKLM\\\\SOFTWARE\\\\X\""
        );
    }

    #[test]
    fn keys_normalise_and_join() {
        assert_eq!(hklm(r"\SOFTWARE\\X\").path, r"SOFTWARE\X");
        assert_eq!(hklm("").join("SOFTWARE").to_string(), r"HKLM\SOFTWARE");
        assert_eq!(hklm("SOFTWARE").join("").to_string(), r"HKLM\SOFTWARE");
        assert_eq!(Key::parse(" hkcu ").unwrap(), hkcu(""));
    }
}
