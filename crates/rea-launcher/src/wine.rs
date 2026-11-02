//! Wine: DLL overrides, detection from the environment, winebus values.
//!
//! - [`merge_dll_overrides`] is CMR4's `wine_overrides` generalised (as the
//!   win32 crate first did), with CMR3 4d7291f / CMR4 0d75edb: Wine reads
//!   `d3d9.dll` and `D3D9` as `d3d9`, so a player's rule spelled with
//!   `.dll` is replaced too. Ours come first; the player's other rules stay.
//! - [`is_wine_env`] is the fallback CMR4/OR use when a Wine build hides
//!   `wine_get_version` (the export check itself is in the win32 crate).
//! - The winebus names are OR's: SRR's `srr-play` wrote `"Disable Hidraw"`
//!   and `"Disable Input"`, which Wine never reads (SRR d70d8d4). Which
//!   values a game wants is its own knowledge; [`SDL_ONLY`] is OR's
//!   known-good baseline, offered as data.

/// The DLL names a `WINEDLLOVERRIDES` string sets, lower-cased and without
/// a `.dll` suffix.
pub fn override_names(rules: &str) -> Vec<String> {
    rules
        .split(';')
        .filter_map(|rule| rule.split_once('=').map(|(names, _)| names))
        .flat_map(|names| names.split(','))
        .map(dll_stem)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

/// `dsound.dll` names the same DLL as `dsound`.
fn dll_stem(name: &str) -> &str {
    let name = name.trim();
    match name.len().checked_sub(4) {
        Some(at)
            if name
                .get(at..)
                .is_some_and(|ext| ext.eq_ignore_ascii_case(".dll")) =>
        {
            &name[..at]
        }
        _ => name,
    }
}

/// `ours` first, then each of the player's `existing` rules with the DLLs
/// that `ours` names taken out (a rule left with no DLL is dropped). Rules
/// without `=` and DLLs we don't decide are passed on as written.
pub fn merge_dll_overrides(ours: &str, existing: Option<&str>) -> String {
    let decided = override_names(ours);
    let mut rules: Vec<String> = ours
        .split(';')
        .filter(|rule| !rule.trim().is_empty())
        .map(str::to_owned)
        .collect();
    for rule in existing.into_iter().flat_map(|rules| rules.split(';')) {
        if rule.trim().is_empty() {
            continue;
        }
        let Some((names, action)) = rule.split_once('=') else {
            rules.push(rule.to_owned());
            continue;
        };
        let kept: Vec<&str> = names
            .split(',')
            .map(str::trim)
            .filter(|name| {
                let stem = dll_stem(name);
                !stem.is_empty() && !decided.iter().any(|ours| stem.eq_ignore_ascii_case(ours))
            })
            .collect();
        if !kept.is_empty() {
            rules.push(format!("{}={action}", kept.join(",")));
        }
    }
    rules.join(";")
}

/// Environment variables that only exist under Wine or Proton (set by
/// Wine itself, Proton, or the play helpers).
pub const WINE_VARIABLES: [&str; 3] = ["WINEPREFIX", "WINELOADER", "STEAM_COMPAT_DATA_PATH"];

/// Whether the environment says Wine; `is_set` reads one variable.
pub fn wine_env(is_set: impl Fn(&str) -> bool) -> bool {
    WINE_VARIABLES.into_iter().any(is_set)
}

/// [`wine_env`] on this process's environment.
pub fn is_wine_env() -> bool {
    wine_env(|name| std::env::var_os(name).is_some())
}

/// Wine's bus driver key, under `HKEY_LOCAL_MACHINE`. Its values are
/// DWORDs, seeded only where absent (the player's choice stays).
pub const WINEBUS_KEY: &str = r"System\CurrentControlSet\Services\winebus";
/// 1: no raw hidraw devices (a pad would appear twice).
pub const DISABLE_HIDRAW: &str = "DisableHidraw";
/// 1: no raw evdev devices.
pub const DISABLE_INPUT: &str = "DisableInput";
/// 1: pads through SDL.
pub const ENABLE_SDL: &str = "Enable SDL";
/// 0: SDL pads keep their own layout instead of Wine's mapping.
pub const MAP_CONTROLLERS: &str = "Map Controllers";

/// OR's baseline: only SDL's view of each pad reaches the game.
pub const SDL_ONLY: [(&str, u32); 4] = [
    (DISABLE_HIDRAW, 1),
    (DISABLE_INPUT, 1),
    (ENABLE_SDL, 1),
    (MAP_CONTROLLERS, 0),
];

#[cfg(test)]
mod tests {
    use super::*;

    const CMR4: &str = "dinput8,dsound,d3d9,d3d11,dxgi=n,b;mscoree=d";
    const OR: &str = "dinput8=n,b;d3d9=n,b;mscoree,mshtml=d";

    #[test]
    fn ours_come_first_and_replace_the_same_dlls() {
        assert_eq!(merge_dll_overrides(CMR4, None), CMR4);
        assert_eq!(merge_dll_overrides(OR, Some("")), OR);
        assert_eq!(
            merge_dll_overrides(CMR4, Some("bnet=n,b;DINPUT8=b;foo=b;mscoree=n")),
            format!("{CMR4};bnet=n,b;foo=b")
        );
        assert_eq!(
            merge_dll_overrides(CMR4, Some("d3d8,d3d9=b;dsound=n")),
            format!("{CMR4};d3d8=b")
        );
        assert_eq!(
            merge_dll_overrides(
                OR,
                Some("DINPUT8=b;xinput1_3=n,b;d3d9,d3d11=b;MSCOREE,mshtml=;winemenubuilder.exe=d")
            ),
            format!("{OR};xinput1_3=n,b;d3d11=b;winemenubuilder.exe=d")
        );
    }

    #[test]
    fn rules_without_equals_and_dll_suffixes() {
        assert_eq!(
            merge_dll_overrides(OR, Some("odd;;foo=n")),
            format!("{OR};odd;foo=n")
        );
        assert_eq!(
            merge_dll_overrides("dsound=n,b", Some("DSound.DLL,winmm.dll=b;é=n")),
            "dsound=n,b;winmm.dll=b;é=n"
        );
        // Without our own rules the player's pass through untouched.
        assert_eq!(merge_dll_overrides("", Some("a=b;c")), "a=b;c");
        assert_eq!(merge_dll_overrides("", None), "");
    }

    #[test]
    fn override_names_lists_every_dll() {
        assert_eq!(
            override_names("D3D9.dll,dxgi=n,b;mscoree=d;stray"),
            ["d3d9", "dxgi", "mscoree"]
        );
    }

    #[test]
    fn wine_is_recognised_from_its_environment() {
        assert!(wine_env(|name| name == "WINEPREFIX"));
        assert!(wine_env(|name| name == "WINELOADER"));
        assert!(wine_env(|name| name == "STEAM_COMPAT_DATA_PATH"));
        assert!(!wine_env(|name| name == "USERPROFILE"));
    }

    #[test]
    fn winebus_names_are_the_ones_wine_reads() {
        // SRR's srr-play wrote these with spaces; Wine ignored them.
        for (name, _) in SDL_ONLY {
            assert_ne!(name, "Disable Hidraw");
            assert_ne!(name, "Disable Input");
        }
        assert_eq!(
            SDL_ONLY.map(|(n, _)| n),
            [
                "DisableHidraw",
                "DisableInput",
                "Enable SDL",
                "Map Controllers"
            ]
        );
    }
}
