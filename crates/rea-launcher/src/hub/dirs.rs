//! Where a hub launcher keeps things.
//!
//! - data (`$<PREFIX>_DATA`, else `$XDG_DATA_HOME/<name>` or
//!   `~/.local/share/<name>`; on Windows `%LOCALAPPDATA%\<name>`):
//!   `launcher.cfg`, `settings/<slug>.cfg`, `runtimes/<slug>/`,
//!   `launcher.log`;
//! - components (`$<PREFIX>_COMPONENTS`, else `components/` beside the
//!   program): the launcher's bundled tools and `art/`.
//!
//! An empty variable counts as unset. `XDG_DATA_HOME` and `LOCALAPPDATA`
//! must be absolute (a relative one is ignored, as the XDG spec says);
//! a relative `$<PREFIX>_DATA` is made absolute against the folder the
//! launcher started in, so it can't move when something changes folder.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The names [`HubPaths::discover`] uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubDirs {
    /// `INITIALD`: `$INITIALD_DATA`, `$INITIALD_COMPONENTS`.
    pub env_prefix: &'static str,
    /// `rea-initiald`: the data folder's name.
    pub name: &'static str,
    /// A development build's own assets, used when the program sits beside
    /// no `components/<dir>`: pass `cfg!(debug_assertions).then_some(
    /// concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))`.
    pub dev_assets: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubPaths {
    pub data: PathBuf,
    pub components: PathBuf,
    /// See [`HubDirs::dev_assets`].
    pub dev_assets: Option<PathBuf>,
}

impl HubPaths {
    /// The folders for this process: its environment and executable.
    pub fn discover(dirs: &HubDirs) -> HubPaths {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf));
        HubPaths::discover_in(dirs, &|k| std::env::var_os(k), exe_dir.as_deref())
    }

    /// [`HubPaths::discover`] with the environment (`env`) and the
    /// program's folder given.
    pub fn discover_in(
        dirs: &HubDirs,
        env: &dyn Fn(&str) -> Option<OsString>,
        exe_dir: Option<&Path>,
    ) -> HubPaths {
        let var = |k: &str| env(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let absolute = |p: PathBuf| std::path::absolute(&p).unwrap_or(p);
        let data = var(&format!("{}_DATA", dirs.env_prefix))
            .map(absolute)
            .unwrap_or_else(|| default_data(cfg!(windows), dirs.name, &var));
        let components = var(&format!("{}_COMPONENTS", dirs.env_prefix))
            .map(absolute)
            .unwrap_or_else(|| exe_dir.unwrap_or(Path::new(".")).join("components"));
        HubPaths {
            data,
            components,
            dev_assets: dirs.dev_assets.map(PathBuf::from),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.data.join("launcher.cfg")
    }

    /// The player's pad layout ([`super::bindings::Bindings`]).
    pub fn controls_file(&self) -> PathBuf {
        self.data.join("controls.cfg")
    }
    /// A game's settings file.
    pub fn settings(&self, slug: &str) -> PathBuf {
        self.data.join("settings").join(format!("{slug}.cfg"))
    }
    /// A game's runtime folder (its saves included).
    pub fn runtime(&self, slug: &str) -> PathBuf {
        self.data.join("runtimes").join(slug)
    }
    pub fn log(&self) -> PathBuf {
        self.data.join("launcher.log")
    }
    /// A picture of the hub (`<slug>.jpg`, `<slug>-tile.jpg`).
    pub fn art(&self, name: &str) -> PathBuf {
        self.asset("art").join(name)
    }

    /// `components/<dir>`, else the development build's `<dev_assets>/<dir>`
    /// when that exists.
    pub fn asset(&self, dir: &str) -> PathBuf {
        let packaged = self.components.join(dir);
        match &self.dev_assets {
            Some(dev) if !packaged.is_dir() && dev.join(dir).is_dir() => dev.join(dir),
            _ => packaged,
        }
    }
}

fn default_data(windows: bool, name: &str, var: &dyn Fn(&str) -> Option<PathBuf>) -> PathBuf {
    let abs = |k: &str| var(k).filter(|p| p.is_absolute());
    let home =
        || var(if windows { "USERPROFILE" } else { "HOME" }).unwrap_or_else(std::env::temp_dir);
    let base = if windows {
        abs("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData").join("Local"))
    } else {
        abs("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local").join("share"))
    };
    base.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIRS: HubDirs = HubDirs {
        env_prefix: "ID",
        name: "rea-id",
        dev_assets: None,
    };

    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: Vec<(String, OsString)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |k| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    }

    fn data(vars: &[(&str, &str)]) -> PathBuf {
        HubPaths::discover_in(&DIRS, &env_of(vars), Some(Path::new("/opt/id"))).data
    }

    #[test]
    #[cfg(not(windows))]
    fn the_data_folder_follows_xdg_and_ignores_empty_or_relative_values() {
        assert_eq!(
            data(&[("HOME", "/home/p")]),
            Path::new("/home/p/.local/share/rea-id")
        );
        assert_eq!(
            data(&[("HOME", "/home/p"), ("XDG_DATA_HOME", "/xdg")]),
            Path::new("/xdg/rea-id")
        );
        for bad in ["", "relative/share"] {
            assert_eq!(
                data(&[("HOME", "/home/p"), ("XDG_DATA_HOME", bad)]),
                Path::new("/home/p/.local/share/rea-id"),
                "{bad:?}"
            );
        }
        assert_eq!(
            data(&[("HOME", "/home/p"), ("ID_DATA", "")]),
            Path::new("/home/p/.local/share/rea-id")
        );
        assert_eq!(data(&[("ID_DATA", "/d")]), Path::new("/d"));
    }

    #[test]
    #[cfg(windows)]
    fn the_data_folder_is_in_local_app_data_and_ignores_empty_or_relative_values() {
        assert_eq!(
            data(&[("LOCALAPPDATA", "C:\\Users\\p\\AppData\\Local")]),
            Path::new("C:\\Users\\p\\AppData\\Local\\rea-id")
        );
        for bad in ["", "AppData"] {
            assert_eq!(
                data(&[("USERPROFILE", "C:\\Users\\p"), ("LOCALAPPDATA", bad)]),
                Path::new("C:\\Users\\p\\AppData\\Local\\rea-id"),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn a_relative_override_is_made_absolute() {
        let d = data(&[("ID_DATA", "here")]);
        assert!(d.is_absolute() && d.ends_with("here"), "{}", d.display());
    }

    #[test]
    fn components_sit_beside_the_program_unless_set() {
        let env = env_of(&[]);
        let p = HubPaths::discover_in(&DIRS, &env, Some(Path::new("/opt/id")));
        assert_eq!(p.components, Path::new("/opt/id/components"));
        let env = env_of(&[("ID_COMPONENTS", "/c")]);
        let p = HubPaths::discover_in(&DIRS, &env, Some(Path::new("/opt/id")));
        let c = std::path::absolute("/c").unwrap();
        assert_eq!(p.components, c);
        assert_eq!(p.art("id1.jpg"), c.join("art").join("id1.jpg"));
    }

    #[test]
    fn a_development_build_finds_the_repositorys_art() {
        let t = crate::test_dir("hub-dirs");
        let dev = t.join("assets");
        std::fs::create_dir_all(dev.join("art")).unwrap();
        let mut p = HubPaths {
            data: t.join("data"),
            components: t.join("pkg/components"),
            dev_assets: Some(dev.clone()),
        };
        assert_eq!(p.art("x.jpg"), dev.join("art/x.jpg"));
        std::fs::create_dir_all(t.join("pkg/components/art")).unwrap();
        assert_eq!(
            p.art("x.jpg"),
            t.join("pkg/components/art/x.jpg"),
            "a package wins"
        );
        p.dev_assets = None;
        assert_eq!(p.asset("shaders"), t.join("pkg/components/shaders"));
        assert_eq!(p.settings("id6"), t.join("data/settings/id6.cfg"));
        assert_eq!(p.runtime("id6"), t.join("data/runtimes/id6"));
        assert_eq!(p.log(), t.join("data/launcher.log"));
        assert_eq!(p.config_file(), t.join("data/launcher.cfg"));
    }
}
