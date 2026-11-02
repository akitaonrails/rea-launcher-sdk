//! Which builds of @GAME_EXE@ this launcher knows, by SHA-256
//! (`rea_launcher::identify`).
//!
//! Every edition the owner has (retail, GOG, Steam, regional, patched) gets
//! a row in [`EDITIONS`], from the hashes taken in the playbook's Phase 2.
//! Any other build is refused with its digest and the link to the
//! "unknown edition" issue template, so players can report it
//! (docs/STANDARDS.md, "Every edition"). Nothing is ever patched by
//! guesswork.

use std::path::{Path, PathBuf};

use rea_launcher::identify::{self, Known};
use rea_launcher::paths;

/// The game's executable, as shipped (found in any letter case).
pub const GAME_EXE: &str = "@GAME_EXE@";

/// Where players report a build the launcher does not know.
pub const UNKNOWN_EDITION_ISSUE: &str =
    "https://github.com/@REPO@/issues/new?template=unknown-edition.yml";

/// What the launcher knows about one build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edition {
    /// For the log and the window: "PC v1.1 (retail)".
    pub name: &'static str,
}

/// Every known build of [`GAME_EXE`]. Empty until Phase 2 records the
/// owner's editions, so every build is refused for now. A row looks like:
///
/// ```text
/// Known::new(
///     "<64 hex digits: sha256sum of the exe>",
///     Edition { name: "PC v1.1 (retail)" },
/// ),
/// ```
pub static EDITIONS: &[Known<Edition>] = &[];

/// The build of these exe bytes, or their digest when it is unknown.
pub fn identify(bytes: &[u8]) -> Result<&'static Edition, [u8; 32]> {
    identify::identify(bytes, EDITIONS)
}

/// The message for a build that is not in [`EDITIONS`]: what happened, the
/// SHA-256 and where to report it.
pub fn unknown_message(digest: &[u8; 32]) -> String {
    format!(
        "This {GAME_EXE} is not a build this launcher knows, so it was left alone.\n\n\
         SHA-256: {}\n\n\
         Please report it, with that SHA-256 and where your copy of the game came \
         from, so it can be supported:\n{UNKNOWN_EDITION_ISSUE}",
        identify::hex(digest)
    )
}

/// The game's executable in a folder, and its known build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The exe, spelled as it is on disk.
    pub exe: PathBuf,
    pub edition: &'static Edition,
}

/// Looks for [`GAME_EXE`] in `dir` and identifies it. The error is a
/// message for the player: no such folder, no exe, or an unknown build
/// ([`unknown_message`]).
pub fn find(dir: &Path) -> Result<Found, String> {
    if !dir.is_dir() {
        return Err(format!("There is no folder {}.", dir.display()));
    }
    let exe = paths::find_ignoring_case(dir, GAME_EXE)
        .filter(|exe| exe.is_file())
        .ok_or_else(|| format!("There is no {GAME_EXE} in {}.", dir.display()))?;
    let digest = identify::sha256_file(&exe)
        .map_err(|error| format!("Cannot read {}: {error}", exe.display()))?;
    match identify::identify_digest(digest, EDITIONS) {
        Ok(edition) => Ok(Found { exe, edition }),
        Err(digest) => Err(unknown_message(&digest)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_build_has_its_own_digest() {
        for (i, known) in EDITIONS.iter().enumerate() {
            assert!(!known.value.name.is_empty());
            assert!(
                EDITIONS[i + 1..].iter().all(|k| k.sha256 != known.sha256),
                "{} is listed twice",
                known.value.name
            );
            assert_eq!(identify::identify_digest(known.sha256, EDITIONS), Ok(&known.value));
        }
    }

    #[test]
    fn an_unknown_build_is_refused_with_its_digest_and_the_issue_link() {
        let digest = identify::sha256(b"not the game");
        assert_eq!(identify(b"not the game"), Err(digest));
        let message = unknown_message(&digest);
        assert!(message.contains(&identify::hex(&digest)), "{message}");
        assert!(message.contains(UNKNOWN_EDITION_ISSUE), "{message}");
        assert!(UNKNOWN_EDITION_ISSUE.starts_with("https://github.com/"));
    }
}
