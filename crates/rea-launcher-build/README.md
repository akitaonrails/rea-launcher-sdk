# rea-launcher-build

`build.rs` helper for rea launchers: version information, an icon and a
manifest in the Windows executable, version information in a hook DLL.
A named, versioned program is one of the things antivirus heuristics weigh
(docs/STANDARDS.md, "Antivirus").

```rust
// build.rs
fn main() {
    rea_launcher_build::Resources::new("CMR Launcher", "Colin McRae Rally launcher")
        .company("AkitaOnRails")
        .file_name("CMRLauncher.exe")
        .icon("../../packaging/appimage/cmr-launcher.png")
        .embed()
        .unwrap();
}
```

Add `rea-launcher-build` to `[build-dependencies]`. `embed` does nothing
for non-Windows targets. For Windows it needs MinGW's `windres`
(`$WINDRES`, else `<arch>-w64-mingw32-windres`, else `windres`); the
launcher-ci MinGW packages include it. Use `.dll()` for a hook DLL (no icon
or manifest, linked into the cdylib). The version is the crate's.

Check a build: `i686-w64-mingw32-windres -i CMRLauncher.exe -O rc`.

The manifest only asks for no elevation and lists the Windows versions; it
changes nothing about DPI or code pages.
