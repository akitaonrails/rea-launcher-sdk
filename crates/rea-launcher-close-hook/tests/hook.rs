//! The hook installed into a stand-in game (`examples/hooked_game.rs`)
//! through `rea_launcher_win32::process::Running::message_hook`, one
//! strategy per test. Windows or Wine only (`scripts/wine-test`), headless.

#![cfg(windows)]

use std::path::PathBuf;
use std::time::Duration;

use rea_launcher_win32::process::{Closed, Launch, Running, top_level_windows};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_SYSCOMMAND, WM_SYSKEYDOWN};

const SC_CLOSE: usize = 0xF060;
const VK_F4: usize = 0x73;
const ALT_DOWN: isize = 1 << 29;

/// `target/<triple>/debug`: the test runs from its `deps` folder.
fn build_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().to_path_buf()
}

/// Headless Wine creates windows only through its null graphics driver.
fn headless_wine_windows() {
    let headless = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .all(|name| std::env::var_os(name).is_none());
    if rea_launcher_win32::wine::is_wine() && headless {
        use rea_launcher_win32::reg::{Hive, Key};
        Key::create(Hive::CurrentUser, r"Software\Wine\Drivers")
            .and_then(|key| key.set_string_if_missing("Graphics", "null"))
            .unwrap();
    }
}

/// Starts the stand-in game with `config` and the hook; returns it with
/// its window.
fn game(config: &str) -> (Running, usize, rea_launcher_win32::process::MessageHook) {
    headless_wine_windows();
    let dir = build_dir();
    let game = Launch::new(dir.join("examples").join("hooked_game.exe"))
        .env(rea_close_hook::ENV, config)
        .spawn()
        .unwrap();
    let mut window = None;
    for _ in 0..200 {
        if let Some(&hwnd) = top_level_windows(game.pid()).first() {
            window = Some(hwnd as usize);
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let window = window.expect("the game's window");
    // Cargo leaves the test build's DLL next to the test (`deps`), a plain
    // build's in `debug`.
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = [deps, dir]
        .into_iter()
        .map(|d| d.join("rea_close_hook.dll"))
        .find(|p| p.exists())
        .expect("rea_close_hook.dll built");
    let hook = game.message_hook(&dll, c"close_hook").unwrap();
    (game, window, hook)
}

fn post(window: usize, message: u32, wparam: usize, lparam: isize) {
    // SAFETY: posting to the child's window; a stale handle just fails.
    unsafe {
        PostMessageW(window as _, message, wparam, lparam);
    }
}

/// Waits for the game to end on its own; its exit code.
fn ended(game: &Running) -> Option<u32> {
    for _ in 0..200 {
        if let Some(code) = game.exit_code() {
            return Some(code);
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    None
}

#[test]
fn terminate_on_sc_close() {
    let (game, window, _hook) = game("class=rea_hooked_game;strategy=terminate;code=5");
    post(window, WM_SYSCOMMAND, SC_CLOSE, 0);
    assert_eq!(ended(&game), Some(5));
}

#[test]
fn exit_on_alt_f4() {
    let (game, window, _hook) = game("class=REA_HOOKED_GAME;strategy=exit;code=7");
    post(window, WM_SYSKEYDOWN, VK_F4, ALT_DOWN);
    assert_eq!(ended(&game), Some(7));
}

#[test]
fn quit_message_ends_the_loop() {
    let (game, window, _hook) = game("strategy=quit;code=6");
    post(window, WM_SYSCOMMAND, SC_CLOSE, 0);
    assert_eq!(ended(&game), Some(6));
}

#[test]
fn posted_quit_ends_the_loop() {
    let (game, window, _hook) = game("strategy=post_quit;code=8");
    post(window, WM_SYSCOMMAND, SC_CLOSE, 0);
    assert_eq!(ended(&game), Some(8));
}

#[test]
fn posted_keys_reach_the_games_menu() {
    // The stand-in quits with 4 on Escape, like a game's Quit key.
    let (game, window, _hook) = game("strategy=post_keys: 50, escape");
    post(window, WM_SYSCOMMAND, SC_CLOSE, 0);
    assert_eq!(ended(&game), Some(4));
}

#[test]
fn another_window_class_is_left_alone() {
    let (game, window, _hook) = game("class=SOME_OTHER_GAME;strategy=terminate;code=5");
    post(window, WM_SYSCOMMAND, SC_CLOSE, 0);
    std::thread::sleep(Duration::from_millis(300));
    assert!(game.is_alive(), "not the configured window");
    assert_eq!(
        game.close_gracefully(Duration::from_millis(200)).unwrap(),
        Closed::Terminated { windows: 1 }
    );
}
