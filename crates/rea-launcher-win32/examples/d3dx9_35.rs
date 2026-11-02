//! A stand-in "native" d3dx9_35.dll for the DLL-override test: Wine has a
//! builtin of that name, so which one loads shows the effective override.
//! Its only export marks it as ours.

/// Present only in this native stand-in, never in Wine's builtin.
#[unsafe(no_mangle)]
pub extern "system" fn rea_native_marker() -> u32 {
    0x5EA
}
