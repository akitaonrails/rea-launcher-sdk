//! Finding the player's games in their library folders.
//!
//! From the TeknoParrot hub launchers' `library.rs`, the best of the four:
//! a library is a folder of game folders (any names); each folder whose main
//! file ([`Title::main_file`], possibly in a subfolder: Sega) matches a
//! known edition becomes an [`Entry`], and one folder may hold several games
//! (Initial D's NAOMI 2 folder: one CHD per game). A known file name with an
//! unknown digest is reported, never launched. Fixes folded in:
//!
//! - [`HashCache`] clones share one cache, so a rescan started while a scan
//!   runs reuses its digests instead of starting empty (Initial D, Nintendo
//!   and Sega re-hashed every file);
//! - extra images found by extension (Nintendo's renamed `.iso`/`.gcm`) are
//!   hashed only within a size range ([`Extra`]): any stray disc image was
//!   read whole on every start and `--list`;
//! - files and folders that cannot be read are returned as [`Problem`]s
//!   instead of skipped silently;
//! - hashing goes through [`crate::identify::sha256_file`] (retries an
//!   interrupted read).

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::catalog::{self, Edition, IdentKind, Title};
use crate::identify;

/// Digests by (path, kind), kept with the file's size and modification
/// time, so a rescan doesn't read large files again. Clones share the same
/// cache (it is safe to use from several scan threads).
#[derive(Debug, Default, Clone)]
pub struct HashCache {
    map: Arc<Mutex<Digests>>,
}

/// (path, kind) → (size and modification time, digest).
type Digests = HashMap<(PathBuf, IdentKind), (Stamp, String)>;
type Stamp = (u64, Option<SystemTime>);

impl HashCache {
    /// The digest of `path` as `kind` identifies it, from the cache while
    /// the file's size and modification time are unchanged.
    pub fn digest(&self, path: &Path, kind: IdentKind) -> io::Result<String> {
        let meta = fs::metadata(path)?;
        let stamp = (meta.len(), meta.modified().ok());
        let key = (path.to_path_buf(), kind);
        if let Some((s, d)) = self.lock().get(&key)
            && *s == stamp
        {
            return Ok(d.clone());
        }
        // hashed without the lock: another scan keeps using the cache
        let digest = match kind {
            IdentKind::Sha256 => identify::hex(&identify::sha256_file(path)?),
            IdentKind::ChdSha1 => chd_sha1(path)?,
        };
        self.lock().insert(key, (stamp, digest.clone()));
        Ok(digest)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Digests> {
        // a panic in another scan leaves only finished entries behind
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The SHA-1 a CHD v5 header records for the disc (raw data plus metadata,
/// bytes 84..104): the identity MAME and Redump use, read without hashing
/// the whole file.
pub fn chd_sha1(path: &Path) -> io::Result<String> {
    let mut head = [0u8; 124];
    fs::File::open(path)?.read_exact(&mut head)?;
    if &head[..8] != b"MComprHD" {
        return Err(io::Error::other("not a CHD file"));
    }
    let version = u32::from_be_bytes([head[12], head[13], head[14], head[15]]);
    if version != 5 {
        return Err(io::Error::other(format!("CHD version {version}, need 5")));
    }
    Ok(identify::hex(&head[84..104]))
}

/// Images a game folder may hold under any name (a Triforce dump renamed
/// by the player): files directly in the folder with one of `extensions`
/// (lower case, with the dot) and a size in `sizes`. They are hashed
/// as SHA-256; one that is no known edition is not reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    pub extensions: &'static [&'static str],
    pub sizes: RangeInclusive<u64>,
}

impl Extra {
    fn takes(&self, name: &str, size: u64) -> bool {
        let lower = name.to_ascii_lowercase();
        self.extensions.iter().any(|x| lower.ends_with(x)) && self.sizes.contains(&size)
    }
}

/// What a file in a game folder is.
#[derive(Debug)]
pub enum Found<G: 'static> {
    Known {
        game: &'static G,
        edition: &'static Edition,
    },
    /// A file named like a game's main file, with a digest we don't know.
    Unknown { file: String, digest: String },
}

impl<G> Clone for Found<G> {
    fn clone(&self) -> Self {
        match self {
            Found::Known { game, edition } => Found::Known { game, edition },
            Found::Unknown { file, digest } => Found::Unknown {
                file: file.clone(),
                digest: digest.clone(),
            },
        }
    }
}

impl<G> PartialEq for Found<G> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Found::Known {
                    game: a,
                    edition: e,
                },
                Found::Known {
                    game: b,
                    edition: f,
                },
            ) => std::ptr::eq(*a, *b) && e == f,
            (Found::Unknown { file: a, digest: d }, Found::Unknown { file: b, digest: e }) => {
                a == b && d == e
            }
            _ => false,
        }
    }
}

/// A game (or an unknown build of one) in a folder.
#[derive(Debug)]
pub struct Entry<G: 'static> {
    /// The game folder.
    pub dir: PathBuf,
    /// The file that identified it (a renamed image keeps its name).
    pub file: PathBuf,
    pub found: Found<G>,
}

impl<G> Clone for Entry<G> {
    fn clone(&self) -> Self {
        Entry {
            dir: self.dir.clone(),
            file: self.file.clone(),
            found: self.found.clone(),
        }
    }
}

impl<G> PartialEq for Entry<G> {
    fn eq(&self, other: &Self) -> bool {
        (&self.dir, &self.file, &self.found) == (&other.dir, &other.file, &other.found)
    }
}

impl<G: Title> Entry<G> {
    pub fn game_id(&self) -> Option<G::Id> {
        match self.found {
            Found::Known { game, .. } => Some(game.id()),
            Found::Unknown { .. } => None,
        }
    }
}

/// A file or folder of the library that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub path: PathBuf,
    pub error: String,
}

impl Problem {
    fn new(path: &Path, error: &io::Error) -> Problem {
        Problem {
            path: path.to_path_buf(),
            error: error.to_string(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.error)
    }
}

/// What a scan found: the games in catalog order (unknown builds last),
/// and what could not be read.
#[derive(Debug)]
pub struct Scan<G: 'static> {
    pub entries: Vec<Entry<G>>,
    pub problems: Vec<Problem>,
}

impl<G> Clone for Scan<G> {
    fn clone(&self) -> Self {
        Scan {
            entries: self.entries.clone(),
            problems: self.problems.clone(),
        }
    }
}

impl<G> PartialEq for Scan<G> {
    fn eq(&self, other: &Self) -> bool {
        (&self.entries, &self.problems) == (&other.entries, &other.problems)
    }
}

impl<G> Scan<G> {
    fn empty() -> Scan<G> {
        Scan {
            entries: Vec::new(),
            problems: Vec::new(),
        }
    }
}

impl<G: Title> Scan<G> {
    /// The first folder found for each game (a library may hold duplicates).
    pub fn by_game(&self) -> HashMap<G::Id, &Entry<G>> {
        let mut m = HashMap::new();
        for e in &self.entries {
            if let Some(id) = e.game_id() {
                m.entry(id).or_insert(e);
            }
        }
        m
    }
}

/// A file of a game folder to hash: a main file by its name, or an
/// [`Extra`] image.
struct Candidate {
    name: String,
    file: PathBuf,
    kind: IdentKind,
    extra: bool,
}

/// Scans library folders for the games of one table.
#[derive(Debug, Clone)]
pub struct Library<G: 'static> {
    games: &'static [G],
    extra: Option<Extra>,
    cache: HashCache,
}

impl<G: Title> Library<G> {
    pub fn new(games: &'static [G]) -> Library<G> {
        Library {
            games,
            extra: None,
            cache: HashCache::default(),
        }
    }

    /// Also identifies images found by extension ([`Extra`]).
    pub fn with_extra(mut self, extra: Extra) -> Library<G> {
        self.extra = Some(extra);
        self
    }

    /// Uses (and shares) `cache`: keep one per launcher and hand it to each
    /// scan.
    pub fn with_cache(mut self, cache: HashCache) -> Library<G> {
        self.cache = cache;
        self
    }

    /// Every game in `dir` itself (one folder may hold several).
    pub fn probe_dir(&self, dir: &Path) -> Scan<G> {
        let mut scan = Scan::empty();
        for Candidate {
            name,
            file,
            kind,
            extra,
        } in self.candidates(dir, &mut scan.problems)
        {
            let digest = match self.cache.digest(&file, kind) {
                Ok(d) => d,
                Err(e) => {
                    scan.problems.push(Problem::new(&file, &e));
                    continue;
                }
            };
            let found = match catalog::identify(self.games, &digest) {
                Some((game, edition)) if extra || game.main_file() == name => {
                    Found::Known { game, edition }
                }
                // an image of something else is not ours to report
                _ if extra => continue,
                _ => Found::Unknown { file: name, digest },
            };
            scan.entries.push(Entry {
                dir: dir.to_path_buf(),
                file,
                found,
            });
        }
        scan
    }

    /// Every game in each root and the folders directly inside it, in
    /// catalog order (by id), unknown builds last.
    pub fn scan(&self, roots: &[PathBuf]) -> Scan<G> {
        let mut scan = Scan::empty();
        for root in roots {
            let mut add = |s: Scan<G>| {
                scan.entries.extend(s.entries);
                scan.problems.extend(s.problems);
            };
            add(self.probe_dir(root));
            let rd = match fs::read_dir(root) {
                Ok(rd) => rd,
                Err(e) => {
                    scan.problems.push(Problem::new(root, &e));
                    continue;
                }
            };
            let mut dirs: Vec<PathBuf> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for d in dirs {
                add(self.probe_dir(&d));
            }
        }
        scan.entries
            .sort_by_key(|e| (e.game_id().is_none(), e.game_id()));
        scan
    }

    /// The files of `dir` worth hashing: every main file present (by name,
    /// each name and kind once), then the extra images.
    fn candidates(&self, dir: &Path, problems: &mut Vec<Problem>) -> Vec<Candidate> {
        let mut probes: Vec<(&str, IdentKind)> = self
            .games
            .iter()
            .flat_map(|g| {
                let kinds: Vec<IdentKind> = g.editions().iter().map(|e| e.ident.kind()).collect();
                let kinds = if kinds.is_empty() {
                    vec![IdentKind::Sha256]
                } else {
                    kinds
                };
                kinds.into_iter().map(move |k| (g.main_file(), k))
            })
            .collect();
        probes.sort_unstable();
        probes.dedup();
        let mut out: Vec<_> = probes
            .into_iter()
            .map(|(name, kind)| Candidate {
                name: name.to_string(),
                file: dir.join(name),
                kind,
                extra: false,
            })
            .filter(|c| c.file.is_file())
            .collect();
        let Some(extra) = &self.extra else {
            return out;
        };
        let rd = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                problems.push(Problem::new(dir, &e));
                return out;
            }
        };
        let mut images: Vec<Candidate> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let meta = fs::metadata(e.path()).ok()?;
                (meta.is_file() && extra.takes(&name, meta.len())).then(|| Candidate {
                    name,
                    file: e.path(),
                    kind: IdentKind::Sha256,
                    extra: true,
                })
            })
            .filter(|c| !out.iter().any(|m| m.name == c.name))
            .collect();
        images.sort_by(|a, b| a.name.cmp(&b.name));
        out.extend(images);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tests::{CHD, GAMES, Game, Id};
    use crate::test_dir;

    fn fake_chd(sha1_hex: &str) -> Vec<u8> {
        let mut h = vec![0u8; 124];
        h[..8].copy_from_slice(b"MComprHD");
        h[12..16].copy_from_slice(&5u32.to_be_bytes());
        for i in 0..20 {
            h[84 + i] = u8::from_str_radix(&sha1_hex[i * 2..i * 2 + 2], 16).unwrap();
        }
        h
    }

    fn lib() -> Library<Game> {
        Library::new(GAMES)
    }

    #[test]
    fn unknown_digest_is_reported_with_its_hash() {
        let t = test_dir("library-unknown");
        let g = t.join("Some Game [TP]");
        fs::create_dir(&g).unwrap();
        fs::write(g.join("game.elf"), b"not the game").unwrap();
        fs::create_dir(t.join("empty")).unwrap();
        let found = lib().scan(std::slice::from_ref(&t));
        assert_eq!(found.problems, []);
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].file, g.join("game.elf"));
        match &found.entries[0].found {
            Found::Unknown { file, digest } => {
                assert_eq!(file, "game.elf");
                let want = identify::hex(&identify::sha256_file(&g.join("game.elf")).unwrap());
                assert_eq!(digest, &want);
            }
            other => panic!("{other:?}"),
        }
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn known_games_come_in_catalog_order_and_by_game_takes_the_first() {
        let t = test_dir("library-known");
        for d in ["b", "a"] {
            fs::create_dir_all(t.join(d).join("set")).unwrap();
            fs::write(t.join(d).join("game.elf"), b"elf").unwrap();
        }
        fs::write(t.join("b/set/disc.chd"), fake_chd(CHD)).unwrap();
        let found = lib().scan(std::slice::from_ref(&t));
        let ids: Vec<_> = found.entries.iter().map(Entry::game_id).collect();
        assert_eq!(ids, [Some(Id::Elf), Some(Id::Elf), Some(Id::Disc)]);
        let by = found.by_game();
        assert_eq!(by[&Id::Elf].dir, t.join("a"));
        assert_eq!(by[&Id::Disc].file, t.join("b/set/disc.chd"));
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_main_file_in_a_subfolder_is_found_by_its_path() {
        let t = test_dir("library-subfolder");
        let g = t.join("Disc Game");
        fs::create_dir_all(g.join("set")).unwrap();
        fs::write(g.join("set/disc.chd"), fake_chd(CHD)).unwrap();
        let found = lib().scan(std::slice::from_ref(&t));
        assert_eq!(found.entries.len(), 1);
        assert_eq!(found.entries[0].dir, g);
        assert_eq!(found.entries[0].game_id(), Some(Id::Disc));
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn one_folder_may_hold_several_games() {
        let t = test_dir("library-several");
        fs::create_dir_all(t.join("set")).unwrap();
        fs::write(t.join("set/disc.chd"), fake_chd(CHD)).unwrap();
        fs::write(t.join("game.elf"), b"elf").unwrap();
        let found = lib().probe_dir(&t);
        let ids: Vec<_> = found.entries.iter().map(Entry::game_id).collect();
        assert_eq!(ids, [Some(Id::Elf), Some(Id::Disc)]);
        assert!(found.entries.iter().all(|e| e.dir == t));
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_file_that_is_not_a_chd_is_a_problem_not_a_game() {
        let t = test_dir("library-not-chd");
        fs::create_dir_all(t.join("set")).unwrap();
        fs::write(t.join("set/disc.chd"), b"junk").unwrap();
        let found = lib().probe_dir(&t);
        assert_eq!(found.entries, []);
        assert_eq!(found.problems.len(), 1);
        assert_eq!(found.problems[0].path, t.join("set/disc.chd"));
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_missing_library_folder_is_reported() {
        let t = test_dir("library-missing");
        let gone = t.join("gone");
        let found = lib().scan(std::slice::from_ref(&gone));
        assert_eq!(found.entries, []);
        assert_eq!(found.problems.len(), 1);
        assert!(
            found.problems[0]
                .to_string()
                .starts_with(&gone.display().to_string())
        );
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn extra_images_are_found_by_extension_within_their_size_range() {
        let t = test_dir("library-extra");
        fs::write(t.join("Renamed.ISO"), b"elf").unwrap();
        fs::write(t.join("holiday.iso"), b"junk").unwrap();
        fs::write(t.join("huge.iso"), vec![0u8; 64]).unwrap();
        fs::write(t.join("notes.txt"), b"elf").unwrap();
        let cache = HashCache::default();
        let library = lib()
            .with_extra(Extra {
                extensions: &[".iso", ".gcm"],
                sizes: 1..=16,
            })
            .with_cache(cache.clone());
        let found = library.probe_dir(&t);
        assert_eq!(found.problems, []);
        assert_eq!(found.entries.len(), 1, "a stray image is not reported");
        assert_eq!(found.entries[0].file, t.join("Renamed.ISO"));
        assert_eq!(found.entries[0].game_id(), Some(Id::Elf));
        // the image past the size bound was never read
        let hashed = cache.lock();
        assert!(hashed.keys().any(|(p, _)| p.ends_with("holiday.iso")));
        assert!(!hashed.keys().any(|(p, _)| p.ends_with("huge.iso")));
        drop(hashed);
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn cache_rehashes_only_when_the_file_changes_and_clones_share_it() {
        let t = test_dir("library-cache");
        let f = t.join("main");
        fs::write(&f, b"one").unwrap();
        let c = HashCache::default();
        let a = c.digest(&f, IdentKind::Sha256).unwrap();
        // a clone (another scan thread) finds it without reading the file
        assert_eq!(c.clone().lock().len(), 1);
        assert_eq!(c.clone().digest(&f, IdentKind::Sha256).unwrap(), a);
        fs::write(&f, b"three").unwrap();
        assert_ne!(c.digest(&f, IdentKind::Sha256).unwrap(), a);
        assert!(c.digest(&t.join("gone"), IdentKind::Sha256).is_err());
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn chd_headers_give_their_disc_sha1_and_other_files_an_error() {
        let t = test_dir("library-chd");
        let f = t.join("d.chd");
        fs::write(&f, fake_chd(CHD)).unwrap();
        assert_eq!(chd_sha1(&f).unwrap(), CHD);
        let mut v4 = fake_chd(CHD);
        v4[12..16].copy_from_slice(&4u32.to_be_bytes());
        fs::write(&f, v4).unwrap();
        assert_eq!(
            chd_sha1(&f).unwrap_err().to_string(),
            "CHD version 4, need 5"
        );
        fs::write(&f, b"MComprHD").unwrap();
        assert!(chd_sha1(&f).is_err(), "too short");
        fs::remove_dir_all(t).unwrap();
    }
}
