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
//!
//! **Microsoft's DLLs are replaced by Wine's unless overridden.** The
//! stand-in above has no version resource, so its copy wins. A DLL whose
//! version resource names Microsoft as the vendor (`d3dx9_*`,
//! `d3dcompiler_*`, `xinput1_3`, `msvcp140`, `vcruntime140`, `xaudio2_*`…)
//! is replaced by Wine's builtin even from the program folder
//! (`WINEDEBUG=+module`: `version_heuristics got default vendor "Microsoft
//! Corporation"`, then `find_builtin_dll`; Sega Rally Revo's `d3dx9_35`).
//! So every bundled Microsoft redistributable needs `app_dll_overrides`
//! (`native` or `native,builtin`). Others' DLLs (DXVK, dgVoodoo, Xidi)
//! win by default; override them too when the launcher relies on them.
//!
//! **Only what we wrote is ever removed.** `app_dll_overrides` records
//! each value it writes under [`owned_key`] (same prefix, exact text);
//! `remove_app_dll_overrides` deletes a value only while it still equals
//! that record, so a player's value is kept even when it equals ours.

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

/// Where the launcher records the overrides it wrote for `exe`:
/// `HKCU\Software\rea-launcher\DllOverrides\<exe file name>`, one string
/// value per DLL holding the exact value written. It lives in the same
/// prefix as the overrides, so it travels with them.
pub fn owned_key(exe: &str) -> String {
    let name = exe.rsplit(['\\', '/']).next().unwrap_or(exe);
    format!(r"Software\rea-launcher\DllOverrides\{name}")
}

/// Whether two registry values are the same text (Wine reads them
/// without case; surrounding spaces don't count).
fn same_value(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// What `app_dll_overrides` does with one DLL, from the value there
/// (`existing`), what the launcher recorded writing (`recorded`, see
/// [`owned_key`]) and the mode it wants now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Absent: write ours and record it.
    Write,
    /// Ours (recorded, unchanged) and already the mode we want.
    Ours,
    /// Ours (recorded, unchanged) in an older mode: rewrite and record.
    Update,
    /// Equal to our mode but not written by us (the player's, or written
    /// before the SDK recorded ownership): kept, and never removed.
    Matching,
    /// Another value: the player's choice, kept. A record of ours for it
    /// is dropped, so the value is never removed later.
    Player,
}

/// The [`Step`] for one DLL.
pub fn step(existing: Option<&str>, recorded: Option<&str>, ours: DllMode) -> Step {
    let Some(value) = existing else {
        return Step::Write;
    };
    let owned = recorded.is_some_and(|recorded| same_value(recorded, value));
    let our_mode = DllMode::parse(value) == Some(ours);
    match (owned, our_mode) {
        (true, true) => Step::Ours,
        (true, false) => Step::Update,
        (false, true) => Step::Matching,
        (false, false) => Step::Player,
    }
}

/// Whether removing our override for one DLL deletes the value: only when
/// the value there is exactly what the launcher recorded writing.
pub fn removable(existing: Option<&str>, recorded: Option<&str>) -> bool {
    matches!((existing, recorded), (Some(value), Some(recorded)) if same_value(value, recorded))
}

/// What `app_dll_overrides` (before v0.4.0) did with one DLL, given the
/// value there. It can't tell the player's value from ours when they are
/// equal; `app_dll_overrides` now uses [`step`].
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
    /// Written now (absent before, or ours in an older mode).
    pub written: Vec<String>,
    /// Ours from an earlier run, already in our mode.
    pub ours: Vec<String>,
    /// Already our mode, but not written by this launcher (the player's,
    /// or a launcher built on SDK v0.3.x): kept, never removed.
    pub matching: Vec<String>,
    /// Set to something else by the player, and kept: `(dll, value)`.
    pub player: Vec<(String, String)>,
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use super::{Applied, DllMode, Step, app_defaults_key, owned_key, removable, step};

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
    /// under Wine: each DLL without a value gets ours, and the launcher
    /// records what it wrote ([`owned_key`](super::owned_key)); a value it
    /// didn't write is kept (the player's). Call it before starting the
    /// game, every time. Does nothing (and returns an empty [`Applied`])
    /// on native Windows.
    pub fn app_dll_overrides(exe: &str, dlls: &[(&str, DllMode)]) -> Result<Applied, Error> {
        let mut applied = Applied::default();
        if !is_wine() || dlls.is_empty() {
            return Ok(applied);
        }
        let key = Key::create(Hive::CurrentUser, &app_defaults_key(exe))?;
        let owned = Key::create(Hive::CurrentUser, &owned_key(exe))?;
        for &(dll, mode) in dlls {
            let existing = key.string(dll)?;
            let recorded = owned.string(dll)?;
            match step(existing.as_deref(), recorded.as_deref(), mode) {
                Step::Write | Step::Update => {
                    // Record first: a value of ours is never left
                    // unrecorded (it would read as the player's).
                    owned.set_string(dll, mode.value())?;
                    key.set_string(dll, mode.value())?;
                    applied.written.push(dll.to_owned());
                }
                Step::Ours => applied.ours.push(dll.to_owned()),
                Step::Matching => {
                    forget(&owned, dll, recorded.is_some())?;
                    applied.matching.push(dll.to_owned());
                }
                Step::Player => {
                    forget(&owned, dll, recorded.is_some())?;
                    applied
                        .player
                        .push((dll.to_owned(), existing.unwrap_or_default()));
                }
            }
        }
        Ok(applied)
    }

    /// Drops the record of a value the player has changed since.
    fn forget(owned: &Key, dll: &str, recorded: bool) -> Result<(), Error> {
        if recorded {
            owned.delete_value(dll)?;
        }
        Ok(())
    }

    /// Removes the overrides [`app_dll_overrides`] wrote for `exe` (a
    /// component turned off): only values still exactly what it recorded
    /// writing go; the player's are kept, also when equal to ours, and so
    /// are values written before v0.4.0 (no record). `dlls`' modes are not
    /// needed any more (the record says what was written); the list says
    /// which DLLs to remove. Returns the DLLs removed. Native Windows:
    /// nothing.
    pub fn remove_app_dll_overrides(
        exe: &str,
        dlls: &[(&str, DllMode)],
    ) -> Result<Vec<String>, Error> {
        let mut removed = Vec::new();
        if !is_wine() {
            return Ok(removed);
        }
        let Some(owned) = Key::open_read(Hive::CurrentUser, &owned_key(exe))? else {
            return Ok(removed);
        };
        drop(owned);
        let owned = Key::create(Hive::CurrentUser, &owned_key(exe))?;
        let key = Key::create(Hive::CurrentUser, &app_defaults_key(exe))?;
        for &(dll, _) in dlls {
            let Some(recorded) = owned.string(dll)? else {
                continue;
            };
            if removable(key.string(dll)?.as_deref(), Some(&recorded)) && key.delete_value(dll)? {
                removed.push(dll.to_owned());
            }
            owned.delete_value(dll)?;
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

    #[test]
    fn the_record_says_which_values_are_ours() {
        assert_eq!(
            owned_key(r"C:\Games\SRR\SEGA Rally.exe"),
            r"Software\rea-launcher\DllOverrides\SEGA Rally.exe"
        );
        let nb = DllMode::NativeThenBuiltin;
        // Absent: write it (also when a stale record says we did).
        assert_eq!(step(None, None, nb), Step::Write);
        assert_eq!(step(None, Some("native,builtin"), nb), Step::Write);
        // Ours from an earlier run.
        assert_eq!(
            step(Some("native,builtin"), Some("native,builtin"), nb),
            Step::Ours
        );
        assert_eq!(
            step(Some("Native,Builtin "), Some("native,builtin"), nb),
            Step::Ours
        );
        // Ours in the mode an older launcher wanted: updated.
        assert_eq!(step(Some("native"), Some("native"), nb), Step::Update);
        // The player's value that equals ours (or one SDK v0.3.x wrote
        // without a record): kept, never ours.
        assert_eq!(step(Some("native,builtin"), None, nb), Step::Matching);
        assert_eq!(
            step(Some("n,b"), Some("native,builtin"), nb),
            Step::Matching
        );
        // The player's other value, also over our record.
        assert_eq!(step(Some("builtin"), None, nb), Step::Player);
        assert_eq!(
            step(Some("builtin"), Some("native,builtin"), nb),
            Step::Player
        );
        assert_eq!(step(Some(""), Some("native,builtin"), nb), Step::Player);
    }

    #[test]
    fn only_a_value_we_recorded_is_ever_removed() {
        assert!(removable(Some("native,builtin"), Some("native,builtin")));
        assert!(removable(Some("NATIVE,BUILTIN"), Some("native,builtin")));
        // The player's value equal to ours, with no record: kept.
        assert!(!removable(Some("native,builtin"), None));
        // Changed since we wrote it: the player's now.
        assert!(!removable(Some("n,b"), Some("native,builtin")));
        assert!(!removable(Some("builtin"), Some("native,builtin")));
        assert!(!removable(None, Some("native,builtin")));
        assert!(!removable(None, None));
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
            let probe = rea_launcher::testing::example("dll_probe.exe");
            let examples = probe.parent().unwrap();
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

        // Nor when it equals ours: the player set `builtin` themselves.
        key.set_string("d3dx9_35", "builtin").unwrap();
        let applied = app_dll_overrides(&file, &ours).unwrap();
        assert_eq!(applied.matching, ["d3dx9_35"]);
        assert!(remove_app_dll_overrides(&file, &ours).unwrap().is_empty());
        assert_eq!(key.string("d3dx9_35").unwrap().as_deref(), Some("builtin"));
        assert_eq!(probe::run(&exe, None), probe::BUILTIN, "the player's value");

        // Ours, then changed by the player: kept, and no longer ours.
        key.delete_value("d3dx9_35").unwrap();
        assert_eq!(
            app_dll_overrides(&file, &ours).unwrap().written,
            ["d3dx9_35"]
        );
        key.set_string("d3dx9_35", "native").unwrap();
        assert!(app_dll_overrides(&file, &ours).unwrap().player.len() == 1);
        key.set_string("d3dx9_35", "builtin").unwrap();
        assert!(remove_app_dll_overrides(&file, &ours).unwrap().is_empty());
        assert_eq!(key.string("d3dx9_35").unwrap().as_deref(), Some("builtin"));

        key.delete_value("d3dx9_35").unwrap();
        let owned =
            crate::reg::Key::create(crate::reg::Hive::CurrentUser, &owned_key(&file)).unwrap();
        assert_eq!(owned.string("d3dx9_35").unwrap(), None, "no record left");
        let _ = std::fs::remove_dir_all(exe.parent().unwrap());
    }
}
