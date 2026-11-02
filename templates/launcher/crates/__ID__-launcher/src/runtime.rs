//! The private runtime copy of the game folder (`rea_launcher::runtime`).
//!
//! The player's folder is only read. The launcher brings its files into a
//! runtime folder of its own and changes only that copy: every write there
//! is a whole-file swap (`rea_launcher::fsx`). [`GamePolicy`] says which
//! files are large read-only data (hard-linked when the disk allows),
//! which the game writes itself (never overwritten once present: saves,
//! its own settings) and which the launcher places (components, generated
//! configuration). Fill it in from Phase 2 of the playbook.

use std::io;
use std::path::Path;

use rea_launcher::paths;
use rea_launcher::runtime::{self, Policy, Stamp, SyncReport};

/// The runtime's record of what it brought over.
pub const MANIFEST: &str = ".@ID@-manifest";

/// What @GAME@ does with its files.
pub struct GamePolicy;

impl Policy for GamePolicy {
    /// Static data the game only reads (never anything it opens for
    /// writing). Nothing yet: everything is copied.
    fn link(&self, _rel: &str) -> bool {
        false
    }

    /// Files the game writes: saves, its own settings. Copied once, then
    /// left as the game keeps them. Nothing known yet.
    fn game_written(&self, _rel: &str, _source: Stamp) -> bool {
        false
    }

    /// Files the launcher places itself. Nothing yet.
    fn owned(&self, _rel: &str, _source: Stamp) -> bool {
        false
    }
}

/// Brings the game folder into `runtime`, which may never overlap it.
pub fn prepare(game: &Path, runtime: &Path) -> Result<SyncReport, String> {
    paths::check_separate(game, runtime)?;
    sync(game, runtime).map_err(|error| format!("Cannot prepare {}: {error}", runtime.display()))
}

fn sync(game: &Path, runtime: &Path) -> io::Result<SyncReport> {
    runtime::sync(game, runtime, &GamePolicy, MANIFEST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_game_is_copied_and_never_changed() {
        let root = std::env::temp_dir().join(format!("@ID@-runtime-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (game, rt) = (root.join("Jogos d'Ação"), root.join("runtime"));
        fs::create_dir_all(game.join("data")).unwrap();
        fs::write(game.join("@GAME_EXE@"), b"exe").unwrap();
        fs::write(game.join("data/a.bin"), b"data").unwrap();

        let report = prepare(&game, &rt).unwrap();
        assert_eq!(report.brought, 2);
        assert_eq!(fs::read(rt.join("data/a.bin")).unwrap(), b"data");
        // A second run brings nothing new.
        assert_eq!(prepare(&game, &rt).unwrap().brought, 0);
        assert_eq!(fs::read(game.join("@GAME_EXE@")).unwrap(), b"exe");

        // A runtime inside the game folder is refused.
        assert!(prepare(&game, &game.join("runtime")).is_err());
        fs::remove_dir_all(&root).unwrap();
    }
}
