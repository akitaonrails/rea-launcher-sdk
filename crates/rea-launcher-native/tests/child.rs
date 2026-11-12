//! A tied child dies with its parent, on Linux (PDEATHSIG) and on Windows
//! (the kill-on-close job; run with `scripts/wine-test --target
//! x86_64-pc-windows-gnu -p rea-launcher-native`). This test binary plays
//! every part: the test, a "parent" that spawns a child and is then
//! killed, and the "sleeper" child, which writes a heartbeat file until
//! it is killed or told to stop. An untied child is the control: it must
//! outlive its parent, or the test could not see a survivor.
#![cfg(feature = "child")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

const ROLE: &str = "REA_NATIVE_TEST_ROLE";
const BEAT: &str = "REA_NATIVE_TEST_BEAT";
const STOP: &str = "REA_NATIVE_TEST_STOP";
const TIED: &str = "REA_NATIVE_TEST_TIED";

/// This test binary, running only [`role_helper`] in `role`.
fn helper(role: &str) -> Command {
    let mut cmd = Command::new(env::current_exe().unwrap());
    cmd.args(["--exact", "role_helper", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

/// Does nothing in an ordinary test run; plays a part when asked to.
#[test]
fn role_helper() {
    let var = |k| PathBuf::from(env::var_os(k).unwrap_or_default());
    match env::var(ROLE).as_deref() {
        Ok("parent") => {
            let mut cmd = helper("sleeper");
            let mut child = if env::var_os(TIED).is_some() {
                rea_launcher_native::child::spawn_tied(&mut cmd)
            } else {
                cmd.spawn()
            }
            .unwrap();
            // wait for it on this thread (PDEATHSIG follows the spawning
            // thread), as a launcher waits for its emulator, until the
            // test kills us
            let _ = child.wait();
        }
        Ok("sleeper") => {
            let (beat, stop) = (var(BEAT), var(STOP));
            let started = Instant::now();
            let mut n = 0u64;
            while !stop.exists() && started.elapsed() < Duration::from_secs(120) {
                n += 1;
                let _ = fs::write(&beat, n.to_string());
                thread::sleep(Duration::from_millis(50));
            }
        }
        _ => {}
    }
}

fn wait_for(path: &Path, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Whether the child kept beating after its parent was killed.
fn child_outlives_killed_parent(tied: bool) -> bool {
    let dir = env::temp_dir().join(format!("rea-native-child-{}-{tied}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (beat, stop) = (dir.join("beat"), dir.join("stop"));
    let mut cmd = helper("parent");
    cmd.env(BEAT, &beat).env(STOP, &stop);
    if tied {
        cmd.env(TIED, "1");
    }
    let mut parent = cmd.spawn().unwrap();
    let started = wait_for(&beat, Duration::from_secs(60));
    parent.kill().unwrap();
    parent.wait().unwrap();
    assert!(started, "the child never started");
    thread::sleep(Duration::from_millis(1500));
    let read = || fs::read_to_string(&beat).unwrap_or_default();
    let before = read();
    thread::sleep(Duration::from_millis(800));
    let alive = read() != before;
    // a survivor stops on its own
    fs::write(&stop, "").unwrap();
    thread::sleep(Duration::from_millis(300));
    let _ = fs::remove_dir_all(&dir);
    alive
}

#[test]
fn a_tied_child_dies_with_its_parent() {
    assert!(!child_outlives_killed_parent(true));
}

#[test]
fn an_untied_child_outlives_its_parent() {
    assert!(child_outlives_killed_parent(false));
}
