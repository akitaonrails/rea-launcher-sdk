//! A session's processes on Linux, with a stand-in gamescope (a shell
//! script) that starts a "game" in a session of its own, as Wine's
//! processes leave the group: stopping the session finds and kills it, and
//! nothing untagged is ever signalled (decoys this test owns survive).
//! Every process here is one the test started.
#![cfg(all(feature = "group", target_os = "linux"))]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use rea_launcher_native::group::{Group, Tag};

const VAR: &str = "REA_NATIVE_TEST_SESSION";

/// A fresh folder for one test.
fn dir(name: &str) -> PathBuf {
    let d = env::temp_dir().join(format!("rea-native-group-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// A gamescope stand-in in `d`: it starts the game in a new session
/// (`setsid`), writes the game's pid to `escaped`, and waits.
fn fake_gamescope(d: &Path) -> Command {
    let script = d.join("gamescope");
    fs::write(
        &script,
        "#!/bin/sh\n\
         setsid sh -c 'echo $$ > \"$1.tmp\" && mv \"$1.tmp\" \"$1\" && exec sleep 300' sh \"$1\" &\n\
         wait\n",
    )
    .unwrap();
    let mut cmd = Command::new("sh");
    cmd.arg(&script)
        .arg(d.join("escaped"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

fn wait_for_pid(file: &Path) -> u32 {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if let Some(pid) = fs::read_to_string(file)
            .ok()
            .and_then(|s| s.trim().parse().ok())
        {
            return pid;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("the game never started");
}

/// Whether `pid` runs (exists and isn't a zombie).
fn runs(pid: u32) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|s| !rea_launcher::procs::is_zombie(&s))
}

fn gone_soon(pid: u32) -> bool {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if !runs(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A process the test owns: untagged, or tagged for another session.
fn decoy(tag: Option<&str>) -> Child {
    let mut cmd = Command::new("sleep");
    cmd.arg("300").stdin(Stdio::null());
    if let Some(value) = tag {
        cmd.env(VAR, value);
    }
    cmd.spawn().unwrap()
}

#[test]
fn a_stopped_session_leaves_nothing_and_touches_nothing_else() {
    let d = dir("stop");
    let value = d.to_string_lossy().into_owned();
    // decoys: untagged, another session whose tag starts the same, and one
    // in this test's own process group
    let mut decoys = [
        decoy(None),
        decoy(Some(&format!("{value}0"))),
        decoy(Some("x")),
    ];
    let mut g = Group::spawn(&mut fake_gamescope(&d), &Tag::new(VAR, &value)).unwrap();
    let escaped = wait_for_pid(&d.join("escaped"));
    assert!(g.warning().is_none());

    let members: Vec<u32> = g.members().iter().map(|m| m.pid).collect();
    assert!(
        members.contains(&g.id()) && members.contains(&escaped),
        "{members:?}"
    );
    let game = g.members().into_iter().find(|m| m.pid == escaped).unwrap();
    assert_eq!(game.argv, ["sleep", "300"]);

    // the polite stop reaches the group, not the game that left it
    g.terminate();
    let status = g.wait_exit().unwrap();
    assert!(!status.success());
    assert!(runs(escaped));
    assert!(g.any_left());
    // the leader's end was read without reaping it: the group's id is safe
    let stat = fs::read_to_string(format!("/proc/{}/stat", g.id())).unwrap();
    assert!(rea_launcher::procs::is_zombie(&stat), "{stat}");

    g.kill();
    assert!(gone_soon(escaped), "the game that left the group survived");
    let done = g.finish().unwrap();
    assert_eq!(done.status.code(), status.code());
    assert_eq!(done.leftovers, 0, "killed before finish: nothing left");

    for d in &mut decoys {
        assert!(d.try_wait().unwrap().is_none(), "a decoy was signalled");
        d.kill().unwrap();
        d.wait().unwrap();
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn finish_finds_and_kills_the_leftovers() {
    let d = dir("leftovers");
    let value = d.to_string_lossy().into_owned();
    let mut other = decoy(None);
    let mut g = Group::spawn(&mut fake_gamescope(&d), &Tag::new(VAR, &value)).unwrap();
    let escaped = wait_for_pid(&d.join("escaped"));
    // a stray pid, and one not of the session: no signal
    assert!(!g.terminate_one(other.id()));
    // the leader ends by itself (gamescope closed)
    assert!(g.terminate_one(g.id()));
    g.wait_exit().unwrap();
    let done = g.finish().unwrap();
    assert_eq!(done.leftovers, 1);
    assert!(gone_soon(escaped));
    assert!(
        other.try_wait().unwrap().is_none(),
        "the decoy was signalled"
    );
    other.kill().unwrap();
    other.wait().unwrap();
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_running_leader_is_not_reaped_by_looking() {
    let d = dir("running");
    let mut g = Group::spawn(&mut fake_gamescope(&d), &Tag::new(VAR, d.to_string_lossy())).unwrap();
    wait_for_pid(&d.join("escaped"));
    assert!(g.exited().unwrap().is_none());
    g.kill();
    let st = g.wait_exit().unwrap();
    assert_eq!(g.exited().unwrap(), Some(st));
    g.finish().unwrap();
    let _ = fs::remove_dir_all(&d);
}

const ROLE: &str = "REA_NATIVE_GROUP_ROLE";

/// Does nothing in an ordinary run; as the "launcher", starts a session
/// (the stand-in gamescope, whose game leaves the group) in the folder
/// `REA_NATIVE_GROUP_DIR` and waits until the test kills it.
#[test]
fn role_launcher() {
    if env::var(ROLE).as_deref() != Ok("launcher") {
        return;
    }
    let d = PathBuf::from(env::var_os("REA_NATIVE_GROUP_DIR").unwrap());
    let mut g = Group::spawn(&mut fake_gamescope(&d), &Tag::new(VAR, d.to_string_lossy())).unwrap();
    fs::write(d.join("leader.tmp"), g.id().to_string()).unwrap();
    fs::rename(d.join("leader.tmp"), d.join("leader")).unwrap();
    let _ = g.wait_exit();
}

#[test]
fn the_whole_session_dies_with_a_killed_launcher() {
    let d = dir("tied");
    let value = d.to_string_lossy().into_owned();
    let mut decoys = [decoy(None), decoy(Some(&format!("{value}0")))];
    let mut launcher = Command::new(env::current_exe().unwrap())
        .args([
            "--exact",
            "role_launcher",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, "launcher")
        .env("REA_NATIVE_GROUP_DIR", &d)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let leader = wait_for_pid(&d.join("leader"));
    let escaped = wait_for_pid(&d.join("escaped"));
    assert!(runs(leader) && runs(escaped));
    launcher.kill().unwrap();
    launcher.wait().unwrap();
    assert!(gone_soon(leader), "the leader outlived its launcher");
    assert!(
        gone_soon(escaped),
        "the game that left the group outlived its launcher"
    );
    for d in &mut decoys {
        assert!(d.try_wait().unwrap().is_none(), "a decoy was signalled");
        d.kill().unwrap();
        d.wait().unwrap();
    }
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_dropped_session_takes_its_processes_along() {
    let d = dir("dropped");
    let g = Group::spawn(&mut fake_gamescope(&d), &Tag::new(VAR, d.to_string_lossy())).unwrap();
    let escaped = wait_for_pid(&d.join("escaped"));
    let leader = g.id();
    drop(g);
    assert!(gone_soon(escaped) && gone_soon(leader));
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn a_missing_program_is_named_plainly() {
    let e = Group::spawn(
        &mut Command::new("rea-no-such-gamescope"),
        &Tag::new(VAR, "missing"),
    )
    .err()
    .unwrap();
    assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        e.to_string(),
        "rea-no-such-gamescope is not installed (or not on PATH)"
    );
}
