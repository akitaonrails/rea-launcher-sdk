//! One launcher at a time (docs/STANDARDS.md, "Clean exit, single
//! instance"): a named mutex held for the launcher's lifetime. A second
//! start finds it, brings the first launcher's window forward and exits.
//!
//! ```no_run
//! # #[cfg(windows)] fn main() {
//! use rea_launcher_win32::instance::{self, SecondStart, Window};
//! use rea_launcher_win32::{process, report};
//! # let run = report::Run::default();
//! # let action = false; // --download-*, --setup: work without the window
//! # let runtime_exe = std::path::Path::new(r"C:\rt\game.exe");
//! let audience = report::audience(run);
//! let _instance = match instance::single("rea-outrun-2006") {
//!     Ok(guard) => guard,
//!     Err(running) => {
//!         // The window forward (exit 0), or "already running" (exit 5).
//!         let second = running.second_start(action, Window::Own("OutRun 2006"));
//!         if let Some(text) = second.message("OutRun 2006") {
//!             report::fatal("OutRun 2006", &text, audience.mode());
//!         }
//!         std::process::exit(second.exit_code());
//!     }
//! };
//! // Before Play: a killed launcher released the mutex, but its game may
//! // still run from the same runtime copy.
//! if !process::find_running(runtime_exe).is_empty() {
//!     let busy = SecondStart::Busy;
//!     report::fatal("OutRun 2006", &busy.message("OutRun 2006").unwrap(), audience.mode());
//!     std::process::exit(busy.exit_code());
//! }
//! // ... the launcher; the guard drops at exit
//! # }
//! # #[cfg(not(windows))] fn main() {}
//! ```
//!
//! **Find the window with [`Window::Own`].** eframe gives every window
//! winit's class (`Window Class`), so a class can't tell launchers apart,
//! and a title alone also matches an Explorer window open on a folder of
//! the same name, or the game's own window: CMR4's second start "found"
//! those and exited silently. `Own` takes a visible window with the title
//! whose process runs this launcher's exe (by file name).
//!
//! **Action flags** (`--download-hd-pack`, `--setup`, anything that does
//! work instead of opening the window) must not silently exit 0 when
//! another launcher is open: that reads as success to a script and does
//! nothing. [`SecondStart::decide`] never brings the window forward for an
//! action and always exits non-zero ([`ALREADY_RUNNING`]).
//!
//! The mutex lives in the `Local\` namespace: one instance per user
//! session. Under Wine that is per prefix (wineserver). On Linux the play
//! helper's session lock does the same job.

/// The kernel object name for `name`: `Local\rea-launcher-<name>`, with
/// characters a name can't hold (`\`) replaced.
pub fn mutex_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    format!(r"Local\rea-launcher-{clean}")
}

/// How to find the running launcher's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window<'a> {
    /// A visible top-level window with this title in a process running
    /// this launcher's exe (the same file name). What eframe launchers
    /// want: see the module docs.
    Own(&'a str),
    /// The first window with this title, in any program (an Explorer
    /// folder of the same name counts too: prefer [`Window::Own`]).
    Title(&'a str),
    /// The first window of this class (eframe windows all share winit's).
    Class(&'a str),
    /// A window with this class and this title.
    ClassTitle(&'a str, &'a str),
}

/// Whether two executable paths name the same file, as Windows compares
/// them: without case, `/` and `\` alike, a `\\?\` prefix ignored.
pub fn same_image(a: &str, b: &str) -> bool {
    fn clean(path: &str) -> String {
        let path = path.trim();
        let path = path.strip_prefix(r"\\?\").unwrap_or(path);
        path.replace('/', "\\").to_lowercase()
    }
    clean(a) == clean(b)
}

/// Whether a top-level window titled `title`, in a process whose
/// executable is `process_image` (its full path), is the running
/// launcher's window `wanted`: the title matches exactly and the process
/// runs `own_exe` (the launcher's file name, any case). A renamed launcher
/// is still found by its own file name.
pub fn is_own_window(title: &str, wanted: &str, process_image: &str, own_exe: &str) -> bool {
    let name = process_image.rsplit(['\\', '/']).next().unwrap_or_default();
    title == wanted && name.eq_ignore_ascii_case(own_exe)
}

/// The exit code of a second start that did not just bring a window
/// forward: the game or another launcher run holds the instance. Not one
/// the family uses already (1 refused, 2 bad command line, 3 window closed
/// without Play, 4 no window possible).
pub const ALREADY_RUNNING: i32 = 5;

/// What a second launcher start does (SRR's and DiRT's `instance.rs`, and
/// CMR4's, made one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondStart {
    /// The running launcher's window came forward: exit quietly (0).
    Focused,
    /// No window to bring forward: the game (or a run without a window)
    /// is in progress. Say so, refuse, exit [`ALREADY_RUNNING`]. Also the
    /// answer when the runtime's game is still running
    /// (`process::find_running`).
    Busy,
    /// An action flag found the launcher's window open: say to use the
    /// window, leave it where it is, exit [`ALREADY_RUNNING`].
    WindowOpen,
}

impl SecondStart {
    /// The decision. `action`: the run does work without the window
    /// (`--download-*`, `--setup`, `--check` when it takes the instance).
    /// `window_found` says whether the launcher's window exists without
    /// touching it (`AlreadyRunning::is_window_found`); `bring_to_front`
    /// brings it forward and says whether there was one
    /// (`AlreadyRunning::bring_to_front`). An action never brings the
    /// window forward; a plain start never only looks.
    pub fn decide(
        action: bool,
        window_found: impl FnOnce() -> bool,
        bring_to_front: impl FnOnce() -> bool,
    ) -> SecondStart {
        if action {
            if window_found() {
                SecondStart::WindowOpen
            } else {
                SecondStart::Busy
            }
        } else if bring_to_front() {
            SecondStart::Focused
        } else {
            SecondStart::Busy
        }
    }

    /// 0 for [`SecondStart::Focused`], else [`ALREADY_RUNNING`]: never a
    /// silent success for a run that did nothing.
    pub fn exit_code(self) -> i32 {
        match self {
            SecondStart::Focused => 0,
            SecondStart::Busy | SecondStart::WindowOpen => ALREADY_RUNNING,
        }
    }

    /// What to tell the player (`report::fatal` with the run's
    /// `report::Audience`), in the family's words; `None` when the window
    /// came forward. `game` is the game's name. A launcher may say it in
    /// its own words instead.
    pub fn message(self, game: &str) -> Option<String> {
        match self {
            SecondStart::Focused => None,
            SecondStart::Busy => Some(format!(
                "{game} is already running from this launcher. \
                 Quit the game first, then start the launcher again."
            )),
            SecondStart::WindowOpen => Some(format!(
                "The {game} launcher window is open. Use it there, \
                 or close it and try again."
            )),
        }
    }

    /// The line for `launcher.log`.
    pub fn log_line(self) -> &'static str {
        match self {
            SecondStart::Focused => "second start: brought the running launcher's window forward",
            SecondStart::Busy => {
                "second start: the game or another launcher run is in progress; refused"
            }
            SecondStart::WindowOpen => {
                "second start: an action while the launcher's window is open; refused"
            }
        }
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::ptr::null;

    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND,
    };
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    use windows_sys::core::BOOL;

    use super::{SecondStart, Window, is_own_window};
    use crate::wide::wide;

    /// Held while this launcher runs; dropping it lets the next one start.
    #[derive(Debug)]
    #[must_use = "the instance is released when this is dropped"]
    pub struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from CreateMutexW and is closed once.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    /// Another launcher with the same name is running.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AlreadyRunning;

    impl std::fmt::Display for AlreadyRunning {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the launcher is already running")
        }
    }

    impl std::error::Error for AlreadyRunning {}

    impl AlreadyRunning {
        /// Whether the running launcher's window exists (without focusing
        /// it); false when it is the game, or a window-less action, that
        /// holds the instance.
        pub fn is_window_found(&self, window: Window<'_>) -> bool {
            !find(window).is_null()
        }

        /// What this second start does ([`SecondStart::decide`] with
        /// [`Self::is_window_found`] and [`Self::bring_to_front`] for
        /// `window`): for a plain start the window comes forward when it
        /// exists.
        pub fn second_start(&self, action: bool, window: Window<'_>) -> SecondStart {
            SecondStart::decide(
                action,
                || self.is_window_found(window),
                || self.bring_to_front(window),
            )
        }

        /// Restores and focuses the running launcher's window; false when
        /// no such window was found (it may be the game that is running).
        pub fn bring_to_front(&self, window: Window<'_>) -> bool {
            let hwnd = find(window);
            // SAFETY: window calls on the handle FindWindowW returned.
            unsafe {
                if hwnd.is_null() {
                    return false;
                }
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetForegroundWindow(hwnd);
            }
            true
        }
    }

    /// The launcher's own file name (`OutRun2006Launcher.exe`).
    fn own_exe_name() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        Some(exe.file_name()?.to_string_lossy().into_owned())
    }

    /// A visible top-level window titled `title` whose process runs this
    /// launcher's exe.
    fn find_own(title: &str) -> HWND {
        struct Search<'a> {
            title: &'a str,
            own_exe: String,
            found: HWND,
        }
        unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
            // SAFETY: `data` is the Search below, alive for the
            // enumeration; the title buffer outlives GetWindowTextW.
            unsafe {
                let search = &mut *(data as *mut Search);
                if IsWindowVisible(hwnd) == 0 {
                    return 1;
                }
                let mut text = [0u16; 512];
                let len = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
                let text = String::from_utf16_lossy(&text[..len.max(0) as usize]);
                if text != search.title {
                    return 1;
                }
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                match crate::process::image_path(pid) {
                    Some(image) if is_own_window(&text, search.title, &image, &search.own_exe) => {
                        search.found = hwnd;
                        0
                    }
                    _ => 1,
                }
            }
        }
        let Some(own_exe) = own_exe_name() else {
            return std::ptr::null_mut();
        };
        let mut search = Search {
            title,
            own_exe,
            found: std::ptr::null_mut(),
        };
        // SAFETY: the callback only touches `search` during this call.
        unsafe {
            EnumWindows(Some(visit), (&raw mut search) as LPARAM);
        }
        search.found
    }

    fn find(window: Window<'_>) -> HWND {
        let (class, title) = match window {
            Window::Own(title) => return find_own(title),
            Window::Title(title) => (None, Some(wide(title))),
            Window::Class(class) => (Some(wide(class)), None),
            Window::ClassTitle(class, title) => (Some(wide(class)), Some(wide(title))),
        };
        // SAFETY: NUL-terminated strings (or null) that outlive the call.
        unsafe {
            FindWindowW(
                class.as_ref().map_or(null(), |c| c.as_ptr()),
                title.as_ref().map_or(null(), |t| t.as_ptr()),
            )
        }
    }

    /// Takes the instance `name` (see [`super::mutex_name`]), or reports
    /// that another launcher holds it. When the mutex can't be created at
    /// all the launcher runs unguarded rather than refusing to start.
    pub fn single(name: &str) -> Result<Guard, AlreadyRunning> {
        match create(name) {
            Ok(guard) => Ok(guard),
            Err(Some(running)) => Err(running),
            // Could not create the object at all: run unguarded rather
            // than refuse to start.
            Err(None) => Ok(Guard(std::ptr::null_mut())),
        }
    }

    fn create(name: &str) -> Result<Guard, Option<AlreadyRunning>> {
        let name = wide(super::mutex_name(name));
        // SAFETY: a NUL-terminated name and default security; the handle
        // is owned by the Guard (or closed right away when not ours).
        unsafe {
            let handle = CreateMutexW(null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err(None);
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return Err(Some(AlreadyRunning));
            }
            Ok(Guard(handle))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_start_brings_the_window_forward_or_refuses() {
        use std::cell::Cell;
        // A plain start: the window comes forward, no message, exit 0.
        let asked = Cell::new(0);
        let second = SecondStart::decide(
            false,
            || unreachable!("a plain start brings the window forward"),
            || {
                asked.set(asked.get() + 1);
                true
            },
        );
        assert_eq!((second, asked.get()), (SecondStart::Focused, 1));
        assert_eq!((second.exit_code(), second.message("Game")), (0, None));
        // No window: the game is running. Refused, non-zero, said.
        let second = SecondStart::decide(false, || unreachable!(), || false);
        assert_eq!(second, SecondStart::Busy);
        assert_eq!(second.exit_code(), ALREADY_RUNNING);
        let text = second.message("Sega Rally Revo").unwrap();
        assert!(
            text.starts_with("Sega Rally Revo is already running"),
            "{text}"
        );
        assert!(second.log_line().contains("refused"));
    }

    #[test]
    fn an_action_never_focuses_the_window_and_never_exits_0() {
        let second = SecondStart::decide(true, || true, || unreachable!("an action"));
        assert_eq!(second, SecondStart::WindowOpen);
        assert_ne!(second.exit_code(), 0);
        assert!(second.message("DiRT").unwrap().contains("window is open"));
        let second = SecondStart::decide(true, || false, || unreachable!("an action"));
        assert_eq!(second, SecondStart::Busy);
        assert_ne!(second.exit_code(), 0);
        // Not a code the family already uses.
        assert!(!(0..=4).contains(&ALREADY_RUNNING));
    }

    #[test]
    fn only_the_launchers_own_window_counts() {
        let title = "Colin McRae Rally 04";
        let launcher = r"C:\Games\CMR4 Launcher\CMR4Launcher.exe";
        assert!(is_own_window(title, title, launcher, "CMR4Launcher.exe"));
        assert!(is_own_window(title, title, launcher, "cmr4launcher.EXE"));
        assert!(is_own_window(
            title,
            title,
            "Z:/home/p/cmr4.launcher.exe",
            "cmr4.launcher.exe"
        ));
        // Same title, other programs: an Explorer folder window, the game.
        for other in [r"C:\Windows\explorer.exe", r"C:\Games\CMR4\cmr4.exe"] {
            assert!(!is_own_window(title, title, other, "CMR4Launcher.exe"));
        }
        // The launcher's other windows (a file dialog) don't count.
        assert!(!is_own_window(
            "Choose the game folder",
            title,
            launcher,
            "CMR4Launcher.exe"
        ));
    }

    #[test]
    fn images_compare_as_windows_does() {
        let exe = r"C:\users\José\AppData\Local\OR\runtime\OR2006C2C.EXE";
        assert!(same_image(
            exe,
            r"c:\Users\josé\appdata\local\or\runtime\or2006c2c.exe"
        ));
        assert!(same_image(
            exe,
            r"\\?\C:\users\José\AppData\Local\OR\runtime\OR2006C2C.EXE"
        ));
        assert!(same_image("Z:/games/rt/game.exe", r"z:\games\rt\game.exe"));
        // Another copy of the same game is another runtime.
        assert!(!same_image(r"C:\rt\game.exe", r"D:\rt\game.exe"));
        assert!(!same_image(r"C:\rt\game.exe", r"C:\rt\game.exe.bak"));
    }

    #[test]
    fn names_are_local_and_clean() {
        assert_eq!(mutex_name("rea-cmr4"), r"Local\rea-launcher-rea-cmr4");
        assert_eq!(mutex_name(r"a\b"), r"Local\rea-launcher-a_b");
    }

    /// Windows/Wine: a second acquisition fails until the first is dropped.
    #[cfg(windows)]
    #[test]
    fn one_instance_at_a_time() {
        let name = format!("test-{}", std::process::id());
        let first = single(&name).expect("first instance");
        assert_eq!(single(&name).unwrap_err(), AlreadyRunning);
        // Also from another thread (a second handle).
        let from_thread = {
            let name = name.clone();
            std::thread::spawn(move || single(&name).is_err())
                .join()
                .unwrap()
        };
        assert!(from_thread);
        drop(first);
        let again = single(&name).expect("free after the first exits");
        drop(again);
        assert!(!AlreadyRunning.bring_to_front(Window::Title("no such window rea-test")));
        assert!(!AlreadyRunning.is_window_found(Window::Class("no_such_class_rea_test")));
        assert!(!AlreadyRunning.is_window_found(Window::Own("no such window rea-test")));
    }

    /// Windows/Wine (headless: Wine's null graphics driver in the test
    /// prefix): a visible window of this process is "the launcher's own"
    /// (the test binary is the launcher here); a class or a title that
    /// differs is not.
    #[cfg(windows)]
    #[test]
    fn the_own_window_is_found_by_title_and_exe() {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, WNDCLASSW, WS_VISIBLE,
        };
        let class = crate::wide::wide("rea_instance_test_class");
        let title_text = format!("rea instance test {}", std::process::id());
        let title = crate::wide::wide(&title_text);
        // SAFETY: a window class and window on this thread; every pointer
        // refers to a live local.
        let hwnd = unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                hInstance: GetModuleHandleW(null()),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_VISIBLE,
                0,
                0,
                100,
                100,
                null_mut(),
                null_mut(),
                wc.hInstance,
                null(),
            )
        };
        assert!(!hwnd.is_null(), "a test window");
        let found = |window| AlreadyRunning.is_window_found(window);
        assert!(found(Window::Own(&title_text)));
        assert!(found(Window::ClassTitle(
            "rea_instance_test_class",
            &title_text
        )));
        assert!(found(Window::Title(&title_text)));
        assert!(!found(Window::ClassTitle("Window Class", &title_text)));
        assert!(!found(Window::Own("rea instance test, another title")));
        // A plain second start brings it forward.
        assert_eq!(
            AlreadyRunning.second_start(false, Window::Own(&title_text)),
            SecondStart::Focused
        );
        assert_eq!(
            AlreadyRunning.second_start(true, Window::Own(&title_text)),
            SecondStart::WindowOpen
        );
        // SAFETY: the window this test created, on its thread.
        unsafe {
            DestroyWindow(hwnd);
        }
        assert!(!found(Window::Own(&title_text)));
        assert_eq!(
            AlreadyRunning.second_start(false, Window::Own(&title_text)),
            SecondStart::Busy
        );
    }

    /// Windows/Wine: this test binary is found running from its own path,
    /// and nothing runs from a path next to it.
    #[cfg(windows)]
    #[test]
    fn a_running_process_is_found_by_its_image_path() {
        let me = std::env::current_exe().unwrap();
        let image = crate::process::image_path(std::process::id()).expect("own image path");
        assert!(
            same_image(&image, &me.to_string_lossy()),
            "{image} vs {me:?}"
        );
        let found = crate::process::find_running(&me);
        assert!(found.contains(&std::process::id()), "{found:?}");
        let other = me.with_file_name("rea-no-such-game.exe");
        assert!(crate::process::find_running(&other).is_empty());
        // The same file name in another folder is another runtime copy.
        let elsewhere = me
            .parent()
            .unwrap()
            .join("elsewhere")
            .join(me.file_name().unwrap());
        assert!(!crate::process::find_running(&elsewhere).contains(&std::process::id()));
    }
}
