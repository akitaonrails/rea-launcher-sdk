//! Version information in the DLL: what it is and who made it, so Windows
//! (and antivirus heuristics) see a named, versioned library.
fn main() {
    rea_launcher_build::Resources::new(
        "rea-launcher DirectInput wheel layer",
        "Shows an old game one virtual wheel made from the player's wheel and pedals (rea launchers)",
    )
    .company("AkitaOnRails")
    .file_name("rea_dinput.dll")
    .dll()
    .embed()
    .unwrap();
}
