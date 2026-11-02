//! GPU preferences, each written only when the player has none.
//!
//! - [`prefer_fast_gpu`]: Windows' per-program "High performance" choice
//!   (`GpuPreference=2;` under `UserGpuPreferences`), for laptops with two
//!   GPUs. Same in CMR3/CMR4/SRR/OR; under Wine the Linux helper's GPU
//!   environment does this instead.
//! - [`prefer_glx`]: Wine `UseEGL = N`. Wine 11 draws OpenGL through EGL by
//!   default, and where the driver has no EGL for X11 (NVIDIA without
//!   egl-x11) that falls back to software; GLX reaches the GPU. The settings
//!   window is the only OpenGL in these launchers (CMR3's note).

use std::path::Path;

use crate::reg::{Error, Hive, Key};

/// Selects the high-performance GPU for `exe` unless a choice exists;
/// true when it wrote.
pub fn prefer_fast_gpu(exe: &Path) -> Result<bool, Error> {
    Key::create(
        Hive::CurrentUser,
        r"Software\Microsoft\DirectX\UserGpuPreferences",
    )?
    .set_string_if_missing(&exe.display().to_string(), "GpuPreference=2;")
}

/// Under Wine: OpenGL through GLX unless `UseEGL` is set; true when it wrote.
pub fn prefer_glx() -> Result<bool, Error> {
    Key::create(Hive::CurrentUser, r"Software\Wine\X11 Driver")?
        .set_string_if_missing("UseEGL", "N")
}
