//! The component ownership ledger: what the launcher installed into the
//! runtime, by SHA-256.
//!
//! From CMR4 `components.rs` (ported to CMR3 in 53c91f0): the runtime
//! starts as a copy of the player's folder, which may already hold a
//! `dinput8.dll` or `d3d9.dll` — the game's, or a mod of the player's
//! (DSOAL, SpecialK). CMR3's Xidi "Off" deleted any `dinput8.dll`. Now a
//! file is installed only over our own or an untouched copy of the game's,
//! taken out only while its bytes are still what we wrote (or a known
//! bundle file, for runtimes from before the ledger), and taking it out
//! brings the game's own file back. Names are plain relative paths
//! (CMR3 c59356a: a record never reaches outside the runtime).
//!
//! The ledger file holds `hex-sha256\trel` lines.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsx;
use crate::identify::{hex, sha256};
use crate::paths::{find_ignoring_case, native};
use crate::runtime;

/// What [`Ledger::install`] and [`Ledger::take_out`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Installed (or already ours with these bytes, rewritten anyway).
    Installed,
    /// Ours, taken out: the game's own file is back, or none.
    Restored,
    /// Nothing there, or nothing of ours.
    Absent,
    /// Someone else's file (a player's mod, a changed copy): left alone.
    NotOurs,
}

/// The ledger of one runtime folder.
#[derive(Debug, Clone)]
pub struct Ledger {
    game: PathBuf,
    runtime: PathBuf,
    file: PathBuf,
    owned: BTreeMap<String, String>,
}

/// True for a plain relative `/`-separated path that stays below the
/// folder it is joined to: no root, drive, `.` or `..`, on any host.
pub fn is_plain_relative(rel: &str) -> bool {
    !rel.is_empty()
        && rel
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.contains(':'))
}

fn plain(rel: &str) -> io::Result<()> {
    if is_plain_relative(rel) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a plain relative path: {rel:?}"),
        ))
    }
}

impl Ledger {
    /// Reads `runtime/name` (a missing or unreadable ledger is empty;
    /// lines with other than plain relative paths are dropped).
    pub fn load(game: &Path, runtime: &Path, name: &str) -> Ledger {
        let file = runtime.join(name);
        let owned = fs::read_to_string(&file)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let (digest, rel) = line.split_once('\t')?;
                is_plain_relative(rel).then(|| (rel.to_owned(), digest.to_owned()))
            })
            .collect();
        Ledger {
            game: game.to_path_buf(),
            runtime: runtime.to_path_buf(),
            file,
            owned,
        }
    }

    fn save(&self) -> io::Result<()> {
        let text: String = self
            .owned
            .iter()
            .map(|(rel, digest)| format!("{digest}\t{rel}\n"))
            .collect();
        fsx::write_atomic(&self.file, text)
    }

    /// The names the ledger records.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.owned.keys().map(String::as_str)
    }

    /// True when the runtime's `rel` holds what the ledger recorded, or one
    /// of `known` (bundle files we may have written before the ledger).
    pub fn owns(&self, rel: &str, known: &[&[u8]]) -> io::Result<bool> {
        plain(rel)?;
        match fs::read(self.runtime_path(rel)) {
            Ok(bytes) => Ok(self.owned.get(rel) == Some(&hex(&sha256(&bytes)))
                || known.contains(&bytes.as_slice())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The runtime's `rel`, in whatever case it exists (Wine would load a
    /// `D3D9.DLL` for `d3d9.dll`, so a second spelling is never added).
    fn runtime_path(&self, rel: &str) -> PathBuf {
        find_ignoring_case(&self.runtime, rel).unwrap_or_else(|| native(&self.runtime, rel))
    }

    /// The game's own `rel`, in whatever case it exists.
    fn game_file(&self, rel: &str) -> Option<PathBuf> {
        find_ignoring_case(&self.game, rel).filter(|p| p.is_file())
    }

    /// True when the runtime's `rel` is byte-equal to the game's own.
    fn untouched_game_copy(&self, rel: &str) -> io::Result<bool> {
        let Some(game) = self.game_file(rel) else {
            return Ok(false);
        };
        let runtime = self.runtime_path(rel);
        Ok(fs::read(&game).map_err(fsx::at(&game))?
            == fs::read(&runtime).map_err(fsx::at(&runtime))?)
    }

    /// Installs `bytes` as the runtime's `rel` and records it — only when
    /// nothing is there, it is ours (see [`Ledger::owns`]), or it is an
    /// untouched copy of the game's own file. Returns
    /// [`Outcome::NotOurs`] (writing nothing) otherwise.
    pub fn install(&mut self, rel: &str, bytes: &[u8], known: &[&[u8]]) -> io::Result<Outcome> {
        plain(rel)?;
        let path = self.runtime_path(rel);
        if fs::symlink_metadata(&path).is_ok()
            && !self.owns(rel, known)?
            && !self.untouched_game_copy(rel)?
        {
            return Ok(Outcome::NotOurs);
        }
        fsx::install(&path, bytes)?;
        self.owned.insert(rel.to_owned(), hex(&sha256(bytes)));
        self.save()?;
        Ok(Outcome::Installed)
    }

    /// Takes the runtime's `rel` out when it is ours: the game's own file
    /// (found whatever its case) comes back as a copy, or the file goes.
    /// Someone else's file stays. The record goes either way.
    pub fn take_out(&mut self, rel: &str, known: &[&[u8]]) -> io::Result<Outcome> {
        self.take_out_with(rel, known, &runtime::CopyAll)
    }

    /// Takes the runtime's `rel` out when it is ours, WITHOUT bringing the
    /// game folder's own copy back (OR: repacks ship SpecialK's `d3d9.dll`,
    /// which must not load before the real Direct3D). `Restored` here means
    /// ours was removed. Someone else's file stays; the record goes.
    pub fn take_out_keep_game(&mut self, rel: &str, known: &[&[u8]]) -> io::Result<Outcome> {
        plain(rel)?;
        let path = self.runtime_path(rel);
        let outcome = if self.owns(rel, known)? {
            fsx::remove(&path)?;
            Outcome::Restored
        } else if fs::symlink_metadata(&path).is_ok() {
            Outcome::NotOurs
        } else {
            Outcome::Absent
        };
        if self.owned.remove(rel).is_some() {
            self.save()?;
        }
        Ok(outcome)
    }

    /// [`Ledger::take_out`], bringing the game's file back the way a sync
    /// would: hard-linked where `policy` links it (and the original is
    /// writable), else copied with its time.
    pub fn take_out_with(
        &mut self,
        rel: &str,
        known: &[&[u8]],
        policy: &dyn runtime::Policy,
    ) -> io::Result<Outcome> {
        plain(rel)?;
        let path = self.runtime_path(rel);
        let outcome = if self.owns(rel, known)? {
            match self.game_file(rel) {
                Some(game) => {
                    let game_rel = game
                        .strip_prefix(&self.game)
                        .map(|p| {
                            p.components()
                                .map(|c| c.as_os_str().to_string_lossy())
                                .collect::<Vec<_>>()
                                .join("/")
                        })
                        .unwrap_or_else(|_| rel.to_owned());
                    runtime::bring_over(&game, &path, policy.link(&game_rel))?;
                }
                None => fsx::remove(&path)?,
            }
            Outcome::Restored
        } else if fs::symlink_metadata(&path).is_ok() && !self.untouched_game_copy(rel)? {
            Outcome::NotOurs
        } else {
            Outcome::Absent
        };
        if self.owned.remove(rel).is_some() {
            self.save()?;
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    const NAME: &str = ".test-components";

    fn setup(name: &str) -> (PathBuf, PathBuf) {
        (
            test_dir(&format!("{name}-game")),
            test_dir(&format!("{name}-rt")),
        )
    }

    #[test]
    fn installs_and_takes_out_only_what_it_put_in() {
        let (game, rt) = setup("ledger-own");
        // The game ships its own dinput8.dll; the runtime has its copy.
        fs::write(game.join("dinput8.dll"), b"game's own").unwrap();
        fs::write(rt.join("dinput8.dll"), b"game's own").unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.install("dinput8.dll", b"xidi", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(
            ledger.install("d3d9.dll", b"dxvk", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(fs::read(rt.join("dinput8.dll")).unwrap(), b"xidi");

        // A fresh load sees the record.
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.names().collect::<Vec<_>>(),
            ["d3d9.dll", "dinput8.dll"]
        );
        assert_eq!(
            ledger.take_out("dinput8.dll", &[]).unwrap(),
            Outcome::Restored
        );
        assert_eq!(fs::read(rt.join("dinput8.dll")).unwrap(), b"game's own");

        // A d3d9.dll the player dropped in after ours is not ours.
        fs::write(rt.join("d3d9.dll"), b"player's d3d9").unwrap();
        assert_eq!(ledger.take_out("d3d9.dll", &[]).unwrap(), Outcome::NotOurs);
        assert_eq!(fs::read(rt.join("d3d9.dll")).unwrap(), b"player's d3d9");
        assert_eq!(
            ledger.install("d3d9.dll", b"dxvk", &[]).unwrap(),
            Outcome::NotOurs
        );
        assert_eq!(fs::read(rt.join("d3d9.dll")).unwrap(), b"player's d3d9");
        assert!(Ledger::load(&game, &rt, NAME).names().next().is_none());

        // The game's untouched copy is already the "off" state.
        assert_eq!(
            ledger.take_out("dinput8.dll", &[]).unwrap(),
            Outcome::Absent
        );
        assert_eq!(ledger.take_out("never.dll", &[]).unwrap(), Outcome::Absent);
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn known_bundle_bytes_count_as_ours_before_the_ledger() {
        let (game, rt) = setup("ledger-known");
        fs::write(rt.join("dinput8.dll"), b"xidi 1.0").unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.take_out("dinput8.dll", &[b"xidi 2.0"]).unwrap(),
            Outcome::NotOurs
        );
        assert!(rt.join("dinput8.dll").exists());
        assert_eq!(
            ledger
                .install("dinput8.dll", b"xidi 2.0", &[b"xidi 1.0"])
                .unwrap(),
            Outcome::Installed
        );
        assert_eq!(
            ledger.take_out("dinput8.dll", &[]).unwrap(),
            Outcome::Restored
        );
        assert!(!rt.join("dinput8.dll").exists(), "the game has none");
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn only_plain_relative_names_are_accepted() {
        let (game, rt) = setup("ledger-plain");
        fs::write(
            rt.join(NAME),
            "00\t../outside.dll\n00\tC:\\x.dll\n00\t/abs.dll\n00\tData/ok.dds\n",
        )
        .unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(ledger.names().collect::<Vec<_>>(), ["Data/ok.dds"]);
        assert!(ledger.install("../evil.dll", b"x", &[]).is_err());
        assert!(ledger.take_out("a/./b", &[]).is_err());
        assert!(is_plain_relative("fonts/hel_18pt.dds"));
        assert!(!is_plain_relative(""));
        assert!(!is_plain_relative("a//b"));
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn restoring_never_writes_through_a_hard_link() {
        let (game, rt) = setup("ledger-link");
        fs::write(game.join("dsound.dll"), b"game's").unwrap();
        fs::hard_link(game.join("dsound.dll"), rt.join("dsound.dll")).unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        // A linked game copy counts as untouched.
        assert_eq!(
            ledger.install("dsound.dll", b"loader", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(fs::read(game.join("dsound.dll")).unwrap(), b"game's");
        assert_eq!(
            ledger.take_out("dsound.dll", &[]).unwrap(),
            Outcome::Restored
        );
        assert_eq!(fs::read(rt.join("dsound.dll")).unwrap(), b"game's");
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn names_match_the_game_and_runtime_whatever_their_case() {
        let (game, rt) = setup("ledger-case");
        fs::write(game.join("DINPUT8.DLL"), b"game's own").unwrap();
        fs::write(rt.join("DInput8.dll"), b"game's own").unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.install("dinput8.dll", b"xidi", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(fs::read(rt.join("DInput8.dll")).unwrap(), b"xidi");
        // One file, in the runtime's own spelling (`exists` would ignore
        // case on Windows, so the listing is what counts).
        let dlls = |dir: &Path| -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|n| n.to_ascii_lowercase().ends_with(".dll"))
                .collect();
            names.sort();
            names
        };
        assert_eq!(dlls(&rt), ["DInput8.dll"], "no second spelling");
        assert_eq!(
            ledger.take_out("dinput8.dll", &[]).unwrap(),
            Outcome::Restored
        );
        assert_eq!(fs::read(rt.join("DInput8.dll")).unwrap(), b"game's own");
        assert_eq!(dlls(&rt), ["DInput8.dll"]);
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn taking_out_can_link_the_games_file_back() {
        use std::os::unix::fs::MetadataExt;
        struct LinkDlls;
        impl runtime::Policy for LinkDlls {
            fn link(&self, rel: &str) -> bool {
                rel.to_ascii_lowercase().ends_with(".dll")
            }
        }
        let (game, rt) = setup("ledger-link-back");
        fs::write(game.join("D3D9.DLL"), b"game's own").unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.install("d3d9.dll", b"dxvk", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(
            ledger.take_out_with("d3d9.dll", &[], &LinkDlls).unwrap(),
            Outcome::Restored
        );
        let ino = |p: &Path| fs::metadata(p).unwrap().ino();
        assert_eq!(
            ino(&rt.join("d3d9.dll")),
            ino(&game.join("D3D9.DLL")),
            "linked"
        );
        // The default still copies.
        ledger.install("d3d9.dll", b"dxvk", &[]).unwrap();
        ledger.take_out("d3d9.dll", &[]).unwrap();
        assert_ne!(ino(&rt.join("d3d9.dll")), ino(&game.join("D3D9.DLL")));
        assert_eq!(fs::read(rt.join("d3d9.dll")).unwrap(), b"game's own");
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn taking_out_can_leave_the_games_copy_out() {
        let (game, rt) = setup("ledger-keep-game");
        // A repack's SpecialK d3d9.dll in the game folder.
        fs::write(game.join("d3d9.dll"), b"specialk").unwrap();
        let mut ledger = Ledger::load(&game, &rt, NAME);
        assert_eq!(
            ledger.install("d3d9.dll", b"dxvk", &[]).unwrap(),
            Outcome::Installed
        );
        assert_eq!(
            ledger.take_out_keep_game("d3d9.dll", &[]).unwrap(),
            Outcome::Restored
        );
        assert!(!rt.join("d3d9.dll").exists(), "SpecialK does not come back");
        assert!(ledger.names().next().is_none());
        // A player's own file stays.
        fs::write(rt.join("d3d9.dll"), b"player's").unwrap();
        assert_eq!(
            ledger.take_out_keep_game("d3d9.dll", &[]).unwrap(),
            Outcome::NotOurs
        );
        assert_eq!(fs::read(rt.join("d3d9.dll")).unwrap(), b"player's");
        // Pre-ledger runtimes: known bundle bytes count as ours.
        fs::write(rt.join("d3d9.dll"), b"dxvk 1.0").unwrap();
        assert_eq!(
            ledger
                .take_out_keep_game("d3d9.dll", &[b"dxvk 1.0"])
                .unwrap(),
            Outcome::Restored
        );
        assert_eq!(
            ledger.take_out_keep_game("d3d9.dll", &[]).unwrap(),
            Outcome::Absent
        );
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }
}
