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
//! - a file with no recorded state follows [`FirstRun`] (OR: the saves
//!   folder wins; CMR3/SCR: the newer copy wins).
//!
//! Nothing is ever deleted: a save missing on one side is copied back from
//! the other. Copies are whole-file swaps keeping the source's exact time.
//! State files written by CMR3 (`size\tmtime`), OR (`size\tmtime\trel`)
//! and SCR (`size\tns\tsize\tns\trel`) are read; whole-second records
//! compare at whole seconds.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::fsx;
use crate::paths::native;
use crate::runtime;

/// What wins for a file both sides have but no state records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FirstRun {
    /// The saves folder's copy (OR).
    #[default]
    SavesWin,
    /// The copy with the later modification time; the runtime's on a tie
    /// (CMR3, SCR).
    NewerWins,
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

/// Both sides of one file as of the last sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recorded {
    pub saves: SaveStamp,
    pub runtime: SaveStamp,
    /// Read from an older whole-second record: compared at whole seconds.
    pub coarse: bool,
}

impl Recorded {
    fn same(&self, recorded: SaveStamp, now: SaveStamp) -> bool {
        const NS: u128 = 1_000_000_000;
        if self.coarse {
            recorded.size == now.size && recorded.mtime_ns / NS == now.mtime_ns / NS
        } else {
            recorded == now
        }
    }
}

/// Files by relative path.
pub type Stamps = BTreeMap<String, SaveStamp>;
/// The recorded state by relative path.
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
pub fn plan(saves: &Stamps, runtime: &Stamps, state: Option<&State>, first: FirstRun) -> Moved {
    let mut moved = Moved::default();
    let names: BTreeSet<&String> = saves.keys().chain(runtime.keys()).collect();
    for rel in names {
        let (s, r) = (saves.get(rel).copied(), runtime.get(rel).copied());
        let to_runtime = match (s, r, state.and_then(|st| st.get(rel))) {
            (Some(_), None, _) => true,
            (None, Some(_), _) => false,
            (Some(s), Some(r), Some(rec)) => {
                if !rec.same(rec.saves, s) {
                    true
                } else if !rec.same(rec.runtime, r) {
                    false
                } else {
                    continue;
                }
            }
            (Some(s), Some(r), None) => {
                if s == r {
                    continue;
                }
                match first {
                    FirstRun::SavesWin => true,
                    FirstRun::NewerWins => s.mtime_ns > r.mtime_ns,
                }
            }
            (None, None, _) => continue,
        };
        if to_runtime {
            moved.to_runtime.push(rel.clone());
        } else {
            moved.to_saves.push(rel.clone());
        }
    }
    moved
}

/// Reads a state file (any of the formats in the module documentation).
/// `single` names the entry of a one-file mirror, for CMR3's record that
/// has no name.
pub fn parse_state(text: &str, single: &str) -> State {
    let mut state = State::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| fields.get(i).and_then(|f| f.parse::<u128>().ok());
        let stamp = |size: Option<u128>, time: Option<u128>| {
            Some(SaveStamp {
                size: u64::try_from(size?).ok()?,
                mtime_ns: time?,
            })
        };
        let entry = match fields.len() {
            2 => stamp(num(0), num(1)).map(|s| (single.to_owned(), s, s, true)),
            3 => stamp(num(0), num(1)).map(|s| (fields[2].to_owned(), s, s, true)),
            n if n >= 5 => stamp(num(0), num(1))
                .zip(stamp(num(2), num(3)))
                .map(|(s, r)| (fields[4..].join("\t"), s, r, false)),
            _ => None,
        };
        if let Some((rel, saves, runtime, coarse)) = entry {
            // Whole seconds to nanoseconds.
            let scale = |s: SaveStamp| SaveStamp {
                mtime_ns: if coarse {
                    s.mtime_ns * 1_000_000_000
                } else {
                    s.mtime_ns
                },
                ..s
            };
            state.insert(
                rel,
                Recorded {
                    saves: scale(saves),
                    runtime: scale(runtime),
                    coarse,
                },
            );
        }
    }
    state
}

/// Writes a state file in the precise five-field format.
pub fn format_state(state: &State) -> String {
    state
        .iter()
        .map(|(rel, r)| {
            format!(
                "{}\t{}\t{}\t{}\t{rel}\n",
                r.saves.size, r.saves.mtime_ns, r.runtime.size, r.runtime.mtime_ns
            )
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Kind {
    File,
    Folder,
}

/// One mirrored save: a single file or a folder, its runtime and saves
/// sides, and the state file that records the last sync (keep it in the
/// runtime folder).
pub struct Mirror<'a> {
    runtime: &'a Path,
    saves: &'a Path,
    state: &'a Path,
    kind: Kind,
    first: FirstRun,
    filter: Option<&'a dyn Fn(&str) -> bool>,
}

impl<'a> Mirror<'a> {
    /// A single file (CMR3's `CMR3` profile).
    pub fn file(runtime: &'a Path, saves: &'a Path, state: &'a Path) -> Mirror<'a> {
        Mirror::new(runtime, saves, state, Kind::File)
    }

    /// Every file below a folder (OR's `SaveGame`).
    pub fn folder(runtime: &'a Path, saves: &'a Path, state: &'a Path) -> Mirror<'a> {
        Mirror::new(runtime, saves, state, Kind::Folder)
    }

    fn new(runtime: &'a Path, saves: &'a Path, state: &'a Path, kind: Kind) -> Mirror<'a> {
        Mirror {
            runtime,
            saves,
            state,
            kind,
            first: FirstRun::default(),
            filter: None,
        }
    }

    /// What wins without a record (default [`FirstRun::SavesWin`]).
    pub fn first_run(mut self, first: FirstRun) -> Mirror<'a> {
        self.first = first;
        self
    }

    /// Mirrors only the relative paths `keep` accepts (SCR: `*.SAV`,
    /// `CHOICE.DAT`, `REPLAYS/*.RPL`).
    pub fn only(mut self, keep: &'a dyn Fn(&str) -> bool) -> Mirror<'a> {
        self.filter = Some(keep);
        self
    }

    /// The name of the one entry of a file mirror.
    fn single(&self) -> String {
        self.runtime
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    fn path(&self, root: &Path, rel: &str) -> PathBuf {
        match self.kind {
            Kind::File => root.to_path_buf(),
            Kind::Folder => native(root, rel),
        }
    }

    fn stamps(&self, root: &Path) -> io::Result<Stamps> {
        let rels: Vec<String> = match self.kind {
            Kind::File => vec![self.single()],
            Kind::Folder if root.is_dir() => runtime::list(root)?.into_keys().collect(),
            Kind::Folder => Vec::new(),
        };
        Ok(rels
            .into_iter()
            .filter(|rel| self.filter.is_none_or(|keep| keep(rel)))
            .filter_map(|rel| SaveStamp::of(&self.path(root, &rel)).map(|s| (rel, s)))
            .collect())
    }

    fn read_state(&self) -> Option<State> {
        fs::read_to_string(self.state)
            .ok()
            .map(|text| parse_state(&text, &self.single()))
    }

    /// Records every file both sides have.
    fn record(&self) -> io::Result<()> {
        let runtime = self.stamps(self.runtime)?;
        let state: State = self
            .stamps(self.saves)?
            .into_iter()
            .filter_map(|(rel, saves)| {
                let runtime = *runtime.get(&rel)?;
                Some((
                    rel,
                    Recorded {
                        saves,
                        runtime,
                        coarse: false,
                    },
                ))
            })
            .collect();
        fsx::write_atomic(self.state, format_state(&state))
    }

    /// Before the game starts: brings both sides in step and records the
    /// result. Returns what went each way, for the log.
    pub fn before(&self) -> io::Result<Moved> {
        let (s, r) = (self.stamps(self.saves)?, self.stamps(self.runtime)?);
        let moved = plan(&s, &r, self.read_state().as_ref(), self.first);
        for rel in &moved.to_runtime {
            copy_stamped(&self.path(self.saves, rel), &self.path(self.runtime, rel))?;
        }
        for rel in &moved.to_saves {
            copy_stamped(&self.path(self.runtime, rel), &self.path(self.saves, rel))?;
        }
        self.record()?;
        Ok(moved)
    }

    /// After the game exits: every runtime file whose bytes differ from
    /// the saves folder's goes there, and the state follows. Returns the
    /// files copied.
    pub fn after(&self) -> io::Result<Vec<String>> {
        let mut copied = Vec::new();
        for rel in self.stamps(self.runtime)?.into_keys() {
            let (from, to) = (self.path(self.runtime, &rel), self.path(self.saves, &rel));
            let same = matches!((fs::read(&from), fs::read(&to)), (Ok(a), Ok(b)) if a == b);
            if !same {
                copy_stamped(&from, &to)?;
                copied.push(rel);
            }
        }
        self.record()?;
        Ok(copied)
    }
}

/// Copies `from` over `to` as a fresh file with `from`'s exact time.
fn copy_stamped(from: &Path, to: &Path) -> io::Result<()> {
    let modified = fs::metadata(from)?.modified()?;
    fsx::install_copy(from, to)?;
    fs::OpenOptions::new()
        .write(true)
        .open(to)?
        .set_modified(modified)
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
            Mirror::file(&self.rt, &self.saves, &self.state).first_run(FirstRun::NewerWins)
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
    fn without_a_record_the_configured_side_wins() {
        let p = Profile::new("saves-first-newer");
        put(&p.saves, b"old", 1000);
        put(&p.rt, b"new", 10);
        p.mirror().before().unwrap();
        assert_eq!(fs::read(&p.saves).unwrap(), b"new");
        assert!(p.state.is_file(), "recorded from now on");

        let q = Profile::new("saves-first-saves");
        put(&q.saves, b"old", 1000);
        put(&q.rt, b"new", 10);
        Mirror::file(&q.rt, &q.saves, &q.state).before().unwrap();
        assert_eq!(fs::read(&q.rt).unwrap(), b"old", "SavesWin is the default");
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
    fn a_missing_side_is_filled_never_deleted() {
        let p = Profile::new("saves-missing");
        put(&p.saves, b"start", 100);
        p.mirror().before().unwrap();
        fs::remove_file(&p.saves).unwrap();
        assert_eq!(p.mirror().before().unwrap().to_saves, ["CMR3"]);
        assert_eq!(fs::read(&p.saves).unwrap(), b"start");
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
            saves: s,
            runtime: r,
            coarse: false,
        };
        let state: State = [
            ("a".to_string(), rec(st(1, 1), st(1, 1))),
            ("b".to_string(), rec(st(1, 1), st(1, 1))),
            ("c".to_string(), rec(st(1, 1), st(1, 1))),
        ]
        .into();
        let saves = stamps(&[
            ("a", st(1, 0)),
            ("b", st(1, 1)),
            ("c", st(1, 1)),
            ("n", st(5, 5)),
        ]);
        let runtime = stamps(&[
            ("a", st(2, 9)),
            ("b", st(2, 9)),
            ("c", st(1, 1)),
            ("o", st(5, 5)),
        ]);
        let moved = plan(&saves, &runtime, Some(&state), FirstRun::SavesWin);
        // a: the saves side changed (older): it wins over a newer runtime.
        assert_eq!(moved.to_runtime, ["a", "n"]);
        assert_eq!(moved.to_saves, ["b", "o"]);
        // Never synced: saves win, or the newer copy.
        let moved = plan(&saves, &runtime, None, FirstRun::SavesWin);
        assert_eq!(moved.to_runtime, ["a", "b", "n"]);
        let moved = plan(&saves, &runtime, None, FirstRun::NewerWins);
        assert_eq!(moved.to_runtime, ["n"]);
        assert_eq!(moved.to_saves, ["a", "b", "o"]);
    }

    #[test]
    fn state_round_trips_and_older_formats_are_read() {
        let state: State = [(
            "SaveGame/a b.sav".to_string(),
            Recorded {
                saves: SaveStamp {
                    size: 1,
                    mtime_ns: 1_700_000_000_123_456_789,
                },
                runtime: SaveStamp {
                    size: 1,
                    mtime_ns: 1_700_000_000_000_000_000,
                },
                coarse: false,
            },
        )]
        .into();
        assert_eq!(parse_state(&format_state(&state), "x"), state);
        // CMR3: one profile, no name; OR: one stamp per name.
        let cmr3 = parse_state("12\t1700000000\n", "CMR3");
        assert!(cmr3["CMR3"].coarse);
        assert_eq!(cmr3["CMR3"].saves.mtime_ns, 1_700_000_000_000_000_000);
        let or = parse_state("3\t5\tslot 1.dat\n", "");
        assert_eq!(or["slot 1.dat"].runtime.size, 3);
        // A coarse record matches any time within its second.
        let rec = or["slot 1.dat"];
        let now = SaveStamp {
            size: 3,
            mtime_ns: 5_999_999_999,
        };
        assert!(rec.same(rec.saves, now));
        assert!(parse_state("junk\n\t\n", "x").is_empty());
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
        let mirror = Mirror::folder(&rt_save, &saves, &state).only(&keep);
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
        let fresh = Mirror::folder(&rt_save, &none, &state);
        assert_eq!(fresh.before().unwrap().to_saves.len(), 3);
        for d in [rt, saves, none] {
            fs::remove_dir_all(d).unwrap();
        }
    }
}
