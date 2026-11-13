//! The layer inside the game: `DllMain` and the import ([`hook`]), the
//! `IDirectInput8A` wrapper ([`di8`]), the virtual wheel ([`device`]), its
//! effects ([`effect`]), the real devices behind it ([`real`]) and the
//! test-only fakes ([`fake`]).
//!
//! Every COM entry runs inside [`guard`]: a panic becomes an error code
//! (and a log line), never an unwind into the game.

mod com;
mod device;
mod di8;
mod effect;
mod fake;
mod hook;
mod real;

use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use windows_sys::core::GUID;

use crate::Config;
use crate::log::Log;

/// `DIERR_*` and `DI_*` codes the layer returns.
mod hr {
    pub const OK: i32 = 0;
    /// `DI_NOEFFECT`, `DI_BUFFEROVERFLOW`, `S_FALSE`.
    pub const FALSE: i32 = 1;
    pub const GENERIC: i32 = 0x8000_4005_u32 as i32;
    pub const UNSUPPORTED: i32 = 0x8000_4001_u32 as i32;
    pub const INVALIDPARAM: i32 = 0x8007_0057_u32 as i32;
    pub const NOTFOUND: i32 = 0x8007_0002_u32 as i32;
    pub const NOTACQUIRED: i32 = 0x8007_000C_u32 as i32;
    pub const INPUTLOST: i32 = 0x8007_001E_u32 as i32;
    pub const ACQUIRED: i32 = 0x8007_00AA_u32 as i32;
    pub const DEVICENOTREG: i32 = 0x8004_0154_u32 as i32;
    pub const MOREDATA: i32 = 0x8004_0202_u32 as i32;
    pub const NOTDOWNLOADED: i32 = 0x8004_0203_u32 as i32;
    pub const NOTBUFFERED: i32 = 0x8004_0207_u32 as i32;

    pub fn ok(hr: i32) -> bool {
        hr >= 0
    }
}

/// Runs a COM entry; a panic answers `failed` instead of unwinding into
/// the game.
fn guard<R>(failed: R, f: impl FnOnce() -> R) -> R {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        log_line("internal error (panic): answered with an error");
        failed
    })
}

/// A mutex that stays usable after a panic inside [`guard`].
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// This DLL's folder.
fn dll_dir() -> Option<PathBuf> {
    let mut module: HMODULE = null_mut();
    let mut name = [0u16; 1024];
    // SAFETY: looks up the module containing this function (no reference
    // taken) and copies its path into a local buffer of the given size.
    let len = unsafe {
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            dll_dir as *const () as *const u16,
            &mut module,
        ) == 0
        {
            return None;
        }
        GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32)
    };
    let path = PathBuf::from(String::from_utf16_lossy(&name[..len as usize]));
    path.parent().map(PathBuf::from)
}

/// The settings, read once: `rea_wheel.ini` next to the DLL and
/// [`crate::MODE_ENV`]. Opens the log when they ask for it.
fn config() -> &'static Config {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let dir = dll_dir();
        let text = dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(rea_launcher::wheel::dinput::FILE)).ok())
            .map(rea_launcher::cfg::decode);
        let env = std::env::var(crate::MODE_ENV).ok();
        let config = Config::read(text.as_deref(), env.as_deref());
        if config.setup.log
            && let Some(dir) = dir
        {
            *lock(&LOG) = Some(Log::create(&dir.join(crate::log::FILE)));
            LOGGING.store(true, Ordering::Release);
            log_line(&format!(
                "rea_dinput {}: mode {} ({} {}), settings {}",
                env!("CARGO_PKG_VERSION"),
                config.mode.key(),
                crate::MODE_ENV,
                env.as_deref().unwrap_or("unset"),
                if text.is_some() { "read" } else { "missing" },
            ));
        }
        config
    })
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);
static LOGGING: AtomicBool = AtomicBool::new(false);

fn logging() -> bool {
    LOGGING.load(Ordering::Acquire)
}

fn log_line(text: &str) {
    if logging()
        && let Some(log) = lock(&LOG).as_mut()
    {
        log.line(text);
    }
}

/// Logs a line built only when the log is on.
macro_rules! log {
    ($($arg:tt)*) => {
        if $crate::win::logging() {
            $crate::win::log_line(&format!($($arg)*));
        }
    };
}
use log;

/// `{xxxxxxxx-…}` as the settings keep it.
fn guid_text(g: &GUID) -> String {
    rea_launcher::wheel::rig::guid_text(g.data1, g.data2, g.data3, g.data4)
}

/// `text` into a fixed, NUL-terminated ANSI buffer, cut to fit.
fn ansi(text: &str, buffer: &mut [i8]) {
    buffer.fill(0);
    let len = text.len().min(buffer.len().saturating_sub(1));
    for (dst, src) in buffer.iter_mut().zip(&text.as_bytes()[..len]) {
        *dst = *src as i8;
    }
}

/// A NUL-terminated ANSI buffer as text.
fn from_ansi(buffer: &[i8]) -> String {
    let bytes: Vec<u8> = buffer
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_owned()
}
