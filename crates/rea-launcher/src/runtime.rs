//! The runtime folder: a private, writable copy of the player's game folder.
//!
//! From CMR3's link + manifest engine, the one the audit picked: the game
//! folder is only read; static data may be hard-linked (near-instant setup
//! on one volume), everything else is copied, and every later write is a
//! whole-file swap ([`crate::fsx`]). Fixes folded in: read-only originals
//! are copied, never linked (CMR3 bdda455 — a link shares the attribute);
//! a symlinked original links its real target (CMR3 ea6af9d / SCR 6edc244
//! — a hard link to a relative symlink dangles); a link's time is never
//! set (CMR3 bc654e6 — it is the player's file); unreadable and looping
//! folders are skipped with a warning (SCR 950ef68). OR's "owned" files
//! (the launcher places its own) are recorded but never copied.
//!
//! A manifest (`size\tmtime\trel` lines, mtime in whole seconds) records
//! each source file as last brought over, so later launches bring only what
//! changed there.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use crate::fsx;
use crate::paths::native;

/// A source file's size and modification time (whole seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub mtime: u64,
}

impl Stamp {
    pub fn of(meta: &fs::Metadata) -> Stamp {
        Stamp {
            size: meta.len(),
            mtime: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs()),
        }
    }
}

/// Relative path (`/`-separated) to its stamp.
pub type Listing = BTreeMap<String, Stamp>;

/// What the game knows about its own files. Every method has a safe
/// default: copy everything, overwrite changed files, own nothing.
pub trait Policy {
    /// Static data the game only reads: may be hard-linked into the
    /// runtime (falling back to a copy). Never return true for anything
    /// the game opens for writing.
    fn link(&self, _rel: &str) -> bool {
        false
    }

    /// Files the game writes (saves, its own settings): copied once, then
    /// never overwritten by a later change in the source.
    fn game_written(&self, _rel: &str) -> bool {
        false
    }

    /// Files the launcher places itself (wrappers, generated configs, a
    /// repack's stray DLL): recorded in the manifest, never copied.
    fn owned(&self, _rel: &str) -> bool {
        false
    }

    /// Where `rel` goes in the runtime folder (SCR stages DOS names in
    /// upper case). The identity by default.
    fn runtime_rel(&self, rel: &str) -> String {
        rel.to_owned()
    }
}

/// The default [`Policy`]: copy everything, own nothing.
pub struct CopyAll;

impl Policy for CopyAll {}

/// Which files to bring over, and the manifest to write once they are in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub copy: Vec<String>,
    pub manifest: Listing,
}

/// What [`sync`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Files brought over (linked or copied).
    pub brought: usize,
    /// How many of those were hard-linked.
    pub linked: usize,
    /// Folders and names skipped while listing the source, for the log.
    pub warnings: Vec<String>,
}

/// Decides which source files to bring over: missing in the runtime, or
/// changed since the last sync (per `manifest`) unless the game writes
/// them. Owned files are recorded but never brought.
pub fn plan(
    source: &Listing,
    runtime_has: &dyn Fn(&str) -> bool,
    manifest: &Listing,
    policy: &dyn Policy,
) -> Plan {
    let mut copy = Vec::new();
    let mut next = Listing::new();
    for (rel, stamp) in source {
        let recorded = manifest.get(rel);
        if policy.owned(rel) {
            // Kept so a file an older launcher once copied is not seen as
            // new forever.
            next.insert(rel.clone(), *recorded.unwrap_or(stamp));
            continue;
        }
        let changed = recorded != Some(stamp);
        if !runtime_has(rel) || (changed && !policy.game_written(rel)) {
            copy.push(rel.clone());
            next.insert(rel.clone(), *stamp);
        } else {
            next.insert(rel.clone(), *recorded.unwrap_or(stamp));
        }
    }
    Plan {
        copy,
        manifest: next,
    }
}

/// Reads a manifest; malformed lines are skipped.
pub fn parse_manifest(text: &str) -> Listing {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let size = parts.next()?.parse().ok()?;
            let mtime = parts.next()?.parse().ok()?;
            let rel = parts.next()?.to_owned();
            Some((rel, Stamp { size, mtime }))
        })
        .collect()
}

pub fn format_manifest(listing: &Listing) -> String {
    listing
        .iter()
        .map(|(rel, s)| format!("{}\t{}\t{}\n", s.size, s.mtime, rel))
        .collect()
}

/// Deepest folder level listed (a guard where folder identities are not
/// available).
const MAX_DEPTH: usize = 32;

/// Every regular file below `root`. See [`list_with_warnings`].
pub fn list(root: &Path) -> io::Result<Listing> {
    list_with_warnings(root).map(|(listing, _)| listing)
}

/// Every regular file below `root`, plus what was skipped. Symbolic links
/// are followed (a linked file counts as its target; a dangling one is
/// skipped); a folder already visited (a link loop) is not entered again;
/// an unreadable subfolder or a name that is not Unicode is skipped with a
/// warning. Only an unreadable `root` is an error.
pub fn list_with_warnings(root: &Path) -> io::Result<(Listing, Vec<String>)> {
    let mut walk = Walk {
        root,
        out: Listing::new(),
        warnings: Vec::new(),
        seen: BTreeSet::new(),
    };
    if let Ok(meta) = fs::metadata(root) {
        walk.first_visit(root, &meta);
    }
    walk.dir(root, 0)?;
    Ok((walk.out, walk.warnings))
}

struct Walk<'a> {
    root: &'a Path,
    out: Listing,
    warnings: Vec<String>,
    seen: BTreeSet<FolderId>,
}

#[cfg(unix)]
type FolderId = (u64, u64);
#[cfg(not(unix))]
type FolderId = std::path::PathBuf;

impl Walk<'_> {
    /// False when `dir` was visited before.
    fn first_visit(&mut self, _dir: &Path, _meta: &fs::Metadata) -> bool {
        #[cfg(unix)]
        let id = {
            use std::os::unix::fs::MetadataExt;
            (_meta.dev(), _meta.ino())
        };
        #[cfg(not(unix))]
        let id = fs::canonicalize(_dir).unwrap_or_else(|_| _dir.to_path_buf());
        self.seen.insert(id)
    }

    fn dir(&mut self, dir: &Path, depth: usize) -> io::Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if depth > 0 => {
                self.warnings
                    .push(format!("skipped unreadable folder {}: {e}", dir.display()));
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                if depth + 1 > MAX_DEPTH || !self.first_visit(&path, &meta) {
                    self.warnings
                        .push(format!("skipped looping folder {}", path.display()));
                    continue;
                }
                self.dir(&path, depth + 1)?;
            } else if meta.is_file() {
                let parts: Option<Vec<&str>> = path
                    .strip_prefix(self.root)
                    .expect("the walk stays below root")
                    .components()
                    .map(|c| c.as_os_str().to_str())
                    .collect();
                match parts {
                    Some(parts) => {
                        self.out.insert(parts.join("/"), Stamp::of(&meta));
                    }
                    None => self
                        .warnings
                        .push(format!("skipped non-Unicode name {}", path.display())),
                }
            }
        }
        Ok(())
    }
}

/// Brings one file over as a whole-file swap: a hard link to the real file
/// when `link` and the original is writable (a read-only original is
/// copied: the link would share its attribute), else a writable copy with
/// the source's modification time. Returns true when linked.
fn bring_over(from: &Path, to: &Path, link: bool) -> io::Result<bool> {
    if link
        && fs::metadata(from).is_ok_and(|m| !m.permissions().readonly())
        && let Ok(real) = fs::canonicalize(from)
        && fsx::put_in_place(to, |tmp| fs::hard_link(&real, tmp)).is_ok()
    {
        // The link is the original: its time is already the source's, and
        // setting it would change the player's file.
        return Ok(true);
    }
    let modified = fs::metadata(from)?.modified().ok();
    fsx::install_copy(from, to)?;
    if let Some(when) = modified {
        // Whole seconds, as the manifest compares.
        let secs = when.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        fs::OpenOptions::new()
            .write(true)
            .open(to)?
            .set_modified(UNIX_EPOCH + Duration::from_secs(secs))?;
    }
    Ok(false)
}

/// Brings `runtime` up to date with `game`, recording the result in
/// `runtime/manifest_name`. Never deletes runtime-only files and never
/// writes into `game`.
pub fn sync(
    game: &Path,
    runtime: &Path,
    policy: &dyn Policy,
    manifest_name: &str,
) -> io::Result<SyncReport> {
    let (source, warnings) = list_with_warnings(game)?;
    fs::create_dir_all(runtime)?;
    let manifest_path = runtime.join(manifest_name);
    let manifest = fs::read_to_string(&manifest_path)
        .map(|t| parse_manifest(&t))
        .unwrap_or_default();
    let to = |rel: &str| native(runtime, &policy.runtime_rel(rel));
    let plan = plan(&source, &|rel| to(rel).is_file(), &manifest, policy);
    let mut linked = 0;
    for rel in &plan.copy {
        if bring_over(&native(game, rel), &to(rel), policy.link(rel))? {
            linked += 1;
        }
    }
    fsx::write_atomic(&manifest_path, format_manifest(&plan.manifest))?;
    Ok(SyncReport {
        brought: plan.copy.len(),
        linked,
        warnings,
    })
}

/// Puts the game's own `rel` back in the runtime folder (linked when
/// `link`, else copied), or removes the runtime's when the game has none.
pub fn restore(game: &Path, runtime: &Path, rel: &str, link: bool) -> io::Result<()> {
    let from = native(game, rel);
    let to = native(runtime, rel);
    if from.is_file() {
        bring_over(&from, &to, link).map(|_| ())
    } else {
        fsx::remove(&to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;
    use std::time::SystemTime;

    const MANIFEST: &str = ".test-manifest";

    /// CMR3's classification, as a policy.
    struct Cmr3;

    impl Policy for Cmr3 {
        fn link(&self, rel: &str) -> bool {
            let top = rel.split('/').next().unwrap_or_default();
            if matches!(top, "Data" | "fonts" | "shaders") {
                return true;
            }
            !rel.contains('/') && {
                let lower = rel.to_ascii_lowercase();
                [".dll", ".ico", ".exe", ".bin"]
                    .iter()
                    .any(|ext| lower.ends_with(ext))
            }
        }

        fn game_written(&self, rel: &str) -> bool {
            rel.eq_ignore_ascii_case("CMR3")
        }
    }

    /// OR's: everything copied, some files the launcher's own.
    struct OutRun;

    impl Policy for OutRun {
        fn game_written(&self, rel: &str) -> bool {
            rel.split('/')
                .next()
                .is_some_and(|top| top.eq_ignore_ascii_case("SaveGame"))
        }

        fn owned(&self, rel: &str) -> bool {
            ["d3d9.dll", "or2006c2c.exe"].contains(&rel.to_ascii_lowercase().as_str())
        }
    }

    fn stamp(size: u64, mtime: u64) -> Stamp {
        Stamp { size, mtime }
    }

    fn listing(items: &[(&str, Stamp)]) -> Listing {
        items.iter().map(|(r, s)| (r.to_string(), *s)).collect()
    }

    #[test]
    fn first_sync_brings_everything_over() {
        let src = listing(&[
            ("Rally_3PC.exe", stamp(1, 1)),
            ("Data/Tracks/a.dat", stamp(2, 2)),
            ("CMR3", stamp(3, 3)),
        ]);
        let p = plan(&src, &|_| false, &Listing::new(), &Cmr3);
        assert_eq!(p.copy, ["CMR3", "Data/Tracks/a.dat", "Rally_3PC.exe"]);
        assert_eq!(p.manifest, src);
    }

    #[test]
    fn unchanged_source_brings_nothing() {
        let src = listing(&[("Rally_3PC.exe", stamp(1, 1))]);
        assert!(plan(&src, &|_| true, &src, &Cmr3).copy.is_empty());
    }

    #[test]
    fn changed_data_is_brought_again_but_game_written_files_are_kept() {
        let old = listing(&[("Data/a.dat", stamp(1, 1)), ("CMR3", stamp(1, 1))]);
        let new = listing(&[("Data/a.dat", stamp(1, 9)), ("CMR3", stamp(5, 9))]);
        let p = plan(&new, &|_| true, &old, &Cmr3);
        assert_eq!(p.copy, ["Data/a.dat"]);
        assert_eq!(p.manifest["CMR3"], stamp(1, 1));
    }

    #[test]
    fn owned_files_stay_owned_even_after_an_older_launcher_copied_them() {
        let src = listing(&[("d3d9.dll", stamp(1, 1)), ("data.bin", stamp(1, 1))]);
        let p = plan(&src, &|_| false, &Listing::new(), &OutRun);
        assert_eq!(p.copy, ["data.bin"]);
        assert_eq!(p.manifest["d3d9.dll"], stamp(1, 1), "recorded");
        let older = listing(&[("d3d9.dll", stamp(9, 9))]);
        let p = plan(&src, &|_| true, &older, &OutRun);
        assert_eq!(p.manifest["d3d9.dll"], stamp(9, 9));
    }

    #[test]
    fn manifest_round_trips_with_spaces() {
        let l = listing(&[("Data/a b.dat", stamp(10, 20))]);
        assert_eq!(parse_manifest(&format_manifest(&l)), l);
        assert_eq!(parse_manifest("garbage\n1\tx\ty\n").len(), 0);
    }

    #[test]
    fn sync_links_data_and_keeps_the_original_safe() {
        let game = test_dir("rt-sync-game");
        let rt = test_dir("rt-sync-rt");
        fs::create_dir_all(game.join("Data/Tracks")).unwrap();
        fs::write(game.join("Rally_3PC.exe"), b"exe").unwrap();
        fs::write(game.join("Data/Tracks/a.dat"), b"data").unwrap();
        fs::write(game.join("CMR3"), b"profile").unwrap();

        let r = sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        assert_eq!((r.brought, r.linked), (3, 2), "the exe and the track file");
        assert_eq!(fs::read(rt.join("CMR3")).unwrap(), b"profile");
        let again = sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        assert_eq!((again.brought, again.linked), (0, 0));

        // Installing over a linked file must not change the original.
        fsx::install(&rt.join("Data/Tracks/a.dat"), b"replaced").unwrap();
        assert_eq!(fs::read(game.join("Data/Tracks/a.dat")).unwrap(), b"data");

        // The game writes its profile; a changed source file comes in, the
        // profile does not.
        fs::write(rt.join("CMR3"), b"played").unwrap();
        fs::write(game.join("CMR3"), b"other-machine").unwrap();
        let f = fs::OpenOptions::new()
            .write(true)
            .open(game.join("Rally_3PC.exe"))
            .unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(10))
            .unwrap();
        drop(f);
        assert_eq!(sync(&game, &rt, &Cmr3, MANIFEST).unwrap().brought, 1);
        assert_eq!(fs::read(rt.join("CMR3")).unwrap(), b"played");
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn copies_keep_the_source_time_and_owned_files_stay_out() {
        let game = test_dir("rt-copy-game");
        let rt = test_dir("rt-copy-rt");
        fs::write(game.join("d3d9.dll"), b"specialk").unwrap();
        fs::write(game.join("data.bin"), b"data").unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let f = fs::OpenOptions::new()
            .write(true)
            .open(game.join("data.bin"))
            .unwrap();
        f.set_modified(when).unwrap();
        drop(f);
        let r = sync(&game, &rt, &OutRun, MANIFEST).unwrap();
        assert_eq!((r.brought, r.linked), (1, 0));
        assert!(!rt.join("d3d9.dll").exists());
        let copied = fs::metadata(rt.join("data.bin")).unwrap();
        assert_eq!(copied.modified().unwrap(), when);
        assert!(
            parse_manifest(&fs::read_to_string(rt.join(MANIFEST)).unwrap())
                .contains_key("d3d9.dll")
        );
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn runtime_names_follow_the_policy() {
        struct Upper;
        impl Policy for Upper {
            fn runtime_rel(&self, rel: &str) -> String {
                rel.to_ascii_uppercase()
            }
        }
        let game = test_dir("rt-upper-game");
        let rt = test_dir("rt-upper-rt");
        fs::write(game.join("choice.dat"), b"x").unwrap();
        assert_eq!(sync(&game, &rt, &Upper, MANIFEST).unwrap().brought, 1);
        assert!(rt.join("CHOICE.DAT").is_file());
        assert_eq!(sync(&game, &rt, &Upper, MANIFEST).unwrap().brought, 0);
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn linked_originals_keep_their_time_and_attributes() {
        use std::os::unix::fs::PermissionsExt;
        let game = test_dir("rt-linked-meta-game");
        let rt = test_dir("rt-linked-meta-rt");
        fs::create_dir_all(game.join("Data")).unwrap();
        let original = game.join("Data/a.dat");
        fs::write(&original, b"data").unwrap();
        // A fraction of a second: a whole-second set_modified through the
        // link would show.
        let when = UNIX_EPOCH + Duration::from_millis(1_700_000_000_500);
        let f = fs::OpenOptions::new().write(true).open(&original).unwrap();
        f.set_modified(when).unwrap();
        drop(f);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;

        let r = sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        assert_eq!((r.brought, r.linked), (1, 1));
        assert_eq!(fs::metadata(&original).unwrap().modified().unwrap(), when);
        // A runtime from before read-only originals were copied: the link
        // shares the original's read-only mode.
        fs::set_permissions(&original, fs::Permissions::from_mode(0o444)).unwrap();
        fsx::install(&rt.join("Data/a.dat"), b"replaced").unwrap();
        assert_eq!(mode(&original), 0o444, "install over the link");
        fs::remove_file(rt.join("Data/a.dat")).unwrap();
        fs::hard_link(&original, rt.join("Data/a.dat")).unwrap();
        fsx::remove(&rt.join("Data/a.dat")).unwrap();
        assert_eq!(mode(&original), 0o444, "remove of the link");
        assert_eq!(fs::read(&original).unwrap(), b"data");
        assert_eq!(fs::metadata(&original).unwrap().modified().unwrap(), when);
        fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn read_only_originals_are_copied_not_linked() {
        use std::os::unix::fs::PermissionsExt;
        let game = test_dir("rt-ro-game");
        let rt = test_dir("rt-ro-rt");
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("Data/a.dat"), b"data").unwrap();
        fs::write(game.join("Data/b.dat"), b"more").unwrap();
        fs::set_permissions(game.join("Data/a.dat"), fs::Permissions::from_mode(0o444)).unwrap();
        let r = sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        assert_eq!((r.brought, r.linked), (2, 1), "only b.dat is linked");
        assert_eq!(fs::read(rt.join("Data/a.dat")).unwrap(), b"data");
        let copied = fs::metadata(rt.join("Data/a.dat")).unwrap().permissions();
        assert_eq!(copied.mode() & 0o777, 0o644, "writable by its owner only");
        fs::set_permissions(game.join("Data/a.dat"), fs::Permissions::from_mode(0o644)).unwrap();
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_originals_link_the_real_file() {
        use std::os::unix::fs::MetadataExt;
        let game = test_dir("rt-symlink-game");
        let rt = test_dir("rt-symlink-rt");
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::write(game.join("real.dat"), b"data").unwrap();
        // A relative target: a link to the symlink itself would dangle.
        std::os::unix::fs::symlink("../real.dat", game.join("Data/a.dat")).unwrap();
        std::os::unix::fs::symlink("missing", game.join("Data/gone.dat")).unwrap();
        // A copied (not linkable) symlinked file arrives as a real file too.
        std::os::unix::fs::symlink("real.dat", game.join("notes.txt")).unwrap();
        sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        let got = rt.join("Data/a.dat");
        assert!(!fs::symlink_metadata(&got).unwrap().file_type().is_symlink());
        assert_eq!(
            fs::metadata(&got).unwrap().ino(),
            fs::metadata(game.join("real.dat")).unwrap().ino()
        );
        let notes = rt.join("notes.txt");
        assert!(
            !fs::symlink_metadata(&notes)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(notes).unwrap(), b"data");
        assert!(!rt.join("Data/gone.dat").exists());
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_and_looping_folders_are_skipped_with_a_warning() {
        use std::os::unix::fs::PermissionsExt;
        let game = test_dir("rt-loop-game");
        fs::create_dir_all(game.join("sub")).unwrap();
        fs::write(game.join("sub/a.dat"), b"a").unwrap();
        std::os::unix::fs::symlink("..", game.join("sub/up")).unwrap();
        fs::create_dir_all(game.join("locked")).unwrap();
        fs::set_permissions(game.join("locked"), fs::Permissions::from_mode(0o000)).unwrap();
        let (listing, warnings) = list_with_warnings(&game).unwrap();
        assert_eq!(listing.keys().collect::<Vec<_>>(), ["sub/a.dat"]);
        assert!(
            warnings.iter().any(|w| w.contains("looping")),
            "{warnings:?}"
        );
        // Root ignores mode bits, so only check when the lock held.
        if fs::read_dir(game.join("locked")).is_err() {
            assert!(warnings.iter().any(|w| w.contains("unreadable")));
        }
        fs::set_permissions(game.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            list(&game.join("missing")).is_err(),
            "only the root is fatal"
        );
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn relinking_the_same_original_leaves_no_temp() {
        let game = test_dir("rt-relink-game");
        let rt = test_dir("rt-relink-rt");
        fs::write(game.join("Rally_3PC.exe"), b"exe").unwrap();
        sync(&game, &rt, &Cmr3, MANIFEST).unwrap();
        // The runtime already holds a link to this very file.
        restore(&game, &rt, "Rally_3PC.exe", true).unwrap();
        assert!(!fsx::temp_beside(&rt.join("Rally_3PC.exe")).exists());
        assert_eq!(fs::read(rt.join("Rally_3PC.exe")).unwrap(), b"exe");
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }

    #[test]
    fn restore_brings_the_games_file_back_or_removes_ours() {
        let game = test_dir("rt-restore-game");
        let rt = test_dir("rt-restore-rt");
        fs::write(game.join("dinput8.dll"), b"game's own").unwrap();
        fs::write(rt.join("dinput8.dll"), b"ours").unwrap();
        fs::write(rt.join("d3d9.dll"), b"ours").unwrap();
        restore(&game, &rt, "dinput8.dll", false).unwrap();
        restore(&game, &rt, "d3d9.dll", false).unwrap();
        assert_eq!(fs::read(rt.join("dinput8.dll")).unwrap(), b"game's own");
        assert!(!rt.join("d3d9.dll").exists());
        for d in [game, rt] {
            fs::remove_dir_all(d).unwrap();
        }
    }
}
