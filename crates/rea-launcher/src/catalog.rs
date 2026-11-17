//! A hub launcher's games as data: what each one is, which builds of it
//! are known, and whether it can start on this system.
//!
//! From the TeknoParrot hub launchers' `catalog.rs` (Initial D, Nintendo,
//! Sega, Wangan): a game is recognised by its main file, either the
//! SHA-256 of the whole file or, for a GD-ROM, the SHA-1 its CHD header
//! records for the disc (Initial D). Only builds listed are launched;
//! anything else is reported with its digest (STANDARDS: "Exact binary
//! identity"). The game table itself, its routes and its per-game rules stay
//! in the launcher, which implements [`Title`] for its own game type.

use std::fmt::Debug;
use std::hash::Hash;

/// Whether the launcher can start a game today, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// Verified in the real game (a race played).
    Ready,
    /// Starts, but nothing was verified yet; the text says what is known.
    Unverified(&'static str),
    /// Known not to work yet; the text says why, for the player.
    Blocked(&'static str),
}

/// The system the launcher runs on: what it can start depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Linux,
    Windows,
}

impl Host {
    /// The system this build of the launcher is for.
    pub const fn current() -> Host {
        if cfg!(windows) {
            Host::Windows
        } else {
            Host::Linux
        }
    }
}

/// What identifies an edition: a lower- or upper-case hex digest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ident {
    /// SHA-256 of the main file (64 hex digits).
    Sha256(&'static str),
    /// The data SHA-1 in a CHD v5 header (40 hex digits;
    /// [`crate::library::chd_sha1`]).
    ChdSha1(&'static str),
}

impl Ident {
    pub fn digest(self) -> &'static str {
        match self {
            Ident::Sha256(d) | Ident::ChdSha1(d) => d,
        }
    }

    /// How a file is hashed for this kind of identity.
    pub fn kind(self) -> IdentKind {
        match self {
            Ident::Sha256(_) => IdentKind::Sha256,
            Ident::ChdSha1(_) => IdentKind::ChdSha1,
        }
    }

    /// Whether `digest` (any case) is this identity.
    pub fn matches(self, digest: &str) -> bool {
        self.digest().eq_ignore_ascii_case(digest)
    }

    /// The digest is hex of the kind's length.
    pub fn well_formed(self) -> bool {
        let len = match self {
            Ident::Sha256(_) => 64,
            Ident::ChdSha1(_) => 40,
        };
        let d = self.digest();
        d.len() == len && d.bytes().all(|c| c.is_ascii_hexdigit())
    }
}

/// How a main file is hashed ([`Ident`] without its digest).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IdentKind {
    Sha256,
    ChdSha1,
}

/// One known build of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edition {
    pub ident: Ident,
    /// The build's name for the player (`Export Rev. D (1.5)`).
    pub label: &'static str,
}

impl Edition {
    /// A build known by its main file's SHA-256.
    pub const fn sha256(digest: &'static str, label: &'static str) -> Edition {
        Edition {
            ident: Ident::Sha256(digest),
            label,
        }
    }

    /// A GD-ROM known by its CHD header's SHA-1.
    pub const fn chd_sha1(digest: &'static str, label: &'static str) -> Edition {
        Edition {
            ident: Ident::ChdSha1(digest),
            label,
        }
    }
}

/// A game of the launcher's table (`static GAMES: &[Game]`).
pub trait Title: Debug + Sync + 'static {
    /// The game's id: its order is the order games are listed in.
    type Id: Copy + Eq + Ord + Hash + Debug + Send + Sync;

    fn id(&self) -> Self::Id;

    /// The file that identifies the edition, relative to the game folder,
    /// `/`-separated (it may be in a subfolder: `Rally/Rally.exe`).
    fn main_file(&self) -> &'static str;

    fn editions(&self) -> &'static [Edition];
}

/// The game and edition `digest` belongs to (either kind of identity).
pub fn identify<G: Title>(
    games: &'static [G],
    digest: &str,
) -> Option<(&'static G, &'static Edition)> {
    games.iter().find_map(|g| {
        g.editions()
            .iter()
            .find(|e| e.ident.matches(digest))
            .map(|e| (g, e))
    })
}

/// The game with id `id`.
pub fn by_id<G: Title>(games: &'static [G], id: G::Id) -> Option<&'static G> {
    games.iter().find(|g| g.id() == id)
}

/// Why a game table is wrong, for a launcher's own test: a duplicate id, a
/// digest listed twice, or a digest of the wrong length or alphabet.
pub fn check_table<G: Title>(games: &[G]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut digests = std::collections::HashSet::new();
    for g in games {
        if !ids.insert(g.id()) {
            return Err(format!("{:?} is listed twice", g.id()));
        }
        for e in g.editions() {
            if !e.ident.well_formed() {
                return Err(format!("{:?}: malformed digest {:?}", g.id(), e.ident));
            }
            if !digests.insert(e.ident.digest().to_ascii_lowercase()) {
                return Err(format!(
                    "{:?}: digest {} listed twice",
                    g.id(),
                    e.ident.digest()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub enum Id {
        Elf,
        Disc,
        Twin,
    }

    #[derive(Debug)]
    pub struct Game {
        pub id: Id,
        pub main_file: &'static str,
        pub editions: &'static [Edition],
    }

    impl Title for Game {
        type Id = Id;
        fn id(&self) -> Id {
            self.id
        }
        fn main_file(&self) -> &'static str {
            self.main_file
        }
        fn editions(&self) -> &'static [Edition] {
            self.editions
        }
    }

    /// sha256(b"elf"), and a CHD's recorded SHA-1.
    pub const ELF: &str = "780d84b20d7ae7e6292919399348bdbf96025270136198083fc8a4da398b5ca9";
    pub const CHD: &str = "c146e8c4d71d1a0cbe836a0b50dea87797869da8";

    pub static GAMES: &[Game] = &[
        Game {
            id: Id::Elf,
            main_file: "game.elf",
            editions: &[Edition::sha256(ELF, "Export")],
        },
        Game {
            id: Id::Disc,
            main_file: "set/disc.chd",
            editions: &[Edition::chd_sha1(CHD, "GDS-0027")],
        },
        Game {
            id: Id::Twin,
            main_file: "game.elf",
            editions: &[Edition::sha256(
                "1111111111111111111111111111111111111111111111111111111111111111",
                "Twin",
            )],
        },
    ];

    #[test]
    fn identify_finds_a_game_by_either_identity_in_any_case() {
        let (g, e) = identify(GAMES, &ELF.to_ascii_uppercase()).unwrap();
        assert_eq!((g.id, e.label), (Id::Elf, "Export"));
        let (g, e) = identify(GAMES, CHD).unwrap();
        assert_eq!(g.id, Id::Disc);
        assert_eq!(e.ident.kind(), IdentKind::ChdSha1);
        assert!(identify(GAMES, "00").is_none());
        assert_eq!(by_id(GAMES, Id::Twin).unwrap().main_file, "game.elf");
    }

    #[test]
    fn a_table_with_a_duplicate_or_malformed_entry_is_refused() {
        const SHORT: &[Edition] = &[Edition::chd_sha1("abc", "x")];
        const SAME: &[Edition] = &[Edition::sha256(ELF, "x"), Edition::sha256(ELF, "y")];
        assert_eq!(check_table(GAMES), Ok(()));
        let twice = [
            Game {
                id: Id::Elf,
                main_file: "a",
                editions: &[],
            },
            Game {
                id: Id::Elf,
                main_file: "b",
                editions: &[],
            },
        ];
        assert!(check_table(&twice).unwrap_err().contains("listed twice"));
        let short = [Game {
            id: Id::Elf,
            main_file: "a",
            editions: SHORT,
        }];
        assert!(check_table(&short).unwrap_err().contains("malformed"));
        let same = [Game {
            id: Id::Elf,
            main_file: "a",
            editions: SAME,
        }];
        assert!(check_table(&same).unwrap_err().contains("listed twice"));
    }

    #[test]
    fn the_host_is_the_build_target() {
        assert_eq!(Host::current() == Host::Windows, cfg!(windows));
    }
}
