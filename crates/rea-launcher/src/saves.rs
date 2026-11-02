//! Keeps the player's saves (outside the runtime) and the runtime's copy in
//! step, by the state recorded at the last sync.
//!
//! From CMR3 7d607b4 (one profile file) and OR 7e00c0a (a save folder),
//! with SCR 509d895's state format (both sides' stamps, nanoseconds).
//! Newer-wins, which all five shipped first, threw away a backup the player
//! restored with an older timestamp (`cp -p`, rsync, Syncthing, clock skew)
//! and then overwrote it. Now:
//!
//! - at launch ([`Mirror::before`]) a file that changed in the saves folder
//!   since the last sync wins, even if older; otherwise one that changed in
//!   the runtime wins (the last session ended before the write-back);
//! - at exit ([`Mirror::after`]) every runtime file whose bytes differ goes
//!   to the saves folder;
//! - a file with no recorded state follows [`FirstRun`], which every
//!   launcher states (OR: the saves folder wins; CMR3/SCR: the newer copy);
//! - a file missing on one side while the other is unchanged follows
//!   [`Deleted`]: copied back (CMR3, SCR) or left gone.
//!
//! Names match without regard to case between the two sides (SCR: the
//! game writes `CHOICE.DAT`, a backup may hold `choice.dat`). Nothing is
//! ever deleted. Copies are whole-file swaps keeping the source's exact
//! time. State files written by CMR3 (`size\tmtime`), OR
//! (`size\tmtime\trel`) and SCR (`size\tns\tsize\tns\trel`) are read;
//! whole-second records compare at whole seconds.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::fsx::{self, at};
use crate::paths::{find_ignoring_case, native};
use crate::runtime;

/// What wins for a file both sides have but no state records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRun {
    /// The saves folder's copy (OR).
    SavesWin,
    /// The copy with the later modification time; the runtime's on a tie
    /// (CMR3, SCR).
    NewerWins,
}

/// What happens to a save missing on one side while the other side is
/// unchanged since the last sync (the game, or the player, deleted it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deleted {
    /// Copied back from the other side (CMR3, SCR).
    #[default]
    Restore,
    /// Left gone, and remembered as gone, so a save slot the game deleted
    /// stays deleted (OR's launch rule; the copy on the other side is
    /// kept, never deleted).
    Stays,
}

/// A file's size and modification time in nanoseconds since the epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveStamp {
    pub size: u64,
    pub mtime_ns: u128,
}

impl SaveStamp {
    fn of(path: &Path) -> Option<SaveStamp> {
        let meta = fs::metadata(path).ok().filter(|m| m.is_file())?;
        Some(SaveStamp {
            size: meta.len(),
            mtime_ns: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos()),
        })
    }
}

/// Both sides of one file as of the last sync (`None`: missing there).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    pub saves: Option<SaveStamp>,
    pub runtime: Option<SaveStamp>,
    /// Read from an older whole-second record: compared at whole seconds.
    pub coarse: bool,
}

impl Recorded {
    fn same(&self, recorded: Option<SaveStamp>, now: Option<SaveStamp>) -> bool {
        const NS: u128 = 1_000_000_000;
        match (recorded, now) {
            (Some(r), Some(n)) if self.coarse => {
                r.size == n.size && r.mtime_ns / NS == n.mtime_ns / NS
            }
            (r, n) => r == n,
        }
    }
}

/// The key two sides share for a relative path: lower case.
pub fn key(rel: &str) -> String {
    rel.to_lowercase()
}

/// Files by [`key`].
pub type Stamps = BTreeMap<String, SaveStamp>;
/// The recorded state by [`key`].
pub type State = BTreeMap<String, Recorded>;

/// Which files to copy, each way.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Moved {
    pub to_runtime: Vec<String>,
    pub to_saves: Vec<String>,
}

impl Moved {
    pub fn is_empty(&self) -> bool {
        self.to_runtime.is_empty() && self.to_saves.is_empty()
    }
}

/// Decides the copies at launch from both sides' stamps and the state
/// (`None`: never synced). See the module documentation.
pub fn plan(
    saves: &Stamps,
    runtime: &Stamps,
    state: Option<&State>,
    first: FirstRun,
    deleted: Deleted,
) -> Moved {
    let mut moved = Moved::default();
    let names: BTreeSet<&String> = saves.keys().chain(runtime.keys()).collect();
    for name in names {
        let (s, r) = (saves.get(name).copied(), runtime.get(name).copied());
        let restore = deleted == Deleted::Restore;
        let to_runtime = match state.and_then(|st| st.get(name)) {
            Some(rec) => {
                let (s_changed, r_changed) = (!rec.same(rec.saves, s), !rec.same(rec.runtime, r));
                match (s, r) {
                    (Some(_), _) if s_changed => true,
                    (_, Some(_)) if r_changed => false,
                    (Some(_), None) if restore => true,
                    (None, Some(_)) if restore => false,
                    _ => continue,
                }
            }
            None => match (s, r) {
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(s), Some(r)) if s != r => match first {
                    FirstRun::SavesWin => true,
                    FirstRun::NewerWins => s.mtime_ns > r.mtime_ns,
                },
                _ => continue,
            },
        };
        if to_runtime {
            moved.to_runtime.push(name.clone());
        } else {
            moved.to_saves.push(name.clone());
        }
    }
    moved
}

/// Reads a state file (any of the formats in the module documentation;
/// `-` marks a side that was missing). `single` names the entry of a
/// one-file mirror, for CMR3's record that has no name.
pub fn parse_state(text: &str, single: &str) -> State {
    const NS: u128 = 1_000_000_000;
    let mut state = State::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        // Some(None): a recorded absence; None: not a stamp.
        let stamp = |i: usize, scale: u128| -> Option<Option<SaveStamp>> {
            let (size, time) = (*fields.get(i)?, *fields.get(i + 1)?);
            if size == "-" && time == "-" {
                return Some(None);
            }
            Some(Some(SaveStamp {
                size: size.parse().ok()?,
                mtime_ns: time.parse::<u128>().ok()?.checked_mul(scale)?,
            }))
        };
        let entry = match fields.len() {
            2 => stamp(0, NS).map(|s| (single.to_owned(), s, s, true)),
            3 => stamp(0, NS).map(|s| (fields[2].to_owned(), s, s, true)),
            n if n >= 5 => stamp(0, 1)
                .zip(stamp(2, 1))
                .map(|(s, r)| (fields[4..].join("\t"), s, r, false)),
            _ => None,
        };
        if let Some((rel, saves, runtime, coarse)) = entry {
            state.insert(
                key(&rel),
                Recorded {
                    saves,
                    runtime,
                    coarse,
                },
            );
        }
    }
    state
}

/// Writes a state file in the precise five-field format.
pub fn format_state(state: &State) -> String {
    let side = |s: Option<SaveStamp>| {
        s.map_or_else(
            || "-\t-".to_owned(),
            |s| format!("{}\t{}", s.size, s.mtime_ns),
        )
    };
    state
        .iter()
        .map(|(name, r)| format!("{}\t{}\t{name}\n", side(r.saves), side(r.runtime)))
        .collect()
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    File,
    /// Every file below, recursively.
    Folder,
    /// Only the files directly in these subfolders (`""`: the folder
    /// itself), matched without regard to case.
    Folders(&'a [&'a str]),
}

/// One side's files: key to (relative path as spelled there, stamp).
type Side = BTreeMap<String, (String, SaveStamp)>;

/// One mirrored save: a single file or a folder, its runtime and saves
/// sides, and the state file that records the last sync (keep it in the
/// runtime folder).
pub struct Mirror<'a> {
    runtime: &'a Path,
    saves: &'a Path,
    state: &'a Path,
    kind: Kind<'a>,
    first: FirstRun,
    deleted: Deleted,
    filter: Option<&'a dyn Fn(&str) -> bool>,
}

impl<'a> Mirror<'a> {
    /// A single file (CMR3's `CMR3` profile).
    pub fn file(
        runtime: &'a Path,
        saves: &'a Path,
        state: &'a Path,
        first: FirstRun,
    ) -> Mirror<'a> {
        Mirror::new(runtime, saves, state, first, Kind::File)
    }

    /// Every file below a folder, recursively (OR's `SaveGame`).
    pub fn folder(
        runtime: &'a Path,
        saves: &'a Path,
        state: &'a Path,
        first: FirstRun,
    ) -> Mirror<'a> {
        Mirror::new(runtime, saves, state, first, Kind::Folder)
    }

    fn new(
        runtime: &'a Path,
        saves: &'a Path,
        state: &'a Path,
        first: FirstRun,
        kind: Kind<'a>,
    ) -> Mirror<'a> {
        Mirror {
            runtime,
            saves,
            state,
            kind,
            first,
            deleted: Deleted::default(),
            filter: None,
        }
    }

    /// Looks only at the files directly inside these subfolders of a
    /// folder mirror, not below them (SCR: `&["", "CONFIG", "REPLAYS"]`).
    pub fn scan(mut self, folders: &'a [&'a str]) -> Mirror<'a> {
        self.kind = Kind::Folders(folders);
        self
    }

    /// Mirrors only the relative paths `keep` accepts (SCR: `*.SAV`,
    /// `CHOICE.DAT`, `REPLAYS/*.RPL`).
    pub fn only(mut self, keep: &'a dyn Fn(&str) -> bool) -> Mirror<'a> {
        self.filter = Some(keep);
        self
    }

    /// What happens to a save deleted on one side (default
    /// [`Deleted::Restore`]).
    pub fn deleted(mut self, deleted: Deleted) -> Mirror<'a> {
        self.deleted = deleted;
        self
    }

    /// The name of the one entry of a file mirror.
    fn single(&self) -> String {
        self.runtime
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// Where `rel` is (or goes) below `root`: an existing file or folder
    /// in any case, else as spelled.
    fn path(&self, root: &Path, rel: &str) -> PathBuf {
        if let Kind::File = self.kind {
            return root.to_path_buf();
        }
        if let Some(found) = find_ignoring_case(root, rel) {
            return found;
        }
        match rel.rsplit_once('/') {
            Some((dir, name)) => find_ignoring_case(root, dir)
                .unwrap_or_else(|| native(root, dir))
                .join(name),
            None => native(root, rel),
        }
    }

    fn rels(&self, root: &Path) -> io::Result<Vec<String>> {
        Ok(match self.kind {
            Kind::File => vec![self.single()],
            Kind::Folder if root.is_dir() => {
                runtime::list(root).map_err(at(root))?.into_keys().collect()
            }
            Kind::Folder => Vec::new(),
            Kind::Folders(folders) => {
                let mut rels = Vec::new();
                for folder in folders {
                    let Some(dir) = find_ignoring_case(root, folder).filter(|d| d.is_dir()) else {
                        continue;
                    };
                    for entry in fs::read_dir(&dir).map_err(at(&dir))? {
                        let entry = entry.map_err(at(&dir))?;
                        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                            continue;
                        };
                        if entry.path().is_file() {
                            let parent = dir.strip_prefix(root).unwrap_or(Path::new(""));
                            let parent: Vec<_> = parent
                                .components()
                                .map(|c| c.as_os_str().to_string_lossy())
                                .collect();
                            rels.push(if parent.is_empty() {
                                name
                            } else {
                                format!("{}/{name}", parent.join("/"))
                            });
                        }
                    }
                }
                rels
            }
        })
    }

    fn side(&self, root: &Path) -> io::Result<Side> {
        let mut side = Side::new();
        for rel in self.rels(root)? {
            if self.filter.is_some_and(|keep| !keep(&rel)) {
                continue;
            }
            let path = self.path(root, &rel);
            if path == self.state {
                // The state file may live in the mirrored folder (SCR).
                continue;
            }
            if let Some(stamp) = SaveStamp::of(&path) {
                // Two spellings on one side: the first one stands.
                side.entry(key(&rel)).or_insert((rel, stamp));
            }
        }
        Ok(side)
    }

    fn read_state(&self) -> Option<State> {
        fs::read_to_string(self.state)
            .ok()
            .map(|text| parse_state(&text, &self.single()))
    }

    /// Records every file either side has.
    fn record(&self) -> io::Result<()> {
        let (saves, runtime) = (self.side(self.saves)?, self.side(self.runtime)?);
        let names: BTreeSet<&String> = saves.keys().chain(runtime.keys()).collect();
        let state: State = names
            .into_iter()
            .map(|name| {
                let rec = Recorded {
                    saves: saves.get(name).map(|(_, s)| *s),
                    runtime: runtime.get(name).map(|(_, s)| *s),
                    coarse: false,
                };
                (name.clone(), rec)
            })
            .collect();
        fsx::write_atomic(self.state, format_state(&state))
    }

    /// Before the game starts: brings both sides in step and records the
    /// result. Returns the relative paths that went each way, for the log.
    /// Errors name the path.
    pub fn before(&self) -> io::Result<Moved> {
        let (s, r) = (self.side(self.saves)?, self.side(self.runtime)?);
        let stamps =
            |side: &Side| -> Stamps { side.iter().map(|(k, (_, st))| (k.clone(), *st)).collect() };
        let plan = plan(
            &stamps(&s),
            &stamps(&r),
            self.read_state().as_ref(),
            self.first,
            self.deleted,
        );
        let mut moved = Moved::default();
        for name in &plan.to_runtime {
            let rel = &s[name].0;
            let to = r.get(name).map_or(rel, |(spelled, _)| spelled);
            copy_stamped(&self.path(self.saves, rel), &self.path(self.runtime, to))?;
            moved.to_runtime.push(rel.clone());
        }
        for name in &plan.to_saves {
            let rel = &r[name].0;
            let to = s.get(name).map_or(rel, |(spelled, _)| spelled);
            copy_stamped(&self.path(self.runtime, rel), &self.path(self.saves, to))?;
            moved.to_saves.push(rel.clone());
        }
        self.record()?;
        Ok(moved)
    }

    /// After the game exits: every runtime file whose bytes differ from
    /// the saves folder's goes there (with [`Deleted::Stays`], not one the
    /// player removed from the saves folder while the runtime's stayed
    /// unchanged), and the state follows. Returns the files copied.
    pub fn after(&self) -> io::Result<Vec<String>> {
        let (s, r) = (self.side(self.saves)?, self.side(self.runtime)?);
        let state = self.read_state().unwrap_or_default();
        let mut copied = Vec::new();
        for (name, (rel, stamp)) in &r {
            let saved = s.get(name);
            if saved.is_none()
                && self.deleted == Deleted::Stays
                && state
                    .get(name)
                    .is_some_and(|rec| rec.saves.is_none() && rec.same(rec.runtime, Some(*stamp)))
            {
                continue;
            }
            let to = saved.map_or(rel, |(spelled, _)| spelled);
            let (from, to) = (self.path(self.runtime, rel), self.path(self.saves, to));
            let same = matches!((fs::read(&from), fs::read(&to)), (Ok(a), Ok(b)) if a == b);
            if !same {
                copy_stamped(&from, &to)?;
                copied.push(rel.clone());
            }
        }
        self.record()?;
        Ok(copied)
    }
}

/// Copies `from` over `to` as a fresh file with `from`'s exact time.
fn copy_stamped(from: &Path, to: &Path) -> io::Result<()> {
    let modified = fs::metadata(from)
        .and_then(|m| m.modified())
        .map_err(at(from))?;
    fsx::install_copy(from, to)?;
    fs::OpenOptions::new()
        .write(true)
        .open(to)
        .and_then(|f| f.set_modified(modified))
        .map_err(at(to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;
    use std::time::{Duration, SystemTime};

    /// Writes `bytes` with a modification time `secs_ago` seconds ago.
    fn put(path: &Path, bytes: &[u8], secs_ago: u64) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(secs_ago))
            .unwrap();
    }

    /// A runtime folder, a saves folder, the profile in each, the state.
    struct Profile {
        rt_dir: PathBuf,
        saves_dir: PathBuf,
        rt: PathBuf,
        saves: PathBuf,
        state: PathBuf,
    }

    impl Profile {
        fn new(name: &str) -> Profile {
            let rt_dir = test_dir(&format!("{name}-rt"));
            let saves_dir = test_dir(&format!("{name}-saves"));
            Profile {
                rt: rt_dir.join("CMR3"),
                saves: saves_dir.join("CMR3"),
                state: rt_dir.join(".profile-state"),
                rt_dir,
                saves_dir,
            }
        }

        fn mirror(&self) -> Mirror<'_> {
            Mirror::file(&self.rt, &self.saves, &self.state, FirstRun::NewerWins)
        }
    }

    impl Drop for Profile {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.rt_dir);
            let _ = fs::remove_dir_all(&self.saves_dir);
        }
    }

    #[test]
    fn a_normal_session_saves_the_profile_at_exit() {
        let p = Profile::new("saves-exit");
        put(&p.saves, b"start", 100);
        assert_eq!(p.mirror().before().unwrap().to_runtime, ["CMR3"]);
        assert_eq!(fs::read(&p.rt).unwrap(), b"start");
        assert!(p.mirror().before().unwrap().is_empty());
        put(&p.rt, b"played", 0);
        assert_eq!(p.mirror().after().unwrap(), ["CMR3"]);
        assert_eq!(fs::read(&p.saves).unwrap(), b"played");
        assert!(p.mirror().after().unwrap().is_empty());
        assert!(p.mirror().before().unwrap().is_empty());
    }

    #[test]
    fn a_restored_older_backup_wins() {
        let p = Profile::new("saves-backup");
        put(&p.saves, b"start", 1000);
        p.mirror().before().unwrap();
        put(&p.rt, b"latest", 10);
        p.mirror().after().unwrap();
        // The player copies an older backup into the saves folder.
        put(&p.saves, b"backup", 5000);
        assert_eq!(p.mirror().before().unwrap().to_runtime, ["CMR3"]);
        assert_eq!(fs::read(&p.rt).unwrap(), b"backup");
        assert!(p.mirror().after().unwrap().is_empty());
        assert_eq!(fs::read(&p.saves).unwrap(), b"backup");
    }

    #[test]
    fn a_session_killed_before_saving_is_recovered() {
        let p = Profile::new("saves-crash");
        put(&p.saves, b"start", 1000);
        p.mirror().before().unwrap();
        // The game saved, then the launcher was killed: no `after`.
        put(&p.rt, b"progress", 10);
        assert_eq!(p.mirror().before().unwrap().to_saves, ["CMR3"]);
        assert_eq!(fs::read(&p.saves).unwrap(), b"progress");
        assert_eq!(fs::read(&p.rt).unwrap(), b"progress");
    }

    #[test]
    fn without_a_record_the_stated_side_wins() {
        let p = Profile::new("saves-first-newer");
        put(&p.saves, b"old", 1000);
        put(&p.rt, b"new", 10);
        p.mirror().before().unwrap();
        assert_eq!(fs::read(&p.saves).unwrap(), b"new");
        assert!(p.state.is_file(), "recorded from now on");

        let q = Profile::new("saves-first-saves");
        put(&q.saves, b"old", 1000);
        put(&q.rt, b"new", 10);
        Mirror::file(&q.rt, &q.saves, &q.state, FirstRun::SavesWin)
            .before()
            .unwrap();
        assert_eq!(fs::read(&q.rt).unwrap(), b"old");
    }

    #[test]
    fn a_same_second_same_size_change_still_syncs() {
        let p = Profile::new("saves-same-second");
        put(&p.rt, b"aaaa", 50);
        p.mirror().before().unwrap();
        // Rewritten with the same size inside the same second.
        let when = fs::metadata(&p.rt).unwrap().modified().unwrap();
        fs::write(&p.rt, b"bbbb").unwrap();
        let f = fs::OpenOptions::new().write(true).open(&p.rt).unwrap();
        f.set_modified(when).unwrap();
        drop(f);
        assert_eq!(p.mirror().after().unwrap(), ["CMR3"]);
        assert_eq!(fs::read(&p.saves).unwrap(), b"bbbb");
    }

    #[test]
    fn a_missing_side_is_filled_by_default() {
        let p = Profile::new("saves-missing");
        put(&p.saves, b"start", 100);
        p.mirror().before().unwrap();
        fs::remove_file(&p.saves).unwrap();
        assert_eq!(p.mirror().before().unwrap().to_saves, ["CMR3"]);
        assert_eq!(fs::read(&p.saves).unwrap(), b"start");
    }

    #[test]
    fn a_save_the_game_deleted_can_stay_deleted() {
        let rt = test_dir("saves-deleted-rt");
        let saves = test_dir("saves-deleted-saves");
        let state = rt.join(".state");
        put(&saves.join("slot1.dat"), b"one", 100);
        put(&saves.join("slot2.dat"), b"two", 100);
        let keep = |rel: &str| rel.ends_with(".dat");
        let mirror = Mirror::folder(&rt, &saves, &state, FirstRun::SavesWin)
            .only(&keep)
            .deleted(Deleted::Stays);
        mirror.before().unwrap();
        // The game deletes a slot; the player deletes the other backup.
        fs::remove_file(rt.join("slot1.dat")).unwrap();
        assert!(mirror.after().unwrap().is_empty());
        fs::remove_file(saves.join("slot2.dat")).unwrap();
        for _ in 0..2 {
            assert!(mirror.before().unwrap().is_empty(), "nothing comes back");
            assert!(mirror.after().unwrap().is_empty());
        }
        assert!(!rt.join("slot1.dat").exists());
        assert!(
            saves.join("slot1.dat").exists(),
            "never deleted on the other side"
        );
        assert!(!saves.join("slot2.dat").exists());
        // A slot the game writes again goes out as usual.
        put(&rt.join("slot2.dat"), b"two again", 0);
        assert_eq!(mirror.after().unwrap(), ["slot2.dat"]);
        for d in [rt, saves] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn names_match_without_regard_to_case() {
        let rt = test_dir("saves-case-rt");
        let saves = test_dir("saves-case-saves");
        let state = rt.join(".state");
        put(&rt.join("CHOICE.DAT"), b"game", 100);
        put(&saves.join("choice.dat"), b"backup", 5000);
        put(&saves.join("config/a.sav"), b"cfg", 100);
        fs::create_dir_all(rt.join("CONFIG")).unwrap();
        let mirror = Mirror::folder(&rt, &saves, &state, FirstRun::SavesWin);
        let moved = mirror.before().unwrap();
        assert_eq!(moved.to_runtime, ["choice.dat", "config/a.sav"]);
        // Written over the runtime's own spelling, into its own folder.
        assert_eq!(fs::read(rt.join("CHOICE.DAT")).unwrap(), b"backup");
        assert!(!rt.join("choice.dat").exists() || cfg!(not(unix)));
        assert_eq!(fs::read(rt.join("CONFIG/a.sav")).unwrap(), b"cfg");
        assert!(mirror.before().unwrap().is_empty());
        for d in [rt, saves] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn scanning_only_some_folders_not_below_them() {
        let rt = test_dir("saves-scan-rt");
        let saves = test_dir("saves-scan-saves");
        let state = rt.join(".state");
        for rel in [
            "A.SAV",
            "CONFIG/B.SAV",
            "REPLAYS/C.RPL",
            "REPLAYS/OLD/D.RPL",
            "DATA/E.SAV",
        ] {
            put(&native(&rt, rel), b"x", 10);
        }
        let keep = |rel: &str| {
            let upper = rel.to_ascii_uppercase();
            upper.ends_with(".SAV") || upper.ends_with(".RPL")
        };
        let mirror = Mirror::folder(&rt, &saves, &state, FirstRun::NewerWins)
            .scan(&["", "config", "REPLAYS"])
            .only(&keep);
        let moved = mirror.before().unwrap();
        assert_eq!(moved.to_saves, ["A.SAV", "CONFIG/B.SAV", "REPLAYS/C.RPL"]);
        for d in [rt, saves] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn plan_follows_what_changed_since_the_record() {
        let st = |size, s: u128| SaveStamp {
            size,
            mtime_ns: s * 1_000_000_000,
        };
        let stamps = |items: &[(&str, SaveStamp)]| -> Stamps {
            items.iter().map(|(r, s)| (r.to_string(), *s)).collect()
        };
        let rec = |s, r| Recorded {
            saves: Some(s),
            runtime: Some(r),
            coarse: false,
        };
        let state: State = [
            ("a".to_string(), rec(st(1, 1), st(1, 1))),
            ("b".to_string(), rec(st(1, 1), st(1, 1))),
            ("c".to_string(), rec(st(1, 1), st(1, 1))),
            ("d".to_string(), rec(st(1, 1), st(1, 1))),
        ]
        .into();
        let saves = stamps(&[
            ("a", st(1, 0)),
            ("b", st(1, 1)),
            ("c", st(1, 1)),
            ("d", st(1, 1)),
            ("n", st(5, 5)),
        ]);
        let runtime = stamps(&[
            ("a", st(2, 9)),
            ("b", st(2, 9)),
            ("c", st(1, 1)),
            ("o", st(5, 5)),
        ]);
        let p = |state, first, deleted| plan(&saves, &runtime, state, first, deleted);
        // a: the saves side changed (older): it wins over a newer runtime.
        let moved = p(Some(&state), FirstRun::SavesWin, Deleted::Restore);
        assert_eq!(moved.to_runtime, ["a", "d", "n"]);
        assert_eq!(moved.to_saves, ["b", "o"]);
        // d was deleted in the runtime: with Stays it stays gone.
        let moved = p(Some(&state), FirstRun::SavesWin, Deleted::Stays);
        assert_eq!(moved.to_runtime, ["a", "n"]);
        // Never synced: saves win, or the newer copy.
        let moved = p(None, FirstRun::SavesWin, Deleted::Restore);
        assert_eq!(moved.to_runtime, ["a", "b", "d", "n"]);
        let moved = p(None, FirstRun::NewerWins, Deleted::Restore);
        assert_eq!(moved.to_runtime, ["d", "n"]);
        assert_eq!(moved.to_saves, ["a", "b", "o"]);
    }

    #[test]
    fn state_round_trips_and_older_formats_are_read() {
        let state: State = [
            (
                "savegame/a b.sav".to_string(),
                Recorded {
                    saves: Some(SaveStamp {
                        size: 1,
                        mtime_ns: 1_700_000_000_123_456_789,
                    }),
                    runtime: None,
                    coarse: false,
                },
            ),
            (
                "b".to_string(),
                Recorded {
                    saves: None,
                    runtime: Some(SaveStamp {
                        size: 2,
                        mtime_ns: 3,
                    }),
                    coarse: false,
                },
            ),
        ]
        .into();
        assert_eq!(parse_state(&format_state(&state), "x"), state);
        // CMR3: one profile, no name; OR: one stamp per name; SCR: upper
        // case names.
        let cmr3 = parse_state("12\t1700000000\n", "CMR3");
        assert!(cmr3["cmr3"].coarse);
        assert_eq!(
            cmr3["cmr3"].saves.unwrap().mtime_ns,
            1_700_000_000_000_000_000
        );
        let or = parse_state("3\t5\tSlot 1.dat\n", "");
        assert_eq!(or["slot 1.dat"].runtime.unwrap().size, 3);
        let scr = parse_state("1\t2\t3\t4\tCONFIG/CHOICE.DAT\n", "");
        assert_eq!(scr["config/choice.dat"].runtime.unwrap().size, 3);
        // A coarse record matches any time within its second.
        let rec = or["slot 1.dat"];
        let now = SaveStamp {
            size: 3,
            mtime_ns: 5_999_999_999,
        };
        assert!(rec.same(rec.saves, Some(now)));
        assert!(!rec.same(rec.saves, None));
        assert!(parse_state("junk\n\t\n1\t99999999999999999999999999999999\n", "x").is_empty());
    }

    #[test]
    fn folders_round_trip_through_the_runtime() {
        let rt = test_dir("saves-folder-rt");
        let saves = test_dir("saves-folder-saves");
        let state = rt.join(".saves-state");
        let rt_save = rt.join("SaveGame");
        put(&saves.join("slot1.dat"), b"one", 100);
        put(&saves.join("sub/slot2.dat"), b"two", 100);
        put(&saves.join("notes.txt"), b"skip", 100);
        let keep = |rel: &str| rel.ends_with(".dat");
        let mirror = Mirror::folder(&rt_save, &saves, &state, FirstRun::SavesWin).only(&keep);
        let moved = mirror.before().unwrap();
        assert_eq!(moved.to_runtime, ["slot1.dat", "sub/slot2.dat"]);
        assert!(!rt_save.join("notes.txt").exists(), "filtered out");
        assert_eq!(
            fs::metadata(rt_save.join("slot1.dat"))
                .unwrap()
                .modified()
                .unwrap(),
            fs::metadata(saves.join("slot1.dat"))
                .unwrap()
                .modified()
                .unwrap(),
            "copies keep the exact time"
        );
        put(&rt_save.join("slot3.dat"), b"three", 0);
        put(&rt_save.join("slot1.dat"), b"ONE", 0);
        let mut copied = mirror.after().unwrap();
        copied.sort();
        assert_eq!(copied, ["slot1.dat", "slot3.dat"]);
        assert_eq!(fs::read(saves.join("slot1.dat")).unwrap(), b"ONE");
        assert!(mirror.before().unwrap().is_empty());
        // A missing saves folder is just empty.
        let none = test_dir("saves-folder-none");
        fs::remove_dir(&none).unwrap();
        let fresh = Mirror::folder(&rt_save, &none, &state, FirstRun::SavesWin);
        assert_eq!(fresh.before().unwrap().to_saves.len(), 3);
        for d in [rt, saves, none] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn errors_name_the_path() {
        use std::os::unix::fs::PermissionsExt;
        let rt = test_dir("saves-err-rt");
        let saves = test_dir("saves-err-saves");
        put(&saves.join("a.sav"), b"x", 1);
        fs::set_permissions(saves.join("a.sav"), fs::Permissions::from_mode(0o000)).unwrap();
        let state = rt.join(".state");
        let result = Mirror::folder(&rt, &saves, &state, FirstRun::SavesWin).before();
        fs::set_permissions(saves.join("a.sav"), fs::Permissions::from_mode(0o644)).unwrap();
        // Root reads anything; only check when the lock held.
        if let Err(err) = result {
            assert!(err.to_string().contains("a.sav"), "{err}");
        }
        for d in [rt, saves] {
            fs::remove_dir_all(d).unwrap();
        }
    }
}
