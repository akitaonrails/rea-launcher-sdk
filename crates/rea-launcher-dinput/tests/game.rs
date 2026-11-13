//! The layer in a stand-in game (`examples/wheel_game.rs`) that imports
//! it through its import table (`import_into`), with the test-only fake
//! devices as the player's wheel and pedals. Windows or Wine only
//! (`scripts/wine-test`), headless: the game's window is never shown.

#![cfg(windows)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use rea_launcher::wheel::dinput::FILE;

/// `target/<triple>/debug/examples`.
fn examples() -> PathBuf {
    rea_launcher::testing::example("wheel_game.exe")
        .parent()
        .unwrap()
        .to_path_buf()
}

/// What a game run printed.
struct Run {
    lines: Vec<String>,
    dir: PathBuf,
}

impl Run {
    fn value(&self, key: &str) -> Option<&str> {
        self.lines
            .iter()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
    }

    fn all(&self, key: &str) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .collect()
    }

    fn fake(&self) -> Vec<&str> {
        self.lines
            .iter()
            .filter_map(|l| l.strip_prefix("fake "))
            .collect()
    }

    fn wrapped(&self) -> bool {
        self.value("wrapped") == Some(rea_dinput::DLL)
    }

    fn first_device(&self) -> &str {
        self.value("devices")
            .unwrap_or_default()
            .split('|')
            .next()
            .unwrap_or_default()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The stand-in game, imported with the layer, in a folder of its own
/// with `ini` as the settings (none for `None`), the stand-in
/// `dinput8.dll` next to it when `stand_in`, run with `env` and `steps`.
fn run(name: &str, ini: Option<&str>, stand_in: bool, env: &[(&str, &str)], steps: &[&str]) -> Run {
    run_as(name, ini, stand_in, env, steps, Load::Imported)
}

/// How the layer gets into the game.
enum Load {
    /// In the game's import table (`import_into`).
    Imported,
    /// Renamed `.asi` and loaded by the game itself before it creates
    /// DirectInput, as an ASI loader does; the exe untouched.
    Asi,
}

fn run_as(
    name: &str,
    ini: Option<&str>,
    stand_in: bool,
    env: &[(&str, &str)],
    steps: &[&str],
    load: Load,
) -> Run {
    let examples = examples();
    let dir = examples.join(format!("dinput-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = std::fs::read(examples.join("wheel_game.exe")).unwrap();
    let exe = dir.join("wheel_game.exe");
    let dll = rea_launcher::testing::built(rea_dinput::DLL);
    let asi = "rea_dinput.asi";
    match load {
        Load::Imported => {
            let imported = rea_dinput::import_into(&game).unwrap();
            assert_eq!(
                rea_dinput::import_into(&imported).unwrap(),
                imported,
                "added once"
            );
            std::fs::write(&exe, imported).unwrap();
            std::fs::copy(dll, dir.join(rea_dinput::DLL)).unwrap();
        }
        Load::Asi => {
            std::fs::write(&exe, game).unwrap();
            std::fs::copy(dll, dir.join(asi)).unwrap();
        }
    }
    if let Some(ini) = ini {
        std::fs::write(dir.join(FILE), ini).unwrap();
    }
    let mut command = Command::new(&exe);
    command
        .args(steps)
        .current_dir(&dir)
        .stdout(Stdio::piped())
        .env_remove(rea_dinput::MODE_ENV);
    if stand_in {
        std::fs::copy(examples.join("dinput8.dll"), dir.join("dinput8.dll")).unwrap();
        prefer_the_folders_dinput8();
    }
    for (key, value) in env {
        command.env(key, value);
    }
    if matches!(load, Load::Asi) {
        command.env("REA_WHEEL_GAME_LOAD", asi);
    }
    let lines = finish(command.spawn().unwrap(), &dir);
    Run { lines, dir }
}

/// Under Wine, the game takes the `dinput8.dll` in its folder (Xidi's
/// place) only with a native override. An override in the child's
/// environment is ignored by some Wine versions (9.0 in CI) when a Windows
/// process starts another, so it goes in the prefix's registry, for the
/// stand-in game only. Windows loads a folder's `dinput8.dll` first anyway
/// (it is no KnownDLL) and ignores the key.
fn prefer_the_folders_dinput8() {
    let status = Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\Wine\AppDefaults\wheel_game.exe\DllOverrides",
            "/v",
            "dinput8",
            "/d",
            "native,builtin",
            "/f",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "reg add: {status}");
}

/// The game's output once it ends (killed after 60 s: never a hang).
fn finish(mut child: std::process::Child, dir: &Path) -> Vec<String> {
    let start = Instant::now();
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = std::fs::remove_dir_all(dir);
            panic!("the game hung");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let text = reader.join().unwrap();
    text.lines().map(|l| l.trim_end().to_owned()).collect()
}

const FAKE_WHEEL: &str = "mode = auto\nfake = wheel\n";

#[test]
fn the_game_sees_the_virtual_wheel_first_as_cmr_reads_it() {
    let r = run("cmr", Some(FAKE_WHEEL), false, &[], &["read"]);
    assert_eq!(r.value("create"), Some("0x0"), "{:?}", r.lines);
    assert!(r.wrapped(), "{:?}", r.lines);
    assert_eq!(r.first_device(), "rea wheel");
    assert!(
        !r.value("devices").unwrap().contains("fake"),
        "the merged devices are hidden"
    );
    // DI8DEVTYPE_DRIVING, dual pedals, HID; VID 1209, PID 5245.
    assert_eq!(r.value("info"), Some("type:0x10316 product:52451209"));
    assert_eq!(r.value("caps"), Some("axes:4 buttons:32 povs:1 ff:true"));
    for key in ["device", "format", "cooperate", "acquire"] {
        assert_eq!(r.value(key), Some("0x0"), "{key}: {:?}", r.lines);
    }
    assert_eq!(r.value("ranges"), Some("4/4"));
    // CMR04 runs force feedback only when this works.
    assert_eq!(r.value("ffgain"), Some("0x0,10000"));
    // At rest: centred steering and pedal axis at 127, pedals up.
    assert_eq!(r.value("state"), Some("0x0:127,127,0,0 buttons=0x0 pov=-1"));
    assert!(r.lines.iter().any(|l| l == "done"));
}

#[test]
fn steering_and_pedals_reach_the_game_after_the_lock() {
    let r = run(
        "input",
        Some(FAKE_WHEEL),
        false,
        &[],
        &[
            // A quarter turn right of the 1080° wheel: full lock at 270°,
            // buttons 0 and 2, the hat right.
            "input:0:8192,32767,32767,0,0,0,0,0:5:9000",
            "read",
            // A sixteenth left: a quarter of the lock.
            "input:0:-2048,32767,32767,0,0,0,0,0:0:-1",
            "read",
            // Past the calibration's first second: both pedals floored.
            "wait:1100",
            "input:0:0,-32768,-32768,0,0,0,0,0:0:-1",
            "read",
            "input:0:0,-32768,32767,0,0,0,0,0:0:-1",
            "read",
        ],
    );
    assert_eq!(
        r.all("state"),
        [
            "0x0:255,127,0,0 buttons=0x5 pov=9000",
            "0x0:96,127,0,0 buttons=0x0 pov=-1",
            "0x0:127,127,255,255 buttons=0x0 pov=-1",
            "0x0:127,255,255,0 buttons=0x0 pov=-1",
        ],
        "{:?}",
        r.lines
    );
}

#[test]
fn buffered_data_reports_what_changed() {
    let r = run(
        "buffered",
        Some(FAKE_WHEEL),
        false,
        &[],
        &[
            "data",
            "buffer:8",
            "data",
            "input:0:8192,32767,32767,0,0,0,0,0:1:-1",
            "data",
            "data",
        ],
    );
    let data = r.all("data");
    // DIERR_NOTBUFFERED before a buffer size is set.
    assert_eq!(data[0], "0x80040207:", "{:?}", r.lines);
    assert_eq!(r.value("buffer"), Some("0x0"));
    // Steering (lX at 0) full right, button 0 (at 48) down; then nothing.
    assert_eq!(data[1..], ["0x0:", "0x0:0=255,48=128", "0x0:"]);
}

#[test]
fn separate_pedals_merge_into_the_one_wheel() {
    let ini = format!(
        "mode = auto\nfake = wheel+pedals\npedals = {}\n",
        rea_dinput::fake::PEDALS_GUID
    );
    let r = run(
        "pedals",
        Some(&ini),
        false,
        &[],
        &["wait:1100", "input:1:-32768,32767,0,0,0,0,0,0:1:-1", "read"],
    );
    assert_eq!(r.first_device(), "rea wheel");
    // Throttle on the pedal device, floored; its button after the
    // wheel's twelve.
    assert_eq!(
        r.value("state"),
        Some("0x0:127,255,255,0 buttons=0x1000 pov=-1"),
        "{:?}",
        r.lines
    );
}

#[test]
fn effects_reach_the_wheel_scaled_and_survive_a_replug() {
    let ini = format!("{FAKE_WHEEL}ffb_strength = 50\n");
    let r = run(
        "effects",
        Some(&ini),
        false,
        &[],
        &[
            "events", "effects", "events", "unplug:0", "read", "plug:0", "wait:700", "read",
            "events",
        ],
    );
    for name in ["spring", "damper", "sine", "square", "constant"] {
        assert!(
            r.all("effect")
                .contains(&format!("{name},0x0,0x0").as_str()),
            "{name}: {:?}",
            r.lines
        );
    }
    assert_eq!(r.value("autocenter"), Some("0x0"));
    assert_eq!(r.value("update"), Some("0x0"));
    let fake = r.fake();
    let has = |line: &str| fake.iter().any(|l| l.starts_with(line));
    // Opened as the launcher reads it, exclusive on the game's window.
    for line in [
        "0: format 164 objects",
        "0: range -32768 32767",
        "0: cooperate exclusive",
        "0: acquire",
    ] {
        assert!(has(line), "{line}: {fake:#?}");
    }
    // Half strength in each effect, once; the wheel gets the game's own
    // gain. Along X, on one axis.
    for line in [
        "0: gain 10000",
        "0: autocenter 0",
        "0: create spring offset=0 coefficient=4000/4000 saturation=5000/5000 deadband=0 duration=-1 gain=10000 direction=[1]",
        "0: create damper offset=0 coefficient=4000/4000",
        "0: create sine magnitude=3000 offset=0 phase=0 period=100000",
        "0: create square magnitude=5000",
        "0: create constant magnitude=3000",
        "0: start 1 spring",
        "0: start 1 constant",
        "0: set 0x40000100 constant magnitude=-4000",
    ] {
        assert!(has(line), "{line}: {fake:#?}");
    }
    assert!(
        !has("0: gain 5000"),
        "the strength is not in the gain too: {fake:#?}"
    );
    // Unplugged: the wheel reads neutral; plugged back: acquired, its
    // settings and every effect back and playing.
    assert_eq!(r.all("state")[0], "0x0:127,127,0,0 buttons=0x0 pov=-1");
    let back = fake
        .iter()
        .position(|l| *l == "0: plugged")
        .expect("plugged");
    let after: Vec<&str> = fake[back..].to_vec();
    for line in [
        "0: acquire",
        "0: autocenter 0",
        "0: gain 10000",
        "0: download spring",
        "0: start 1 spring",
        "0: start 1 constant",
    ] {
        assert!(
            after.iter().any(|l| l.starts_with(line)),
            "{line}: {after:#?}"
        );
    }
    assert!(
        after
            .iter()
            .any(|l| l.starts_with("0: set ") && l.contains("constant magnitude=-4000")),
        "{after:#?}"
    );
}

#[test]
fn inverted_force_feedback_pushes_the_other_way() {
    let ini = format!("{FAKE_WHEEL}ffb_strength = 100\nffb_invert = on\n");
    let r = run("invert", Some(&ini), false, &[], &["effects", "events"]);
    let fake = r.fake();
    assert!(
        fake.iter()
            .any(|l| l.starts_with("0: create constant magnitude=-6000")),
        "{fake:#?}"
    );
    assert!(
        fake.iter()
            .any(|l| l.starts_with("0: set 0x40000100 constant magnitude=8000")),
        "{fake:#?}"
    );
}

#[test]
fn the_games_gain_reaches_the_wheel_as_it_set_it() {
    let ini = "mode = auto\nfake = wheel\nffb_strength = 50\n";
    let r = run(
        "gain",
        Some(ini),
        false,
        &[],
        &["events", "gain:7000", "events"],
    );
    assert_eq!(r.value("gain"), Some("0x0,7000"), "{:?}", r.lines);
    let fake = r.fake();
    assert!(fake.contains(&"0: gain 7000"), "{fake:#?}");
    // A wheel that refuses the gain changes nothing for the effects.
    let ini = "mode = auto\nfake = wheel nogain\nffb_strength = 50\n";
    let r = run("nogain", Some(ini), false, &[], &["effects", "events"]);
    let fake = r.fake();
    for line in ["0: refused gain", "0: create constant magnitude=3000"] {
        assert!(
            fake.iter().any(|l| l.starts_with(line)),
            "{line}: {fake:#?}"
        );
    }
}

#[test]
fn a_refused_norestart_update_is_not_asked_again() {
    let ini = "mode = auto\nfake = wheel norestart\nffb_strength = 100\n";
    let r = run(
        "norestart",
        Some(ini),
        false,
        &[],
        &["effects", "update:-5000", "update:-3000", "events"],
    );
    let fake = r.fake();
    let refused = fake
        .iter()
        .filter(|l| l.starts_with("0: refused set"))
        .count();
    assert_eq!(refused, 1, "{fake:#?}");
    for line in [
        "0: set 0x100 constant magnitude=-8000",
        "0: set 0x100 constant magnitude=-5000",
        "0: set 0x100 constant magnitude=-3000",
    ] {
        assert!(
            fake.iter().any(|l| l.starts_with(line)),
            "{line}: {fake:#?}"
        );
    }
}

#[test]
fn effects_started_early_or_after_a_reset_play() {
    let r = run(
        "reset",
        Some(FAKE_WHEEL),
        false,
        &[],
        &["incomplete", "events", "effects", "reset", "events"],
    );
    assert_eq!(r.value("incomplete"), Some("0x0,0x0,0x0"), "{:?}", r.lines);
    let fake = r.fake();
    let made = fake
        .iter()
        .position(|l| l.starts_with("0: create constant magnitude=3500"))
        .expect("made once complete");
    assert_eq!(
        fake.get(made + 1).copied(),
        Some("0: start 1 constant"),
        "{fake:#?}"
    );
    // After the reset every effect is made again, and plays.
    let reset = fake
        .iter()
        .position(|l| *l == "0: command 0x1")
        .expect("the reset");
    let after = &fake[reset..];
    for name in ["spring", "sine", "constant"] {
        let created = after
            .iter()
            .position(|l| l.starts_with(&format!("0: create {name}")));
        let started = after
            .iter()
            .position(|l| *l == format!("0: start 1 {name}"));
        assert!(created.is_some() && started > created, "{name}: {after:#?}");
    }
}

#[test]
fn a_property_that_reacquires_the_wheel_brings_the_effects_back() {
    let ini = "mode = auto\nfake = wheel lockedprops\n";
    let r = run(
        "lockedprops",
        Some(ini),
        false,
        &[],
        &["effects", "events", "autocenter:1", "read", "events"],
    );
    let fake = r.fake();
    let toggled = fake
        .iter()
        .position(|l| *l == "0: autocenter 1")
        .expect("autocentre set");
    let after = &fake[toggled..];
    for line in ["0: acquire", "0: download spring", "0: start 1 spring"] {
        assert!(
            after.iter().any(|l| l.starts_with(line)),
            "{line}: {after:#?}"
        );
    }
}

#[test]
fn a_wheel_another_program_holds_still_steers() {
    let ini = "mode = auto\nfake = wheel busy\n";
    let r = run(
        "busy",
        Some(ini),
        false,
        &[],
        &[
            "input:0:8192,32767,32767,0,0,0,0,0:0:-1",
            "read",
            "effects",
            "events",
            // The other program lets go (the fake: until it is replugged).
            "unplug:0",
            "read",
            "plug:0",
            "wait:700",
            "read",
            "events",
        ],
    );
    assert_eq!(
        r.all("state")[0],
        "0x0:255,127,0,0 buttons=0x0 pov=-1",
        "{:?}",
        r.lines
    );
    let fake = r.fake();
    let back = fake
        .iter()
        .position(|l| *l == "0: plugged")
        .expect("plugged");
    let (before, after) = fake.split_at(back);
    assert!(before.contains(&"0: cooperate shared"), "{fake:#?}");
    assert!(
        !before.iter().any(|l| l.starts_with("0: create")),
        "no force feedback: {fake:#?}"
    );
    // Exclusive again: the effects are made and play.
    assert!(after.contains(&"0: cooperate exclusive"), "{after:#?}");
    assert!(
        after.iter().any(|l| l.starts_with("0: create constant")),
        "{after:#?}"
    );
}

#[test]
fn a_wheel_that_cant_be_opened_leaves_the_game_alone() {
    let r = run(
        "broken",
        Some("mode = on\nfake = wheel broken\n"),
        false,
        &[],
        &[],
    );
    assert!(!r.wrapped(), "{:?}", r.lines);
    assert_ne!(r.first_device(), "rea wheel");
}

#[test]
fn on_without_a_wheel_picks_one_up_when_it_is_plugged_in() {
    let ini = "mode = on\nfake = wheel unplugged\n";
    let r = run(
        "late",
        Some(ini),
        false,
        &[],
        &[
            "read",
            "plug:0",
            "wait:2300",
            "input:0:8192,32767,32767,0,0,0,0,0:0:-1",
            "read",
            "effects",
            "events",
        ],
    );
    assert_eq!(r.first_device(), "rea wheel");
    assert_eq!(
        r.value("caps"),
        Some("axes:4 buttons:32 povs:1 ff:false"),
        "no wheel, no force feedback"
    );
    assert_eq!(
        r.all("state"),
        [
            "0x0:127,127,0,0 buttons=0x0 pov=-1",
            "0x0:255,127,0,0 buttons=0x0 pov=-1"
        ],
        "{:?}",
        r.lines
    );
    assert!(
        r.fake().iter().any(|l| l.starts_with("0: create constant")),
        "{:?}",
        r.fake()
    );
}

#[test]
fn a_second_virtual_device_shares_the_one_wheel() {
    let r = run(
        "second",
        Some(FAKE_WHEEL),
        false,
        &[],
        &[
            "input:0:8192,32767,32767,0,0,0,0,0:0:-1",
            "second",
            "read",
            "events",
        ],
    );
    assert_eq!(r.all("device"), ["0x0", "0x0"]);
    // Both devices read the turned wheel: the second one's set-up never
    // let the first one's wheel go.
    assert_eq!(
        r.all("state"),
        [
            "0x0:255,127,0,0 buttons=0x0 pov=-1",
            "0x0:255,127,0,0 buttons=0x0 pov=-1"
        ],
        "{:?}",
        r.lines
    );
    let fake = r.fake();
    let count = |prefix: &str| fake.iter().filter(|l| l.starts_with(prefix)).count();
    assert_eq!(count("0: format"), 1, "opened once: {fake:#?}");
    assert_eq!(count("0: cooperate"), 1, "cooperates once: {fake:#?}");
}

#[test]
fn a_device_released_while_acquired_lets_the_wheel_go() {
    let r = run(
        "dropped",
        Some(FAKE_WHEEL),
        false,
        &[],
        &["second-drop", "events", "unacquire", "events"],
    );
    assert_eq!(r.value("unacquire"), Some("0x0"));
    let step = r.lines.iter().position(|l| l == "unacquire=0x0").unwrap();
    let after: Vec<&str> = r.lines[step..]
        .iter()
        .filter_map(|l| l.strip_prefix("fake "))
        .collect();
    assert_eq!(
        after,
        ["0: unacquire"],
        "the last holder lets the wheel go: {:?}",
        r.lines
    );
}

#[test]
fn only_a_wheel_with_force_feedback_lists_under_the_force_feedback_filter() {
    let r = run("fflist", Some(FAKE_WHEEL), false, &[], &[]);
    assert_eq!(
        r.value("ffdevices").unwrap_or_default().split('|').next(),
        Some("rea wheel")
    );
    let r = run("fflist-on", Some("mode = on\n"), false, &[], &[]);
    assert_eq!(r.first_device(), "rea wheel");
    assert!(
        !r.value("ffdevices")
            .unwrap_or_default()
            .contains("rea wheel"),
        "{:?}",
        r.lines
    );
}

#[test]
fn without_settings_the_game_gets_its_directinput_untouched() {
    let r = run("none", None, false, &[], &[]);
    assert_eq!(r.value("create"), Some("0x0"));
    assert!(!r.wrapped(), "{:?}", r.lines);
    assert_ne!(r.first_device(), "rea wheel");
}

#[test]
fn off_auto_and_the_launchers_decision() {
    let off = run("off", Some("mode = off\nfake = wheel\n"), false, &[], &[]);
    assert!(
        !off.wrapped() && off.first_device() != "rea wheel",
        "{:?}",
        off.lines
    );
    let on_by_launcher = run(
        "env-on",
        Some("mode = off\nfake = wheel\n"),
        false,
        &[(rea_dinput::MODE_ENV, "on")],
        &[],
    );
    assert_eq!(
        on_by_launcher.first_device(),
        "rea wheel",
        "{:?}",
        on_by_launcher.lines
    );
    let off_by_launcher = run(
        "env-off",
        Some(FAKE_WHEEL),
        false,
        &[(rea_dinput::MODE_ENV, "off")],
        &[],
    );
    assert!(!off_by_launcher.wrapped());
    let garbage = run(
        "env-garbage",
        Some(FAKE_WHEEL),
        false,
        &[(rea_dinput::MODE_ENV, "maybe")],
        &[],
    );
    assert_eq!(garbage.first_device(), "rea wheel", "the file's mode");
    // Auto without a wheel: untouched. On without a wheel: a neutral one.
    let auto = run("auto", Some("mode = auto\n"), false, &[], &[]);
    assert!(!auto.wrapped());
    let on = run("on", Some("mode = on\n"), false, &[], &["read"]);
    assert_eq!(on.first_device(), "rea wheel");
    assert_eq!(
        on.value("state"),
        Some("0x0:127,127,0,0 buttons=0x0 pov=-1")
    );
}

#[test]
fn a_broken_settings_file_is_no_crash() {
    let r = run(
        "broken",
        Some("\u{0}\u{1}= = =\nmode = sideways\nfake\nlock = -7\n[x"),
        false,
        &[],
        &["read"],
    );
    assert_eq!(r.value("create"), Some("0x0"));
    assert!(r.lines.iter().any(|l| l == "done"), "{:?}", r.lines);
}

#[test]
fn the_layer_chains_in_front_of_another_dinput8() {
    let r = run("chain", Some(FAKE_WHEEL), true, &[], &["read"]);
    assert_eq!(r.value("stand_in"), Some("1"), "{:?}", r.lines);
    assert!(r.wrapped());
    assert_eq!(r.first_device(), "rea wheel");
    assert_eq!(r.value("state"), Some("0x0:127,127,0,0 buttons=0x0 pov=-1"));
    // Off: the stand-in's DirectInput as it is.
    let off = run("chain-off", Some("mode = off\n"), true, &[], &[]);
    assert_eq!(off.value("stand_in"), Some("1"));
    assert!(!off.wrapped());
}

#[test]
fn the_log_says_what_happened_when_asked() {
    let r = run(
        "log",
        Some(&format!("{FAKE_WHEEL}log = on\n")),
        false,
        &[],
        &["read"],
    );
    let log = std::fs::read_to_string(r.dir.join(rea_dinput::log::FILE)).unwrap();
    for line in [
        "mode auto",
        "found rea fake wheel",
        "virtual wheel from rea fake wheel",
        "SetDataFormat: 272 bytes",
        "Acquire: 0x0",
    ] {
        assert!(log.contains(line), "{line}: {log}");
    }
    let quiet = run("quiet", Some(FAKE_WHEEL), false, &[], &[]);
    assert!(!quiet.dir.join(rea_dinput::log::FILE).exists());
}

#[test]
fn the_layer_works_loaded_late_as_an_asi_plugin() {
    let r = run_as(
        "asi",
        Some(FAKE_WHEEL),
        false,
        &[],
        &["input:0:8192,32767,32767,0,0,0,0,0:0:-1", "read"],
        Load::Asi,
    );
    assert_eq!(r.value("loaded"), Some("true"), "{:?}", r.lines);
    // Its DllMain pointed the already-bound import at the wrapper.
    assert_eq!(r.value("wrapped"), Some("rea_dinput.asi"));
    assert_eq!(r.first_device(), "rea wheel");
    assert_eq!(r.value("state"), Some("0x0:255,127,0,0 buttons=0x0 pov=-1"));
}
