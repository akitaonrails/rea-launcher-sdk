//! Version information in the hook DLL: what it is and who made it, so
//! Windows (and antivirus heuristics) see a named, versioned library.
fn main() {
    rea_launcher_build::Resources::new(
        "rea-launcher close hook",
        "Makes an old game honour Alt+F4 and window close (rea launchers)",
    )
    .company("AkitaOnRails")
    .file_name("rea_close_hook.dll")
    .dll()
    .embed()
    .unwrap();
}
