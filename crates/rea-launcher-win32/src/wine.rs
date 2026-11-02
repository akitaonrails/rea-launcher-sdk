//! Wine detection, Wine's bus-driver settings and per-program DLL
//! overrides.
//!
//! `is_wine` is the version all launchers converged on (CMR4/OR): the
//! `wine_get_version` export, plus the environment Wine, Proton and the
//! play helpers set (`rea_launcher::wine::wine_env`), for Wine builds that
//! hide the export. Seeding the winebus key only where values are absent is
//! OR's (done in the launcher so it also works under plain Proton); the
//! values are game knowledge (`rea_launcher::wine` has the names).
//!
//! **DLL overrides for the game: use `app_dll_overrides`.** Wine's loader
//! in a child started with `CreateProcessW` ignores a `WINEDLLOVERRIDES`
//! set in the child's environment block (`process::Launch::env`): it reads
//! the Unix environment it inherited (set by `<id>-play`, the AppImage or
//! Proton) and the registry. So a launcher started without our shell
//! helper (plain Proton/Steam) cannot steer which copy of a DLL the game
//! loads through the block; SRR found its bundled d3dx9_35 not loading.
//! The per-program key
//! `HKCU\Software\Wine\AppDefaults\<exe>\DllOverrides` is honoured,
//! inside whatever prefix the launcher runs in.
//!
//! Measured under Wine 11.21 (`tests::env_block_overrides_do_not_reach_wine`,
//! a probe program with a native `d3dx9_35.dll` next to it): by default the
//! program folder's copy loads; `WINEDLLOVERRIDES=d3dx9_35=b` in the
//! environment block changes nothing; the AppDefaults key set to `builtin`
//! loads Wine's.

/// How Wine should load one DLL for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DllMode {
    /// The game's own copy only (`native`).
    Native,
    /// Wine's only (`builtin`).
    Builtin,
    /// The game's copy, else Wine's (`native,builtin`; what bundled
    /// components such as DXVK and dgVoodoo need).
    NativeThenBuiltin,
    /// Wine's, else the game's (`builtin,native`).
    BuiltinThenNative,
    /// Never loaded (empty value).
    Disabled,
}

impl DllMode {
    /// The registry value Wine reads.
    pub fn value(self) -> &'static str {
        match self {
            DllMode::Native => "native",
            DllMode::Builtin => "builtin",
            DllMode::NativeThenBuiltin => "native,builtin",
            DllMode::BuiltinThenNative => "builtin,native",
            DllMode::Disabled => "",
        }
    }

    /// The mode a value says, in any of Wine's spellings (`n,b`,
    /// `Native, Builtin`); `None` for anything else.
    pub fn parse(value: &str) -> Option<DllMode> {
        let words: Vec<String> = value
            .split(',')
            .map(|w| w.trim().to_ascii_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        let word = |w: &str| match w {
            "n" | "native" => Some('n'),
            "b" | "builtin" => Some('b'),
            _ => None,
        };
        let letters: Option<String> = words.iter().map(|w| word(w)).collect();
        match letters?.as_str() {
            "" => Some(DllMode::Disabled),
            "n" => Some(DllMode::Native),
            "b" => Some(DllMode::Builtin),
            "nb" => Some(DllMode::NativeThenBuiltin),
            "bn" => Some(DllMode::BuiltinThenNative),
            _ => None,
        }
    }
}

/// The per-program key for `exe` (a file name or a path; Wine matches the
/// file name without case).
pub fn app_defaults_key(exe: &str) -> String {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    format!(r"Software\Wine\AppDefaults\{name}\DllOverrides")
}

/// What `app_dll_overrides` does with one DLL, given the value there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Absent: write ours.
    Write,
    /// Already ours (the same mode): nothing to do.
    Ours,
    /// Another value: the player's choice, kept.
    Player,
}

/// The [`Plan`] for one DLL whose current value is `existing`.
pub fn plan(existing: Option<&str>, ours: DllMode) -> Plan {
    match existing {
        None => Plan::Write,
        Some(value) if DllMode::parse(value) == Some(ours) => Plan::Ours,
        Some(_) => Plan::Player,
    }
}

/// What `app_dll_overrides` found and did, by DLL name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// Written now.
    pub written: Vec<String>,
    /// Already set to our mode.
    pub ours: Vec<String>,
    /// Set to something else by the player, and kept: `(dll, value)`.
    pub player: Vec<(String, String)>,
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use super::{Applied, DllMode, Plan, app_defaults_key, plan};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    use crate::reg::{Error, Hive, Key};
    use crate::wide::wide;

    /// True under Wine or Proton.
    pub fn is_wine() -> bool {
        // SAFETY: read-only loader queries with NUL-terminated names.
        let exported = unsafe {
            let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
            !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
        };
        exported || rea_launcher::wine::is_wine_env()
    }

    /// Seeds the winebus values the prefix lacks; a value already there
    /// (the player's choice) is kept. True when anything was written.
    pub fn seed_winebus(values: &[(&str, u32)]) -> Result<bool, Error> {
        Key::create(Hive::LocalMachine, rea_launcher::wine::WINEBUS_KEY)?.seed_dwords(values)
    }

    /// Sets Wine's per-program DLL overrides for `exe` (its file name)
    /// under Wine: each DLL without a value gets ours; a value already
    /// there is kept (the player's, or ours from an earlier run). Call it
    /// before starting the game, every time. Does nothing (and returns an
    /// empty [`Applied`]) on native Windows.
    pub fn app_dll_overrides(exe: &str, dlls: &[(&str, DllMode)]) -> Result<Applied, Error> {
        let mut applied = Applied::default();
        if !is_wine() || dlls.is_empty() {
            return Ok(applied);
        }
        let key = Key::create(Hive::CurrentUser, &app_defaults_key(exe))?;
        for &(dll, mode) in dlls {
            let existing = key.string(dll)?;
            match plan(existing.as_deref(), mode) {
                Plan::Write => {
                    key.set_string(dll, mode.value())?;
                    applied.written.push(dll.to_owned());
                }
                Plan::Ours => applied.ours.push(dll.to_owned()),
                Plan::Player => applied
                    .player
                    .push((dll.to_owned(), existing.unwrap_or_default())),
            }
        }
        Ok(applied)
    }

    /// Removes the overrides [`app_dll_overrides`] wrote for `exe` (a
    /// component turned off): only values still equal to our mode go; the
    /// player's are kept. Returns the DLLs removed. Native Windows: nothing.
    pub fn remove_app_dll_overrides(
        exe: &str,
        dlls: &[(&str, DllMode)],
    ) -> Result<Vec<String>, Error> {
        let mut removed = Vec::new();
        if !is_wine() {
            return Ok(removed);
        }
        let Some(key) = Key::open_read(Hive::CurrentUser, &app_defaults_key(exe))? else {
            return Ok(removed);
        };
        drop(key);
        let key = Key::create(Hive::CurrentUser, &app_defaults_key(exe))?;
        for &(dll, mode) in dlls {
            if plan(key.string(dll)?.as_deref(), mode) == Plan::Ours && key.delete_value(dll)? {
                removed.push(dll.to_owned());
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_read_and_write_wines_spellings() {
        for mode in [
            DllMode::Native,
            DllMode::Builtin,
            DllMode::NativeThenBuiltin,
            DllMode::BuiltinThenNative,
            DllMode::Disabled,
        ] {
            assert_eq!(DllMode::parse(mode.value()), Some(mode));
        }
        assert_eq!(DllMode::parse("n,b"), Some(DllMode::NativeThenBuiltin));
        assert_eq!(
            DllMode::parse(" Native , Builtin "),
            Some(DllMode::NativeThenBuiltin)
        );
        assert_eq!(DllMode::parse("B"), Some(DllMode::Builtin));
        assert_eq!(DllMode::parse("native,native"), None);
        assert_eq!(DllMode::parse("disabled"), None);
    }

    #[test]
    fn the_key_is_per_program_file_name() {
        assert_eq!(
            app_defaults_key(r"C:\Games\SRR\SEGA Rally.exe"),
            r"Software\Wine\AppDefaults\SEGA Rally.exe\DllOverrides"
        );
        assert_eq!(
            app_defaults_key("cmr4.exe"),
            r"Software\Wine\AppDefaults\cmr4.exe\DllOverrides"
        );
    }

    #[test]
    fn the_players_values_are_kept() {
        assert_eq!(plan(None, DllMode::Native), Plan::Write);
        assert_eq!(plan(Some("n"), DllMode::Native), Plan::Ours);
        assert_eq!(
            plan(Some("native,builtin"), DllMode::NativeThenBuiltin),
            Plan::Ours
        );
        assert_eq!(plan(Some("builtin"), DllMode::Native), Plan::Player);
        assert_eq!(plan(Some(""), DllMode::Native), Plan::Player);
        assert_eq!(plan(Some("garbage"), DllMode::Native), Plan::Player);
    }

    /// Windows/Wine: a game-like program loading `d3dx9_35.dll` from its
    /// folder, where a native stand-in sits (`examples/dll_probe.rs`,
    /// `examples/d3dx9_35.rs`); its exit code says which copy loaded.
    #[cfg(windows)]
    mod probe {
        use std::path::PathBuf;

        use crate::process::Launch;

        pub const NATIVE: u32 = 10;
        pub const BUILTIN: u32 = 11;

        /// A private copy of the probe as `<name>.exe`, with the stand-in.
        pub fn install(name: &str) -> PathBuf {
            let build = std::env::current_exe().unwrap();
            let examples = build.parent().unwrap().parent().unwrap().join("examples");
            let dir = examples.join(format!("dll-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(examples.join("d3dx9_35.dll"), dir.join("d3dx9_35.dll")).unwrap();
            let exe = dir.join(format!("{name}.exe"));
            std::fs::copy(examples.join("dll_probe.exe"), &exe).unwrap();
            exe
        }

        pub fn run(exe: &PathBuf, env: Option<&str>) -> u32 {
            let mut launch = Launch::new(exe).cwd(exe.parent().unwrap());
            if let Some(value) = env {
                launch = launch.env("WINEDLLOVERRIDES", value);
            }
            launch.run().unwrap()
        }
    }

    /// Which loader settings Wine honours for a child started with
    /// `CreateProcessW`: the default (the native copy in the program's
    /// folder wins), `WINEDLLOVERRIDES` in the child's environment block
    /// asking for the builtin, and the per-program registry key asking for
    /// the builtin. Prints the three results (the findings).
    #[cfg(windows)]
    #[test]
    fn env_block_overrides_do_not_reach_wine() {
        if !is_wine() {
            return;
        }
        let name = format!("probe_env_{}", std::process::id());
        let exe = probe::install(&name);
        let default = probe::run(&exe, None);
        let env_block = probe::run(&exe, Some("d3dx9_35=b"));
        let key = crate::reg::Key::create(
            crate::reg::Hive::CurrentUser,
            &app_defaults_key(&format!("{name}.exe")),
        )
        .unwrap();
        key.set_string("d3dx9_35", "builtin").unwrap();
        let registry = probe::run(&exe, None);
        key.delete_value("d3dx9_35").unwrap();
        println!("dll probe: default {default}, env block {env_block}, registry {registry}");
        assert_eq!(
            default,
            probe::NATIVE,
            "the program folder's copy by default"
        );
        assert_eq!(registry, probe::BUILTIN, "the registry key is honoured");
        assert_eq!(
            env_block,
            probe::NATIVE,
            "WINEDLLOVERRIDES in the environment block is ignored"
        );
        let _ = std::fs::remove_dir_all(exe.parent().unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn app_dll_overrides_take_effect_and_keep_the_players() {
        if !is_wine() {
            return;
        }
        let name = format!("probe_reg_{}", std::process::id());
        let exe = probe::install(&name);
        let file = format!("{name}.exe");
        // The opposite of Wine's default here (the program folder's copy),
        // so the probe shows the override took effect.
        let ours = [("d3dx9_35", DllMode::Builtin)];

        let applied = app_dll_overrides(&file, &ours).unwrap();
        assert_eq!(applied.written, ["d3dx9_35"]);
        assert_eq!(probe::run(&exe, None), probe::BUILTIN, "the override works");
        // Again: already ours, nothing written.
        assert_eq!(app_dll_overrides(&file, &ours).unwrap().ours, ["d3dx9_35"]);

        // Turned off: our value goes, the default is back.
        assert_eq!(
            remove_app_dll_overrides(&file, &ours).unwrap(),
            ["d3dx9_35"]
        );
        assert_eq!(probe::run(&exe, None), probe::NATIVE);

        // The player's own value is never replaced nor removed.
        let key = crate::reg::Key::create(crate::reg::Hive::CurrentUser, &app_defaults_key(&file))
            .unwrap();
        key.set_string("d3dx9_35", "n,b").unwrap();
        let applied = app_dll_overrides(&file, &ours).unwrap();
        assert_eq!(applied.player, [("d3dx9_35".to_owned(), "n,b".to_owned())]);
        assert!(remove_app_dll_overrides(&file, &ours).unwrap().is_empty());
        assert_eq!(key.string("d3dx9_35").unwrap().as_deref(), Some("n,b"));
        key.delete_value("d3dx9_35").unwrap();
        let _ = std::fs::remove_dir_all(exe.parent().unwrap());
    }
}
