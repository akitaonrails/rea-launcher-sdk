//! FFI that only native launchers need (an emulator wrapper, a native
//! port): a child process that dies with the launcher, and SDL2 wheels,
//! pads, haptics and rumble. From rea-rad-mobile's `child.rs` and
//! `input/{sdl,haptic}.rs`.
//!
//! Features: `child` (default) for [`child::spawn_tied`]; `sdl` for
//! [`sdl`] (links SDL2). Each `unsafe` block sits inside a safe type; the
//! decisions around it (rescan throttling, haptic retries, rumble rate,
//! effect directions) are pure and tested in [`policy`]. The Wine-run
//! launchers (i686) need none of this: they keep `rea-launcher-win32`.

#![doc = include_str!("../README.md")]

#[cfg(feature = "child")]
pub mod child;
pub mod policy;
#[cfg(feature = "sdl")]
pub mod sdl;
