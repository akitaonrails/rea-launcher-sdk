//! Win32 helpers shared by the rea launchers.
//!
//! Every module that calls Windows is `#[cfg(windows)]`; on other targets
//! the crate only carries [`reg`]'s pure parts. The pure helpers these
//! modules build on (argument quoting, environment and DLL-override
//! merging, monitor choice, XInput state parsing) live in the portable
//! `rea_launcher` crate, tested on the host. All `unsafe` of a launcher
//! lives here.
//!
//! The launchers are 32-bit (`i686-pc-windows-gnu`) and usually run under
//! Wine, so every helper is written to behave the same there.

pub mod console;
pub mod http;
mod latest;
pub mod reg;
pub mod report;

#[cfg(windows)]
pub mod dialog;
#[cfg(windows)]
pub mod disk;
#[cfg(windows)]
pub mod display;
#[cfg(windows)]
pub mod gpu;
#[cfg(windows)]
pub mod innoextract;
pub mod instance;
#[cfg(windows)]
pub mod process;
#[cfg(windows)]
pub mod short_path;
#[cfg(windows)]
pub mod wide;
pub mod wine;
#[cfg(windows)]
pub mod xinput;
