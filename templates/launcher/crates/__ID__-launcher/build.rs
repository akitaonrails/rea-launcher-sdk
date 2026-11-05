//! The Windows executable's version information, icon and manifest
//! (rea-launcher-build): a named, versioned program with the launcher's
//! icon, instead of an anonymous one that antivirus heuristics distrust.
fn main() {
    rea_launcher_build::Resources::new("@LAUNCHER_NAME@", "Launcher for @GAME@ on modern systems")
        .company("@AUTHOR@")
        .file_name("@LAUNCHER_EXE@")
        .icon("../../packaging/appimage/@ID@-launcher.png")
        .embed()
        .unwrap();
}
