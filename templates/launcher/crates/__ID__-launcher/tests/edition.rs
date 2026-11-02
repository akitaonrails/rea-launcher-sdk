//! Finding and identifying @GAME_EXE@ in a game folder.

use std::path::PathBuf;

use @CRATE_SNAKE@::edition::{self, EDITIONS};
use rea_launcher::identify;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("@ID@-edition-{name}-{}", std::process::id()))
        .join("Games d'Été");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_missing_folder_or_exe_is_named() {
    let dir = scratch("missing");
    let none = edition::find(&dir.join("nowhere")).unwrap_err();
    assert!(none.contains("There is no folder"), "{none}");
    let no_exe = edition::find(&dir).unwrap_err();
    assert!(no_exe.contains(edition::GAME_EXE), "{no_exe}");
}

#[test]
fn an_unknown_build_in_any_letter_case_is_refused_with_its_digest() {
    let dir = scratch("unknown");
    let bytes = b"MZ an unknown build";
    std::fs::write(dir.join(edition::GAME_EXE.to_uppercase()), bytes).unwrap();
    let refused = edition::find(&dir).unwrap_err();
    assert!(refused.contains(&identify::hex(&identify::sha256(bytes))), "{refused}");
    assert!(refused.contains(edition::UNKNOWN_EDITION_ISSUE), "{refused}");
}

#[test]
fn the_table_holds_valid_distinct_digests() {
    for known in EDITIONS {
        assert_ne!(known.sha256, [0; 32], "{}", known.value.name);
        assert_eq!(
            EDITIONS.iter().filter(|k| k.sha256 == known.sha256).count(),
            1,
            "{} is listed twice",
            known.value.name
        );
    }
}
