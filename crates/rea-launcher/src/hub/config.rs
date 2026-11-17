//! `launcher.cfg` of a hub launcher: the library folders (`library = …`,
//! repeatable) and the launcher's own path fields (a BIOS file, a BIOS
//! folder…), each with its old key names.
//!
//! Read with [`crate::cfg::read_file`] (a BOM or a Windows code page is
//! fine; an unreadable file is an error, never an empty config) and written
//! with [`crate::fsx::write_atomic`], keeping the player's comments and
//! unknown keys: a save re-reads the file and refuses to write over one it
//! can't read.

use std::io;
use std::path::{Path, PathBuf};

use crate::cfg;

/// The `launcher.cfg` key of a library folder.
pub const LIBRARY: &str = "library";

/// What a path field takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Folder,
}

impl PathKind {
    /// Whether `path` is one.
    pub fn accepts(self, path: &Path) -> bool {
        match self {
            PathKind::File => path.is_file(),
            PathKind::Folder => path.is_dir(),
        }
    }

    /// "file" or "folder", for messages.
    pub fn noun(self) -> &'static str {
        match self {
            PathKind::File => "file",
            PathKind::Folder => "folder",
        }
    }
}

/// One path the launcher keeps besides the library folders: its key, its
/// flag, and the words of its setup-page row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathField {
    /// The `launcher.cfg` key: `naomi2_bios`.
    pub key: &'static str,
    /// Old keys still read, never written: `chihiro_bios`.
    pub aliases: &'static [&'static str],
    pub kind: PathKind,
    /// The command-line flag: `--naomi2-bios`.
    pub flag: &'static str,
    /// Old flags still accepted: `--chihiro-bios`.
    pub flag_aliases: &'static [&'static str],
    /// The `--help` line: "where your naomi2.zip is (Ver. 1–3)".
    pub help: &'static str,
    /// The setup page's line above the field.
    pub heading: &'static str,
    /// The field's accessible name: "NAOMI 2 BIOS file".
    pub label: &'static str,
    pub placeholder: &'static str,
    /// The button beside the field: "Use BIOS".
    pub button: &'static str,
    /// Said when the button is pressed on an empty field.
    pub empty: &'static str,
    /// Said once the path is saved: "NAOMI 2 BIOS saved.".
    pub saved: &'static str,
}

impl PathField {
    /// Whether `key` is this field's key or one of its old names.
    pub fn answers_to(&self, key: &str) -> bool {
        self.key == key || self.aliases.contains(&key)
    }
}

/// What `launcher.cfg` remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubConfig {
    /// Folders the games are looked for in.
    pub libraries: Vec<PathBuf>,
    /// One per [`PathField`], in the same order.
    pub paths: Vec<Option<PathBuf>>,
}

impl HubConfig {
    /// An empty config for `fields`.
    pub fn new(fields: &[PathField]) -> HubConfig {
        HubConfig {
            libraries: Vec::new(),
            paths: vec![None; fields.len()],
        }
    }

    /// The config in `text`; unknown keys are left to the file.
    pub fn parse(text: &str, fields: &[PathField]) -> HubConfig {
        let mut c = HubConfig::new(fields);
        for e in cfg::entries(text) {
            if e.key == LIBRARY {
                c.add_library(Path::new(e.value));
            } else if let Some(n) = fields.iter().position(|f| f.answers_to(e.key)) {
                c.paths[n] = Some(e.value.into());
            }
        }
        c
    }

    /// `existing` with this config's keys in place of its old ones (and of
    /// their old names); comments and other keys stay where they were, the
    /// keys follow them. A new file starts with "# Written by the launcher.".
    pub fn render(&self, existing: &str, fields: &[PathField]) -> String {
        let managed = |key: &str| key == LIBRARY || fields.iter().any(|f| f.answers_to(key));
        let (bom, existing) = match existing.strip_prefix(BOM) {
            Some(rest) => (BOM.to_string(), rest),
            None => (String::new(), existing),
        };
        let mut out = bom;
        if existing.trim().is_empty() {
            out.push_str("# Written by the launcher.\n");
        } else {
            out.extend(
                existing
                    .lines()
                    .filter(|line| !line_key(line).is_some_and(managed))
                    .map(|line| format!("{line}\n")),
            );
        }
        for l in &self.libraries {
            out.push_str(&format!("{LIBRARY} = {}\n", l.display()));
        }
        for (f, p) in fields.iter().zip(&self.paths) {
            if let Some(p) = p {
                out.push_str(&format!("{} = {}\n", f.key, p.display()));
            }
        }
        out
    }

    /// The config in `file`; a missing file is an empty config, an
    /// unreadable one an error.
    pub fn load(file: &Path, fields: &[PathField]) -> io::Result<HubConfig> {
        Ok(cfg::read_file(file)?
            .map(|text| HubConfig::parse(&text, fields))
            .unwrap_or_else(|| HubConfig::new(fields)))
    }

    /// Writes this config into `file` (see [`HubConfig::render`]), as a
    /// whole-file swap; an existing file that can't be read is left alone
    /// and the error returned.
    pub fn save(&self, file: &Path, fields: &[PathField]) -> io::Result<()> {
        let existing = cfg::read_file(file)?.unwrap_or_default();
        crate::fsx::write_atomic(file, self.render(&existing, fields))
    }

    pub fn add_library(&mut self, dir: &Path) {
        if !self.libraries.iter().any(|l| l == dir) {
            self.libraries.push(dir.to_path_buf());
        }
    }
}

const BOM: char = '\u{feff}';

/// The key of an active `key = value` line, set or blank.
fn line_key(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.starts_with(['#', ';']) {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

/// A typed path: trimmed, with `~`, `~/` or `~\` for the home folder
/// (`HOME`, else `USERPROFILE`, as on Windows).
pub fn expand_home(typed: &str) -> PathBuf {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    expand_home_in(typed, home.as_deref())
}

/// [`expand_home`] with the home folder given (`None`: unknown, `~` stays).
pub fn expand_home_in(typed: &str, home: Option<&Path>) -> PathBuf {
    let typed = typed.trim();
    let rest = typed.strip_prefix("~/").or(typed.strip_prefix("~\\"));
    match (home, rest) {
        (Some(h), Some(rest)) => h.join(rest),
        (Some(h), None) if typed == "~" => h.to_path_buf(),
        _ => PathBuf::from(typed),
    }
}

/// The absolute path a player typed for a `kind`, or the sentence to show:
/// `empty` when nothing was typed, else that it isn't a file/folder this
/// computer can open.
pub fn typed_path(typed: &str, kind: PathKind, empty: &str) -> Result<PathBuf, String> {
    if typed.trim().is_empty() {
        return Err(empty.to_string());
    }
    let path = expand_home(typed);
    if !kind.accepts(&path) {
        return Err(format!(
            "{} isn't a {} this computer can open. Check the path and try again.",
            super::display_path(&path),
            kind.noun()
        ));
    }
    Ok(std::path::absolute(&path).unwrap_or(path))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const BIOS: PathField = PathField {
        key: "bios",
        aliases: &["chihiro_bios"],
        kind: PathKind::Folder,
        flag: "--bios",
        flag_aliases: &["--chihiro-bios"],
        help: "your BIOS folder",
        heading: "BIOS folder, needed for some games",
        label: "BIOS folder",
        placeholder: "/path/to/bios",
        button: "Use folder",
        empty: "Type the path of your BIOS folder first.",
        saved: "BIOS folder saved.",
    };
    const FIELDS: [PathField; 1] = [BIOS];

    #[test]
    fn several_libraries_and_a_path_round_trip_as_before() {
        let mut c = HubConfig::new(&FIELDS);
        c.add_library(Path::new("/roms/TeknoParrot"));
        c.add_library(Path::new("/roms/naomi2"));
        c.add_library(Path::new("/roms/naomi2"));
        c.paths[0] = Some("/bios".into());
        let text = c.render("", &FIELDS);
        assert_eq!(
            text,
            "# Written by the launcher.\nlibrary = /roms/TeknoParrot\nlibrary = /roms/naomi2\nbios = /bios\n",
            "the 0.1 launchers' exact file"
        );
        assert_eq!(HubConfig::parse(&text, &FIELDS), c);
        assert_eq!(c.render(&text, &FIELDS), text, "a save changes nothing");
    }

    #[test]
    fn a_library_listed_twice_is_saved_once() {
        let text = "library = /a\nlibrary = /b\nlibrary = /a\n";
        let c = HubConfig::parse(text, &FIELDS);
        assert_eq!(c.libraries, [PathBuf::from("/a"), PathBuf::from("/b")]);
        assert_eq!(c.render(text, &FIELDS), "library = /a\nlibrary = /b\n");
    }

    #[test]
    fn an_old_key_is_read_and_saved_under_the_new_name() {
        let c = HubConfig::parse("chihiro_bios = /old\n", &FIELDS);
        assert_eq!(c.paths[0], Some(PathBuf::from("/old")));
        assert_eq!(c.render("chihiro_bios = /old\n", &FIELDS), "bios = /old\n");
    }

    #[test]
    fn comments_and_unknown_keys_stay_and_a_bom_is_no_problem() {
        let text = "\u{feff}library = /a\n# mine\nfuture = 1\n";
        let mut c = HubConfig::parse(text, &FIELDS);
        assert_eq!(c.libraries, [PathBuf::from("/a")], "the BOM hides no key");
        c.add_library(Path::new("/b"));
        assert_eq!(
            c.render(text, &FIELDS),
            "\u{feff}# mine\nfuture = 1\nlibrary = /a\nlibrary = /b\n",
            "no duplicate first key, the BOM kept"
        );
    }

    #[test]
    fn an_unreadable_file_is_an_error_and_is_not_overwritten() {
        let dir = crate::test_dir("hub-config");
        let file = dir.join("launcher.cfg");
        assert_eq!(
            HubConfig::load(&file, &FIELDS).unwrap(),
            HubConfig::new(&FIELDS)
        );
        // a folder where the file goes: it can't be read
        std::fs::create_dir_all(&file).unwrap();
        assert!(HubConfig::load(&file, &FIELDS).is_err());
        assert!(HubConfig::new(&FIELDS).save(&file, &FIELDS).is_err());
        assert!(file.is_dir(), "left alone");
        std::fs::remove_dir(&file).unwrap();
        // Latin-1 text keeps its letters
        std::fs::write(&file, b"library = /jogos/A\xe7\xe3o\n").unwrap();
        let c = HubConfig::load(&file, &FIELDS).unwrap();
        assert_eq!(c.libraries, [PathBuf::from("/jogos/Ação")]);
        c.save(&file, &FIELDS).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "library = /jogos/Ação\n"
        );
    }

    #[test]
    fn a_typed_home_path_is_expanded() {
        let home = Path::new("/home/p");
        assert_eq!(expand_home_in("  /a/b ", Some(home)), PathBuf::from("/a/b"));
        assert_eq!(expand_home_in("~/x", Some(home)), home.join("x"));
        assert_eq!(expand_home_in("~\\x", Some(home)), home.join("x"));
        assert_eq!(expand_home_in(" ~ ", Some(home)), home);
        assert_eq!(expand_home_in("~/x", None), PathBuf::from("~/x"));
        assert_eq!(expand_home_in("~x", Some(home)), PathBuf::from("~x"));
    }

    #[test]
    fn a_typed_path_must_be_the_right_kind() {
        let dir = crate::test_dir("hub-typed");
        let file = dir.join("naomi2.zip");
        std::fs::write(&file, b"zip").unwrap();
        assert_eq!(
            typed_path(" ", PathKind::File, "Type it first."),
            Err("Type it first.".into())
        );
        assert_eq!(
            typed_path(&file.display().to_string(), PathKind::File, ""),
            Ok(file.clone())
        );
        let e = typed_path(&dir.display().to_string(), PathKind::File, "").unwrap_err();
        assert!(e.contains("isn't a file this computer can open"), "{e}");
        let e = typed_path(&file.display().to_string(), PathKind::Folder, "").unwrap_err();
        assert!(e.contains("isn't a folder this computer can open"), "{e}");
        assert_eq!(
            typed_path(&dir.display().to_string(), PathKind::Folder, ""),
            Ok(dir.clone())
        );
    }
}
