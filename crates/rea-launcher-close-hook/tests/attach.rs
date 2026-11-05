//! The hook loaded through the game's import table (`import_into`) and
//! attached by the DLL itself (`REA_CLOSE_HOOK_ATTACH=1`): nothing crosses
//! processes. Windows or Wine only (`scripts/wine-test`), headless.

#![cfg(windows)]

use std::path::PathBuf;
use std::time::Duration;

use rea_launcher_win32::process::{Launch, Running, top_level_windows};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_SYSCOMMAND};

const SC_CLOSE: usize = 0xF060;

/// `target/<triple>/debug`.
fn build_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    exe.parent().unwrap().parent().unwrap().to_path_buf()
}

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

/// A private copy of the stand-in game that imports the hook DLL, with the
/// DLL next to it, started with `config` and `attach`.
fn start(name: &str, config: &str, attach: bool) -> (Running, usize, PathBuf) {
    headless_wine_windows();
    let build = build_dir();
    let dir = build
        .join("examples")
        .join(format!("attach-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let game = std::fs::read(build.join("examples").join("hooked_game.exe")).unwrap();
    let imported = rea_close_hook::import_into(&game).unwrap();
    assert_eq!(
        rea_close_hook::import_into(&imported).unwrap(),
        imported,
        "added once"
    );
    let exe = dir.join("hooked_game.exe");
    std::fs::write(&exe, imported).unwrap();
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dll = [deps, build]
        .into_iter()
        .map(|d| d.join(rea_close_hook::DLL))
        .find(|p| p.exists())
        .expect("rea_close_hook.dll built");
    std::fs::copy(dll, dir.join(rea_close_hook::DLL)).unwrap();
    let mut launch = Launch::new(&exe).cwd(&dir).env(rea_close_hook::ENV, config);
    if attach {
        launch = launch.env(rea_close_hook::ATTACH_ENV, "1");
    }
    let game = launch.spawn().unwrap();
    let mut window = None;
    for _ in 0..200 {
        if let Some(&hwnd) = top_level_windows(game.pid()).first() {
            window = Some(hwnd as usize);
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    (game, window.expect("the game's window"), dir)
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

fn close(window: usize) {
    // SAFETY: posting to the child's window; a stale handle just fails.
    unsafe {
        PostMessageW(window as _, WM_SYSCOMMAND, SC_CLOSE, 0);
    }
}

#[test]
fn the_imported_hook_attaches_itself() {
    let (game, window, dir) = start(
        "on",
        "class=rea_hooked_game;strategy=terminate;code=5",
        true,
    );
    // The attach thread polls every 250 ms: give it time to find the window.
    std::thread::sleep(Duration::from_millis(1500));
    close(window);
    let code = ended(&game);
    let _ = game.close_gracefully(Duration::from_millis(200));
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(code, Some(5));
}

#[test]
fn without_the_attach_variable_the_dll_does_nothing() {
    // The control: the stand-in ignores SC_CLOSE by itself (as CMR04 does),
    // so without the hook it never ends with the hook's code.
    let (game, window, dir) = start(
        "off",
        "class=rea_hooked_game;strategy=terminate;code=5",
        false,
    );
    std::thread::sleep(Duration::from_millis(1500));
    close(window);
    let code = ended(&game);
    let _ = game.close_gracefully(Duration::from_millis(200));
    let _ = std::fs::remove_dir_all(dir);
    assert_ne!(code, Some(5));
}
