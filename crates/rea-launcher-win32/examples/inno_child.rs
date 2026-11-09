//! A stand-in for innoextract in the unpack tests (`innoextract::tests`):
//! it takes innoextract's arguments, writes into `--output-dir`, and does
//! what `REA_INNO_CHILD` says:
//!
//!   ok    writes a small game (with a collision and a repack's DLL), exit 0
//!   fail  exit 3, nothing written
//!   slow  writes 200 bytes, then sleeps a minute (for Cancel)

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args
        .iter()
        .position(|arg| arg == "--output-dir")
        .and_then(|at| args.get(at + 1))
        .map(PathBuf::from)
        .expect("--output-dir");
    let app = out.join("app");
    std::fs::create_dir_all(&app).unwrap();
    match std::env::var("REA_INNO_CHILD").as_deref() {
        Ok("ok") => {
            for (name, bytes) in [
                ("game.exe", &b"good"[..]),
                ("p206.big", b"livery"),
                ("p206.big#base", b"default"),
                ("dinput8.dll", b"repack's wrapper"),
            ] {
                std::fs::write(app.join(name), bytes).unwrap();
            }
        }
        Ok("slow") => {
            std::fs::write(app.join("part.big"), [0u8; 200]).unwrap();
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        _ => std::process::exit(3),
    }
}
