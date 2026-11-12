//! The registry redirect in a stand-in game (`examples/registry_game.rs`)
//! that loads the DLL through its import table (`import_into`), as CMR4
//! and CMR2005 load it. Windows or Wine only (`scripts/wine-test`); no
//! window opens.

#![cfg(windows)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use rea_launcher_win32::reg::{Hive, Key};

/// `target/<triple>/debug`.
fn build_dir() -> PathBuf {
    // The profile folder: the parent of `examples` (either Cargo layout).
    let example = rea_launcher::testing::example("registry_game.exe");
    example.parent().unwrap().parent().unwrap().to_path_buf()
}

/// A fresh key name under SOFTWARE: the test prefix keeps earlier runs'.
fn base(name: &str) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("rea-registry-{name}-{}-{stamp}", std::process::id())
}

/// The game's values where it used to read them (HKLM) and where the
/// launcher now writes them (HKCU).
fn seed(base: &str) {
    for (hive, root, tag) in [
        (Hive::LocalMachine, "SOFTWARE", "machine"),
        (Hive::CurrentUser, "Software", "user"),
    ] {
        for (sub, value) in [
            ("Game", tag.to_owned()),
            ("Walk", format!("{tag}-walk")),
            ("Other", format!("{tag}-other")),
        ] {
            Key::create(hive, &format!(r"{root}\{base}\{sub}"))
                .and_then(|key| key.set_string("Path", &value))
                .unwrap();
        }
    }
}

/// Runs a private copy of the stand-in with the DLL in its import table
/// and `rules` in `REA_REGISTRY` (none: the variable unset); what it read.
fn run(name: &str, base: &str, rules: Option<&str>) -> BTreeMap<String, String> {
    let build = build_dir();
    let dir = build
        .join("examples")
        .join(format!("registry-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = std::fs::read(build.join("examples").join("registry_game.exe")).unwrap();
    let exe = dir.join("registry_game.exe");
    std::fs::write(&exe, rea_close_hook::import_into(&game).unwrap()).unwrap();
    let dll = rea_launcher::testing::built(rea_close_hook::DLL);
    std::fs::copy(dll, dir.join(rea_close_hook::DLL)).unwrap();
    let mut command = Command::new(&exe);
    command.arg(base).env_remove(rea_close_hook::registry::ENV);
    if let Some(rules) = rules {
        command.env(rea_close_hook::registry::ENV, rules);
    }
    let output = command.output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(output.status.success(), "{output:?}");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

fn value(hive: Hive, path: &str, name: &str) -> Option<String> {
    Key::open_read(hive, path)
        .unwrap()
        .and_then(|key| key.string(name).unwrap())
}

fn rules(base: &str) -> String {
    rea_close_hook::registry::Config::parse(&format!(
        "HKLM\\SOFTWARE\\{base}\\Game -> HKCU\\Software\\{base}\\Game\n\
         walk HKLM\\SOFTWARE\\{base}\\Walk -> HKCU\\Software\\{base}\\Walk"
    ))
    .unwrap()
    .env_value()
}

#[test]
fn matched_keys_are_read_and_written_in_hkcu() {
    let base = base("on");
    seed(&base);
    let read = run("on", &base, Some(&rules(&base)));
    let expected = [
        ("full", "user"),
        ("wide", "user"),
        // No rule names it: HKLM's, untouched.
        ("other", "machine-other"),
        ("create", "ok"),
        ("create_sub", "ok"),
        // CMR1's walk reaches HKCU; a step off the path stays HKLM's.
        ("walk", "user-walk"),
        ("walk_off", "machine-other"),
    ];
    for (name, want) in expected {
        assert_eq!(read.get(name).map(String::as_str), Some(want), "{name}");
    }
    // The game's writes through the redirected handle land in HKCU.
    let user = format!(r"Software\{base}\Game");
    let machine = format!(r"SOFTWARE\{base}\Game");
    assert_eq!(
        value(Hive::CurrentUser, &user, "Written").as_deref(),
        Some("yes")
    );
    assert_eq!(value(Hive::LocalMachine, &machine, "Written"), None);
    assert!(
        Key::open_read(Hive::CurrentUser, &format!(r"{user}\Sub"))
            .unwrap()
            .is_some()
    );
    assert!(
        Key::open_read(Hive::LocalMachine, &format!(r"{machine}\Sub"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn without_rules_the_game_reads_hklm() {
    // The control: the same DLL without REA_REGISTRY changes nothing.
    let base = base("off");
    seed(&base);
    let read = run("off", &base, None);
    for (name, want) in [
        ("full", "machine"),
        ("wide", "machine"),
        ("walk", "machine-walk"),
        ("walk_off", "machine-other"),
    ] {
        assert_eq!(read.get(name).map(String::as_str), Some(want), "{name}");
    }
    assert_eq!(
        value(
            Hive::LocalMachine,
            &format!(r"SOFTWARE\{base}\Game"),
            "Written"
        )
        .as_deref(),
        Some("yes")
    );
}

#[test]
fn rules_that_dont_parse_leave_the_game_alone() {
    let base = base("bad");
    seed(&base);
    let read = run("bad", &base, Some("HKLM\\SOFTWARE\\X => nowhere"));
    assert_eq!(read.get("full").map(String::as_str), Some("machine"));
}
