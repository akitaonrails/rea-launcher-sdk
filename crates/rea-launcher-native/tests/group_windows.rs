//! A session's processes on Windows (run with `scripts/wine-test --target
//! x86_64-pc-windows-gnu -p rea-launcher-native --features group`). This
//! test binary plays every part: the test; a "leader" that starts a
//! "sleeper" as its very first act (the child a late job would miss) and
//! waits; and a "window" leader whose hidden window saves and quits on
//! `WM_CLOSE`, as Flycast and Dolphin do. A sleeper the test starts itself
//! is the decoy: it must survive. The tests run one at a time: `kill_under`
//! looks at every process the test binary runs.
#![cfg(all(feature = "group", windows))]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use rea_launcher_native::group::{Group, Tag};

const ROLE: &str = "REA_NATIVE_GROUP_ROLE";
const DIR: &str = "REA_NATIVE_GROUP_DIR";
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// This test binary, running only [`role_helper`] as `role` in `dir`.
fn helper(role: &str, dir: &Path) -> Command {
    let mut cmd = Command::new(env::current_exe().unwrap());
    cmd.args(["--exact", "role_helper", "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .env(DIR, dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd
}

/// Does nothing in an ordinary run; plays a part when asked to.
#[test]
fn role_helper() {
    let dir = PathBuf::from(env::var_os(DIR).unwrap_or_default());
    match env::var(ROLE).as_deref() {
        Ok("leader") => {
            let mut child = helper("sleeper", &dir).spawn().unwrap();
            fs::write(dir.join("child"), child.id().to_string()).unwrap();
            let _ = child.wait();
        }
        Ok("sleeper") => beat(&dir.join(format!("beat-{}", std::process::id()))),
        Ok("window") => window::run(&dir.join("closed")),
        _ => {}
    }
}

/// Writes a counter to `file` every 50 ms for two minutes at most.
fn beat(file: &Path) {
    let started = Instant::now();
    let mut n = 0u64;
    while started.elapsed() < Duration::from_secs(120) {
        n += 1;
        let _ = fs::write(file, n.to_string());
        thread::sleep(Duration::from_millis(50));
    }
}

fn dir(name: &str) -> PathBuf {
    let d = env::temp_dir().join(format!("rea-native-groupw-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn wait_for(file: &Path) -> String {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        if let Ok(s) = fs::read_to_string(file)
            && !s.is_empty()
        {
            return s;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("{} never appeared", file.display());
}

/// Whether the sleeper writing `file` still beats.
fn beats(file: &Path) -> bool {
    let before = fs::read_to_string(file).unwrap_or_default();
    thread::sleep(Duration::from_millis(600));
    fs::read_to_string(file).unwrap_or_default() != before
}

fn decoy(d: &Path) -> (Child, PathBuf) {
    let child = helper("sleeper", d).spawn().unwrap();
    let file = d.join(format!("beat-{}", child.id()));
    wait_for(&file);
    (child, file)
}

#[test]
fn the_job_holds_the_first_child_and_only_the_session_is_ended() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir("job");
    let (mut decoy, decoy_beat) = decoy(&d);
    let mut g = Group::spawn(&mut helper("leader", &d), &Tag::new("UNUSED", "x")).unwrap();
    assert_eq!(g.warning(), None);
    let child: u32 = wait_for(&d.join("child")).trim().parse().unwrap();
    let child_beat = d.join(format!("beat-{child}"));
    wait_for(&child_beat);
    assert!(g.any_left());
    assert!(g.exited().unwrap().is_none());

    g.kill();
    g.wait_exit().unwrap();
    let done = g.finish().unwrap();
    assert!(!done.status.success());
    assert!(!beats(&child_beat), "the leader's first child survived");
    assert!(beats(&decoy_beat), "the decoy was ended");
    decoy.kill().unwrap();
    decoy.wait().unwrap();
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn finish_ends_what_the_leader_left_behind() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir("left");
    let mut g = Group::spawn(&mut helper("leader", &d), &Tag::new("UNUSED", "x")).unwrap();
    let child: u32 = wait_for(&d.join("child")).trim().parse().unwrap();
    let child_beat = d.join(format!("beat-{child}"));
    wait_for(&child_beat);
    // the leader quits by itself, its child runs on
    Command::new("taskkill")
        .args(["/F", "/PID", &g.id().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    g.wait_exit().unwrap();
    assert!(g.any_left());
    let done = g.finish().unwrap();
    // the child, and the console host Wine gives a console program
    assert!(done.leftovers >= 1, "{}", done.leftovers);
    assert!(!beats(&child_beat), "the leftover survived");
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn kill_under_ends_only_what_started_after_the_leader() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir("under");
    let (mut decoy, decoy_beat) = decoy(&d);
    // a session whose leader started before the decoy's sibling below
    let mut g = Group::spawn(&mut helper("sleeper", &d), &Tag::new("UNUSED", "x")).unwrap();
    wait_for(&d.join(format!("beat-{}", g.id())));
    // the test binary's folder holds every helper, the test itself too
    let bin = env::current_exe().unwrap();
    let ended = g.kill_under(bin.parent().unwrap());
    assert_eq!(ended, 1, "only the session's leader");
    g.wait_exit().unwrap();
    g.finish().unwrap();
    assert!(beats(&decoy_beat), "the decoy was ended");
    decoy.kill().unwrap();
    decoy.wait().unwrap();
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn terminate_asks_the_leaders_window_to_close() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir("close");
    let mut g = Group::spawn(&mut helper("window", &d), &Tag::new("UNUSED", "x")).unwrap();
    wait_for(&d.join("ready"));
    let windows = g.windows();
    assert!(windows.iter().any(|w| w.pid == g.id()), "{windows:?}");
    g.terminate();
    let started = Instant::now();
    while g.exited().unwrap().is_none() && started.elapsed() < Duration::from_secs(20) {
        thread::sleep(Duration::from_millis(50));
    }
    // (a group dropped while running is killed with its job)
    assert!(g.exited().unwrap().is_some(), "the window ignored WM_CLOSE");
    let done = g.finish().unwrap();
    assert!(done.status.success(), "{:?}", done.status);
    assert_eq!(wait_for(&d.join("closed")), "saved");
    let _ = fs::remove_dir_all(&d);
}

/// A hidden window that writes "saved" and quits on `WM_CLOSE`.
mod window {
    use std::path::Path;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostQuitMessage,
        RegisterClassW, WM_CLOSE, WNDCLASSW, WS_OVERLAPPEDWINDOW,
    };

    static SAVE_TO: OnceLock<std::path::PathBuf> = OnceLock::new();

    unsafe extern "system" fn proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        if msg == WM_CLOSE {
            let _ = std::fs::write(SAVE_TO.get().unwrap(), "saved");
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        unsafe { DefWindowProcW(h, msg, w, l) }
    }

    pub fn run(save_to: &Path) {
        SAVE_TO.set(save_to.to_path_buf()).unwrap();
        let class: Vec<u16> = "rea-group-test\0".encode_utf16().collect();
        unsafe {
            let mut wc: WNDCLASSW = std::mem::zeroed();
            wc.lpfnWndProc = Some(proc);
            wc.lpszClassName = class.as_ptr();
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!hwnd.is_null());
            std::fs::write(save_to.with_file_name("ready"), "1").unwrap();
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    }
}

#[test]
fn a_supervised_stop_closes_the_window_and_waits_for_the_save() {
    use rea_launcher_native::supervise::{self, Ended, Hold, Options};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let d = dir("supervised");
    let g = Group::spawn(&mut helper("window", &d), &Tag::new("UNUSED", "x")).unwrap();
    wait_for(&d.join("ready"));
    // the window's Stop, pressed as soon as the game runs
    let stop = AtomicBool::new(true);
    let opts = Options {
        stop: &stop,
        log: Arc::new(Mutex::new(std::io::sink())),
        grace: Duration::from_secs(20),
        sink: None,
    };
    let started = Instant::now();
    let out = supervise::run(g, &mut Hold::default(), &mut (), opts).unwrap();
    assert_eq!(out.ended, Ended::Stopped);
    assert!(out.status.success(), "killed, not closed: {:?}", out.status);
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(wait_for(&d.join("closed")), "saved");
    let _ = fs::remove_dir_all(&d);
}
