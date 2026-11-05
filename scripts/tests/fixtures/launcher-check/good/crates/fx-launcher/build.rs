fn main() {
    rea_launcher_build::Resources::new("FX Launcher", "Launcher for FX")
        .company("rea")
        .file_name("FXLauncher.exe")
        .embed()
        .unwrap();
}
