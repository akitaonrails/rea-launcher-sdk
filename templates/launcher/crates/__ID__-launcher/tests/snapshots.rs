//! Golden snapshots (docs/STANDARDS.md, "Golden snapshots"): everything the
//! launcher produces, compared with the files in `tests/snapshots/`, so an
//! SDK bump or a refactor can't change it silently. A diff is reviewed like
//! code: it must match an intended change or an SDK Migration item.
//!
//! `REA_UPDATE_SNAPSHOTS=1 cargo test --test snapshots` rewrites them; the
//! SDK helper stores this crate's version as `<version>`, so a release bump
//! never breaks them. Add a snapshot for every file, registry value,
//! environment variable and argument list a look writes as the launcher
//! grows (the plan of each look).

use @CRATE_SNAKE@::config::{self, Look};

/// The names the scaffold filled in, masked: the snapshots stay the same
/// whatever the game is called.
fn masked(text: &str) -> String {
    text.replace("@LAUNCHER_EXE@", "<launcher>")
        .replace("linux/@ID@-play", "linux/<id>-play")
        .replace("@GAME@", "<game>")
}

/// `launcher.cfg` text without its heading (the comment block before the
/// first option, which names the game).
fn body(text: &str) -> &str {
    text.split_once("\n\n").map_or(text, |(_, rest)| rest)
}

#[test]
fn help() {
    rea_launcher::snapshot!("help.txt", masked(&config::usage()));
}

#[test]
fn the_launcher_cfg_template() {
    rea_launcher::snapshot!("launcher.cfg.txt", body(&config::template()));
}

#[test]
fn the_first_run_saves_the_recommended_look() {
    let text = config::first_run_text(Some((2560, 1440)));
    rea_launcher::snapshot!("first-run-2560x1440.cfg.txt", body(&text));
}

/// What each look sets, on the screens players have.
#[test]
fn every_look_on_every_screen() {
    let mut out = String::new();
    for screen in [None, Some((1920, 1080)), Some((2560, 1440)), Some((1280, 800))] {
        for look in Look::ALL {
            let name = screen.map_or("unknown".to_owned(), |(w, h)| format!("{w}x{h}"));
            let picture = look.picture(screen);
            out.push_str(&format!("{name} {}: {}\n", look.key(), picture.describe()));
        }
    }
    rea_launcher::snapshot!("looks.txt", out);
}
