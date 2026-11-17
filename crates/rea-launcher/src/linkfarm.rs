//! A runtime folder made of links to a read-only game folder (feature
//! `linkfarm`).
//!
//! [`crate::runtime`] hard-links or copies every file of a game the
//! launcher may patch. A hub launcher's games are dumps of tens of GB on a
//! library that may be a network share, run with the library bound
//! read-only: the runtime holds a symbolic link to each entry of the dump,
//! real folders where the game writes ([`Place::Open`]), copies of the
//! small files it rewrites ([`Place::CopyOnce`]), and the launcher's own
//! files ([`Op::Refresh`]). [`plan`] decides from listings and a
//! [`Policy`] (the game knowledge); [`apply`] carries the plan out without
//! ever writing to the game folder.
//!
//! On Windows, where a symbolic link needs administrator rights or
//! developer mode, a linked folder is a directory junction (a directory
//! symlink for a folder on a network share, which a junction cannot reach)
//! and a linked file is a copy kept in step ([`mirror_file`]).
//!
//! From the TeknoParrot hub launchers' `runtime.rs` (Initial D, Nintendo,
//! Sega, Wangan), the best of each: Sega's opened folders and recursive
//! [`dangling`], Nintendo's seeds, Wangan's refreshed components, tree
//! copies and [`mirror_action`], Sega's packed archive ([`Op::Archive`]).
//! Fixes folded in: every copy is a whole-file swap made writable
//! ([`crate::fsx::install_copy`]; a dump's read-only file copied with
//! `fs::copy` stayed read-only, and on Windows the next refresh failed); a
//! link an older launcher left where a real folder now goes is replaced,
//! never followed (writing through it wrote into the dump); a real file
//! where a link goes is kept or refused per [`Occupied`].

use std::fs;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::fsx;

/// One entry of a game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub dir: bool,
    pub size: u64,
}

/// The entries of `dir`, by name (links followed).
pub fn list(dir: &Path) -> io::Result<Vec<Item>> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        let m = fs::metadata(e.path())?;
        v.push(Item {
            name: e.file_name().to_string_lossy().into_owned(),
            dir: m.is_dir(),
            size: m.len(),
        });
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(v)
}

/// Where an entry of the game folder goes in the runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Not brought in (the launcher's or the loader's own name, NTFS
    /// leftovers).
    Skip,
    /// A link (the default).
    Link,
    /// A copy the game owns from then on: made once, never overwritten
    /// (a folder: its whole tree).
    CopyOnce,
    /// A folder the game writes into: a real folder whose entries are
    /// placed one by one (a file: [`Place::Link`]).
    Open,
    /// A fresh, empty folder (save folders a loader redirects the game to).
    Empty,
}

/// What a launcher knows about one game's folder. `rel` is the entry's
/// path in the game folder, `/`-separated.
pub trait Policy {
    fn place(&self, _rel: &str, _item: &Item) -> Place {
        Place::Link
    }

    /// A runtime folder (relative) that the files of folder `rel` seed the
    /// first time, while it holds no file (settings a game keeps on its
    /// own drives and rewrites itself). The entry is placed as well.
    fn seed(&self, _rel: &str) -> Option<&str> {
        None
    }
}

/// NTFS leftovers in a dump, never brought in.
pub fn ntfs_leftover(name: &str) -> bool {
    name == "System Volume Information" || name.starts_with('$')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// A real folder (a link an older launcher left there is replaced).
    Dir(PathBuf),
    /// A link `at` → `target` (replaced when it points elsewhere).
    Link { target: PathBuf, at: PathBuf },
    /// A copy the game owns from then on: made once (replacing a link an
    /// older launcher left), never overwritten; nothing when `from` is
    /// missing.
    CopyOnce { from: PathBuf, to: PathBuf },
    /// Folder `from` copied to `to` once, like [`Op::CopyOnce`].
    TreeOnce { from: PathBuf, to: PathBuf },
    /// The files of folder `from` copied into folder `to` while `to` holds
    /// no file yet (the first start); never again after that.
    Seed { from: PathBuf, to: PathBuf },
    /// A launcher-owned file: replaced whenever its content differs;
    /// `exec` sets the execute bits (Unix).
    Refresh {
        from: PathBuf,
        to: PathBuf,
        exec: bool,
    },
    /// A stored zip of `files`, made again only when their names, sizes or
    /// modification times change (recorded in `<to>.sources`).
    Archive { files: Vec<PathBuf>, to: PathBuf },
}

/// The runtime `rt` for game folder `src`: `Dir(rt)`, then each entry
/// placed per `policy`; the [`Place::Open`] folders are listed in turn.
/// `ls` lists a folder ([`list`], or a fake in tests); a folder it cannot
/// list is an error, not a silent gap.
pub fn plan(
    src: &Path,
    rt: &Path,
    policy: &dyn Policy,
    ls: &dyn Fn(&Path) -> io::Result<Vec<Item>>,
) -> io::Result<Vec<Op>> {
    let mut ops = vec![Op::Dir(rt.to_path_buf())];
    let mut stack = vec![String::new()];
    while let Some(rel) = stack.pop() {
        let (from_dir, to_dir) = (src.join(&rel), rt.join(&rel));
        if !rel.is_empty() {
            ops.push(Op::Dir(to_dir.clone()));
        }
        for it in ls(&from_dir)? {
            let child = if rel.is_empty() {
                it.name.clone()
            } else {
                format!("{rel}/{}", it.name)
            };
            let (from, to) = (from_dir.join(&it.name), to_dir.join(&it.name));
            let place = policy.place(&child, &it);
            if place == Place::Skip {
                continue;
            }
            if let Some(into) = policy.seed(&child) {
                ops.push(Op::Seed {
                    from: from.clone(),
                    to: rt.join(into),
                });
            }
            ops.push(match place {
                Place::Open if it.dir => {
                    stack.push(child);
                    continue;
                }
                Place::CopyOnce if it.dir => Op::TreeOnce { from, to },
                Place::CopyOnce => Op::CopyOnce { from, to },
                Place::Empty => Op::Dir(to),
                Place::Skip | Place::Link | Place::Open => Op::Link {
                    target: from,
                    at: to,
                },
            });
        }
    }
    Ok(ops)
}

/// What [`apply`] does with a real file or folder where a link goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occupied {
    /// Keep it: the game made it (Wangan's save folders).
    Keep,
    /// Stop with an error naming it: it may be the player's data, which
    /// the player moves away (Initial D, Nintendo, Sega).
    Refuse,
}

/// Carries out a plan. Idempotent; never writes to a link's target and
/// never replaces a file the game wrote.
pub fn apply(ops: &[Op], occupied: Occupied) -> io::Result<()> {
    for op in ops {
        match op {
            Op::Dir(d) => real_dir(d)?,
            Op::Link { target, at } => link(target, at, occupied)?,
            Op::CopyOnce { from, to } => {
                if from.is_file() && replaceable(to)? {
                    fsx::install_copy(from, to)?;
                }
            }
            Op::TreeOnce { from, to } => {
                if from.is_dir() && replaceable(to)? {
                    copy_tree(from, to)?;
                }
            }
            Op::Seed { from, to } => seed(from, to)?,
            Op::Refresh { from, to, exec } => {
                fsx::install_copy(from, to)?;
                #[cfg(unix)]
                if *exec {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(to, fs::Permissions::from_mode(0o755))?;
                }
                #[cfg(not(unix))]
                let _ = exec;
            }
            Op::Archive { files, to } => archive(files, to)?,
        }
    }
    Ok(())
}

/// True when nothing is at `to`, after removing a link left there.
fn replaceable(to: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(to) {
        Err(_) => Ok(true),
        Ok(m) if is_link(&m) => remove_link(to).map(|()| true),
        Ok(_) => Ok(false),
    }
}

/// A symbolic link or (Windows) a directory junction.
fn is_link(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        m.file_type().is_symlink() || m.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    m.file_type().is_symlink()
}

fn real_dir(d: &Path) -> io::Result<()> {
    if fs::symlink_metadata(d).is_ok_and(|m| is_link(&m)) {
        remove_link(d)?;
    }
    fs::create_dir_all(d)
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)? {
        let e = e?;
        let (path, dest) = (e.path(), to.join(e.file_name()));
        if path.is_dir() {
            copy_tree(&path, &dest)?;
        } else {
            fsx::install_copy(&path, &dest)?;
        }
    }
    Ok(())
}

/// Copies the files of `from` into `to` unless `to` already has a file.
fn seed(from: &Path, to: &Path) -> io::Result<()> {
    let has_file = fs::read_dir(to)
        .is_ok_and(|mut rd| rd.any(|e| e.is_ok_and(|e| e.file_type().is_ok_and(|t| t.is_file()))));
    if has_file || !from.is_dir() {
        return Ok(());
    }
    for e in fs::read_dir(from)? {
        let e = e?;
        if e.file_type()?.is_file() {
            fsx::install_copy(&e.path(), &to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// What an archive was made from: one line per file (path, size,
/// modification time in seconds), so a changed dump makes it again.
pub fn sources(files: &[PathBuf]) -> io::Result<String> {
    let mut s = String::new();
    for f in files {
        let m = fs::metadata(f)?;
        let t = m
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        s.push_str(&format!("{}\t{}\t{t}\n", f.display(), m.len()));
    }
    Ok(s)
}

fn archive(files: &[PathBuf], to: &Path) -> io::Result<()> {
    let record = to.with_extension("sources");
    let want = sources(files)?;
    if to.is_file() && fs::read_to_string(&record).ok().as_deref() == Some(&want) {
        return Ok(());
    }
    fsx::put_in_place(to, |tmp| write_stored_zip(files, tmp))?;
    fsx::write_atomic(&record, want)
}

fn real_where_a_link_goes(at: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "{} is a real file or folder where the launcher expects a link; move it away",
            at.display()
        ),
    )
}

/// A symbolic link `at` → `target`, replaced when it points elsewhere.
#[cfg(not(windows))]
fn link(target: &Path, at: &Path, occupied: Occupied) -> io::Result<()> {
    match fs::symlink_metadata(at) {
        Ok(m) if m.file_type().is_symlink() => {
            if fs::read_link(at)? != *target {
                fs::remove_file(at)?;
                std::os::unix::fs::symlink(target, at)?;
            }
            Ok(())
        }
        Ok(_) if occupied == Occupied::Keep => Ok(()),
        Ok(_) => Err(real_where_a_link_goes(at)),
        Err(_) => std::os::unix::fs::symlink(target, at),
    }
}

/// Windows: a folder is a directory junction, a file a copy kept in step.
#[cfg(windows)]
fn link(target: &Path, at: &Path, occupied: Occupied) -> io::Result<()> {
    let existing = fs::symlink_metadata(at).ok();
    let real = existing.as_ref().is_some_and(|m| !is_link(m));
    if real && (target.is_dir() || existing.as_ref().is_some_and(fs::Metadata::is_dir)) {
        return match occupied {
            Occupied::Keep => Ok(()),
            Occupied::Refuse => Err(real_where_a_link_goes(at)),
        };
    }
    if !target.is_dir() {
        return mirror_file(target, at);
    }
    if existing.is_some() {
        if junction::get_target(at).ok().as_deref() == Some(target) {
            return Ok(());
        }
        remove_link(at)?;
    }
    junction::create(target, at).or_else(|e| {
        std::os::windows::fs::symlink_dir(target, at).map_err(|_| {
            io::Error::new(
                e.kind(),
                format!(
                    "cannot link {} into the runtime folder ({e}); a game folder on a \
                     network share needs Windows developer mode",
                    target.display()
                ),
            )
        })
    })
}

/// What [`mirror_file`] does with the runtime's copy of a game file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    /// Missing, or the game folder's file changed: copy it.
    Copy,
    /// A link (or a folder) where the copy goes: replace it.
    Replace,
    /// Same size and modification time as the source.
    Keep,
}

/// Decides from the two files' metadata (`at` is `None` when missing;
/// read without following links).
pub fn mirror_action(src: &fs::Metadata, at: Option<&fs::Metadata>) -> Mirror {
    match at {
        None => Mirror::Copy,
        Some(m) if is_link(m) || m.is_dir() => Mirror::Replace,
        Some(m) if m.len() == src.len() && m.modified().ok() == src.modified().ok() => Mirror::Keep,
        Some(_) => Mirror::Copy,
    }
}

/// Keeps `at` a writable copy of `target` (Windows's stand-in for a file
/// link): copied when missing or when size or modification time differ;
/// the copy gets the source's modification time. A real folder in the
/// copy's place is the game's and is kept.
pub fn mirror_file(target: &Path, at: &Path) -> io::Result<()> {
    let src = fs::metadata(target)?;
    let cur = fs::symlink_metadata(at).ok();
    match mirror_action(&src, cur.as_ref()) {
        Mirror::Keep => return Ok(()),
        Mirror::Replace if cur.as_ref().is_some_and(|m| !is_link(m)) => return Ok(()),
        Mirror::Replace => remove_link(at)?,
        Mirror::Copy => {}
    }
    fsx::put_in_place(at, |tmp| {
        fs::copy(target, tmp)?;
        fsx::make_writable(tmp)?;
        fs::File::options()
            .write(true)
            .open(tmp)?
            .set_modified(src.modified()?)
    })
}

/// Removes a link the launcher made (on Windows a junction is a folder),
/// never what it points to.
pub fn remove_link(at: &Path) -> io::Result<()> {
    fs::remove_file(at).or_else(|e| fs::remove_dir(at).map_err(|_| e))
}

/// Links in a runtime folder (and the real folders inside it) whose target
/// is gone (a dump moved): to remove so a new plan can link the new place.
pub fn dangling(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut v = Vec::new();
    for e in fs::read_dir(dir)? {
        let p = e?.path();
        let m = fs::symlink_metadata(&p)?;
        if is_link(&m) {
            if fs::metadata(&p).is_err() {
                v.push(p);
            }
        } else if m.is_dir() {
            v.extend(dangling(&p)?);
        }
    }
    Ok(v)
}

/// 1980-01-01 00:00 in DOS time: the archive doesn't depend on when it
/// was made.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// Writes `files` (by their file names) into a new zip at `to`, stored,
/// not compressed: Flycast loads a NAOMI cartridge only from an archive
/// named after its MAME set, and reads stored entries without inflating.
pub fn write_stored_zip(files: &[PathBuf], to: &Path) -> io::Result<()> {
    let mut out = BufWriter::new(fs::File::create(to)?);
    let mut central = Vec::new();
    let mut buf = vec![0u8; 1 << 20];
    for f in files {
        let name = f
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| io::Error::other(format!("{}: not a plain name", f.display())))?;
        let header_at = out.stream_position()?;
        // the header, then the data; its CRC and sizes are filled in after
        local_header(&mut out, name, 0, 0)?;
        let (mut crc, mut size) = (Crc32::default(), 0u64);
        let mut src = fs::File::open(f)?;
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            crc.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            size += n as u64;
        }
        let (crc, size) = (crc.finish(), u32_of(size)?);
        let end = out.stream_position()?;
        out.seek(SeekFrom::Start(header_at))?;
        local_header(&mut out, name, crc, size)?;
        out.seek(SeekFrom::Start(end))?;
        central.push((name.to_string(), crc, size, u32_of(header_at)?));
    }
    let dir_at = u32_of(out.stream_position()?)?;
    for (name, crc, size, offset) in &central {
        out.write_all(&0x0201_4b50u32.to_le_bytes())?;
        out.write_all(&20u16.to_le_bytes())?; // made by
        out.write_all(&10u16.to_le_bytes())?; // needed to extract
        out.write_all(&0u16.to_le_bytes())?; // flags
        out.write_all(&0u16.to_le_bytes())?; // stored
        out.write_all(&DOS_TIME.to_le_bytes())?;
        out.write_all(&DOS_DATE.to_le_bytes())?;
        out.write_all(&crc.to_le_bytes())?;
        out.write_all(&size.to_le_bytes())?;
        out.write_all(&size.to_le_bytes())?;
        out.write_all(&(name.len() as u16).to_le_bytes())?;
        out.write_all(&[0u8; 12])?; // extra, comment, disk, attributes
        out.write_all(&offset.to_le_bytes())?;
        out.write_all(name.as_bytes())?;
    }
    let dir_size = u32_of(out.stream_position()?)? - dir_at;
    let count = u16::try_from(central.len()).map_err(|_| io::Error::other("too many files"))?;
    out.write_all(&0x0605_4b50u32.to_le_bytes())?;
    out.write_all(&[0u8; 4])?; // disk numbers
    out.write_all(&count.to_le_bytes())?;
    out.write_all(&count.to_le_bytes())?;
    out.write_all(&dir_size.to_le_bytes())?;
    out.write_all(&dir_at.to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?; // comment
    out.into_inner().map_err(|e| e.into_error())?.sync_all()
}

fn local_header(out: &mut impl Write, name: &str, crc: u32, size: u32) -> io::Result<()> {
    out.write_all(&0x0403_4b50u32.to_le_bytes())?;
    out.write_all(&10u16.to_le_bytes())?; // needed to extract
    out.write_all(&0u16.to_le_bytes())?; // flags
    out.write_all(&0u16.to_le_bytes())?; // stored
    out.write_all(&DOS_TIME.to_le_bytes())?;
    out.write_all(&DOS_DATE.to_le_bytes())?;
    out.write_all(&crc.to_le_bytes())?;
    out.write_all(&size.to_le_bytes())?;
    out.write_all(&size.to_le_bytes())?;
    out.write_all(&(name.len() as u16).to_le_bytes())?;
    out.write_all(&0u16.to_le_bytes())?; // extra
    out.write_all(name.as_bytes())
}

fn u32_of(n: u64) -> io::Result<u32> {
    u32::try_from(n).map_err(|_| io::Error::other("a file over 4 GB doesn't fit a plain zip"))
}

/// The zip CRC-32 (IEEE 802.3, reflected).
struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Crc32(!0)
    }
}

const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
};

impl Crc32 {
    fn update(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = CRC_TABLE[((self.0 ^ u32::from(b)) & 0xff) as usize] ^ (self.0 >> 8);
        }
    }

    fn finish(&self) -> u32 {
        !self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;
    use std::collections::HashMap;

    fn item(name: &str, dir: bool, size: u64) -> Item {
        Item {
            name: name.into(),
            dir,
            size,
        }
    }

    /// A fake dump: folder → entries.
    fn fake(tree: &[(&str, Vec<Item>)]) -> impl Fn(&Path) -> io::Result<Vec<Item>> + use<> {
        let map: HashMap<PathBuf, Vec<Item>> = tree
            .iter()
            .map(|(d, v)| (PathBuf::from(d), v.clone()))
            .collect();
        move |p: &Path| {
            map.get(p)
                .cloned()
                .ok_or_else(|| io::Error::other(format!("not listed: {}", p.display())))
        }
    }

    fn paths(ops: &[Op], pick: impl Fn(&Op) -> Option<&PathBuf>) -> Vec<String> {
        ops.iter()
            .filter_map(pick)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    fn linked(ops: &[Op]) -> Vec<String> {
        paths(ops, |o| match o {
            Op::Link { at, .. } => Some(at),
            _ => None,
        })
    }

    fn copied(ops: &[Op]) -> Vec<String> {
        paths(ops, |o| match o {
            Op::CopyOnce { to, .. } | Op::TreeOnce { to, .. } => Some(to),
            _ => None,
        })
    }

    fn is_link(p: &Path) -> bool {
        fs::symlink_metadata(p).is_ok_and(|m| super::is_link(&m))
    }

    /// Sega's rules: small files are copied (code never), the loader's
    /// marker and drives skipped at the top, the folders the game writes
    /// into opened, Wangan's save folder made empty and its tree copied,
    /// Nintendo's test-mode settings seeding a drive folder.
    struct Rules;

    impl Policy for Rules {
        fn place(&self, rel: &str, it: &Item) -> Place {
            let top = !rel.contains('/');
            let code = [".exe", ".dll"].iter().any(|x| it.name.ends_with(x));
            match rel {
                _ if top
                    && (ntfs_leftover(rel)
                        || rel.ends_with(".windowsloader")
                        || rel == "AppData") =>
                {
                    Place::Skip
                }
                "Rally" | "ShellData" | "Rally/rankingdata" => Place::Open,
                "TP" => Place::Empty,
                "AMCUS" => Place::CopyOnce,
                _ if !it.dir && it.size < 4 << 20 && !code => Place::CopyOnce,
                _ => Place::Link,
            }
        }

        fn seed(&self, rel: &str) -> Option<&str> {
            (rel == "FTestMode").then_some("F/TestMode")
        }
    }

    #[test]
    fn the_policy_places_each_entry_and_opened_folders_are_listed_in_turn() {
        let ls = fake(&[
            (
                "/lib/sr3",
                vec![
                    item("$RECYCLE.BIN", true, 0),
                    item("AMCUS", true, 0),
                    item("AppData", true, 0),
                    item("FTestMode", true, 0),
                    item("Rally", true, 0),
                    item("ShellData", true, 0),
                    item("TP", true, 0),
                    item("old.windowsloader", false, 10),
                ],
            ),
            (
                "/lib/sr3/Rally",
                vec![
                    item("Main_release", true, 0),
                    item("Rally.exe", false, 9 << 20),
                    item("config.ini", false, 900),
                    item("fmodex.dll", false, 400_000),
                    item("rankingdata", true, 0),
                ],
            ),
            (
                "/lib/sr3/Rally/rankingdata",
                vec![item("time.bin", false, 5000)],
            ),
            ("/lib/sr3/ShellData", vec![item("Game.ini", false, 300)]),
        ]);
        let ops = plan(Path::new("/lib/sr3"), Path::new("/rt/sr3"), &Rules, &ls).unwrap();
        assert_eq!(ops[0], Op::Dir("/rt/sr3".into()));
        for d in [
            "/rt/sr3/Rally",
            "/rt/sr3/Rally/rankingdata",
            "/rt/sr3/ShellData",
            "/rt/sr3/TP",
        ] {
            assert!(ops.contains(&Op::Dir(d.into())), "{d}");
        }
        assert_eq!(
            linked(&ops),
            [
                "/rt/sr3/FTestMode",
                "/rt/sr3/Rally/Main_release",
                "/rt/sr3/Rally/Rally.exe",
                "/rt/sr3/Rally/fmodex.dll",
            ]
        );
        assert_eq!(
            copied(&ops),
            [
                "/rt/sr3/AMCUS",
                "/rt/sr3/ShellData/Game.ini",
                "/rt/sr3/Rally/config.ini",
                "/rt/sr3/Rally/rankingdata/time.bin",
            ]
        );
        assert!(ops.contains(&Op::TreeOnce {
            from: "/lib/sr3/AMCUS".into(),
            to: "/rt/sr3/AMCUS".into()
        }));
        assert!(ops.contains(&Op::Seed {
            from: "/lib/sr3/FTestMode".into(),
            to: "/rt/sr3/F/TestMode".into()
        }));
        let all = format!("{ops:?}");
        for gone in ["RECYCLE", "windowsloader", "AppData"] {
            assert!(!all.contains(gone), "{gone} is never taken from the dump");
        }
    }

    #[test]
    fn a_folder_that_cannot_be_read_is_an_error_not_a_silent_gap() {
        let ls = fake(&[("/lib/src", vec![item("Rally", true, 0)])]);
        assert!(plan(Path::new("/lib/src"), Path::new("/rt"), &Rules, &ls).is_err());
        // the default policy links everything, one level
        let ls = fake(&[(
            "/lib/src",
            vec![item("Rally", true, 0), item("a.ini", false, 1)],
        )]);
        struct LinkAll;
        impl Policy for LinkAll {}
        let ops = plan(Path::new("/lib/src"), Path::new("/rt"), &LinkAll, &ls).unwrap();
        assert_eq!(linked(&ops), ["/rt/Rally", "/rt/a.ini"]);
    }

    /// A dump on disk: `a.ini` (small), `game.exe`, `data/`, `Rally/`
    /// (opened, with `config.ini`), `FTestMode/one.bin`, `AMCUS/sub/x`.
    fn dump(t: &Path) -> PathBuf {
        let src = t.join("dump");
        for d in ["data", "Rally", "FTestMode", "AMCUS/sub"] {
            fs::create_dir_all(src.join(d)).unwrap();
        }
        for (f, bytes) in [
            ("a.ini", &b"old"[..]),
            ("game.exe", b"exe"),
            ("data/big.bin", b"data"),
            ("Rally/config.ini", b"cfg"),
            ("FTestMode/one.bin", b"camera off"),
            ("AMCUS/sub/x", b"x"),
        ] {
            fs::write(src.join(f), bytes).unwrap();
        }
        src
    }

    #[test]
    fn small_files_are_copied_once_and_then_belong_to_the_game() {
        let t = test_dir("linkfarm-copy-once");
        let (src, rt) = (dump(&t), t.join("rt"));
        let ops = plan(&src, &rt, &Rules, &list).unwrap();
        // an older launcher linked the ini: replaced by a copy
        apply(
            &[
                Op::Dir(rt.clone()),
                Op::Link {
                    target: src.join("a.ini"),
                    at: rt.join("a.ini"),
                },
            ],
            Occupied::Refuse,
        )
        .unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        let ini = rt.join("a.ini");
        assert!(!is_link(&ini));
        assert!(is_link(&rt.join("data")) && !is_link(&rt.join("Rally")));
        assert_eq!(fs::read(rt.join("game.exe")).unwrap(), b"exe");
        assert_eq!(fs::read(rt.join("Rally/config.ini")).unwrap(), b"cfg");
        assert_eq!(fs::read(rt.join("AMCUS/sub/x")).unwrap(), b"x");
        fs::write(&ini, b"the game's").unwrap();
        fs::write(rt.join("AMCUS/sub/x"), b"the game's").unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        assert_eq!(fs::read(&ini).unwrap(), b"the game's");
        assert_eq!(fs::read(rt.join("AMCUS/sub/x")).unwrap(), b"the game's");
        // the dump is untouched
        assert_eq!(fs::read(src.join("a.ini")).unwrap(), b"old");
        assert_eq!(fs::read(src.join("AMCUS/sub/x")).unwrap(), b"x");
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_copy_of_a_read_only_file_is_writable() {
        let t = test_dir("linkfarm-read-only");
        let src = t.join("ro.ini");
        fs::write(&src, b"v1").unwrap();
        let mut perm = fs::metadata(&src).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&src, perm).unwrap();
        let ops = [
            Op::CopyOnce {
                from: src.clone(),
                to: t.join("once.ini"),
            },
            Op::Refresh {
                from: src.clone(),
                to: t.join("ours.ini"),
                exec: false,
            },
        ];
        apply(&ops, Occupied::Refuse).unwrap();
        mirror_file(&src, &t.join("mirror.ini")).unwrap();
        for f in ["once.ini", "ours.ini", "mirror.ini"] {
            let p = t.join(f);
            assert!(!fs::metadata(&p).unwrap().permissions().readonly(), "{f}");
            fs::write(&p, b"the game's").unwrap();
        }
        // and they can be replaced again
        apply(&ops, Occupied::Refuse).unwrap();
        mirror_file(&src, &t.join("mirror.ini")).unwrap();
        assert_eq!(fs::read(t.join("ours.ini")).unwrap(), b"v1");
        assert_eq!(fs::read(t.join("mirror.ini")).unwrap(), b"v1");
        assert_eq!(fs::read(t.join("once.ini")).unwrap(), b"the game's");
        let mut perm = fs::metadata(&src).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        fs::set_permissions(&src, perm).unwrap();
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn apply_links_relinks_and_refuses_or_keeps_real_files() {
        let t = test_dir("linkfarm-relink");
        let (a, b, rt) = (t.join("a"), t.join("b"), t.join("rt"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("in-a"), b"a").unwrap();
        let at = rt.join("data");
        let to = |target: &Path, at: &Path| Op::Link {
            target: target.to_path_buf(),
            at: at.to_path_buf(),
        };
        apply(&[Op::Dir(rt.clone()), to(&a, &at)], Occupied::Refuse).unwrap();
        apply(&[to(&a, &at)], Occupied::Refuse).unwrap();
        apply(&[to(&b, &at)], Occupied::Refuse).unwrap();
        // the link follows the new target (a symlink, or a junction on
        // Windows), the folder it pointed to untouched
        fs::write(b.join("in-b"), b"b").unwrap();
        assert_eq!(fs::read(at.join("in-b")).unwrap(), b"b");
        assert_eq!(fs::read(a.join("in-a")).unwrap(), b"a");
        // a real folder or file where a link goes: refused, or kept
        fs::create_dir_all(rt.join("TP")).unwrap();
        fs::write(rt.join("TP/save"), b"s").unwrap();
        fs::write(rt.join("save.bin"), b"mine").unwrap();
        for at in [rt.join("TP"), rt.join("save.bin")] {
            let err = apply(&[to(&a, &at)], Occupied::Refuse).unwrap_err();
            assert!(err.to_string().contains("move it away"), "{err}");
            apply(&[to(&a, &at)], Occupied::Keep).unwrap();
        }
        assert_eq!(fs::read(rt.join("TP/save")).unwrap(), b"s");
        assert_eq!(fs::read(rt.join("save.bin")).unwrap(), b"mine");
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_link_where_a_real_folder_now_goes_is_replaced_not_followed() {
        let t = test_dir("linkfarm-open-link");
        let (src, rt) = (dump(&t), t.join("rt"));
        // an older run linked the whole folder
        apply(
            &[
                Op::Dir(rt.clone()),
                Op::Link {
                    target: src.join("Rally"),
                    at: rt.join("Rally"),
                },
            ],
            Occupied::Refuse,
        )
        .unwrap();
        assert!(is_link(&rt.join("Rally")));
        apply(&plan(&src, &rt, &Rules, &list).unwrap(), Occupied::Refuse).unwrap();
        assert!(!is_link(&rt.join("Rally")));
        // the game's writes land in the runtime, not in the dump
        fs::write(rt.join("Rally/save.bin"), b"s").unwrap();
        assert!(!src.join("Rally/save.bin").exists());
        assert_eq!(fs::read_dir(src.join("Rally")).unwrap().count(), 1);
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn seeds_fill_an_empty_folder_once() {
        let t = test_dir("linkfarm-seed");
        let (src, rt) = (dump(&t), t.join("rt"));
        let ops = plan(&src, &rt, &Rules, &list).unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        let f = rt.join("F/TestMode/one.bin");
        assert_eq!(fs::read(&f).unwrap(), b"camera off");
        // the game rewrites its settings: they are its own from then on
        fs::remove_file(&f).unwrap();
        fs::write(rt.join("F/TestMode/two.bin"), b"the player's").unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        assert!(!f.exists());
        assert_eq!(
            fs::read(src.join("FTestMode/one.bin")).unwrap(),
            b"camera off"
        );
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn launcher_files_are_refreshed_and_missing_sources_copy_nothing() {
        let t = test_dir("linkfarm-refresh");
        let (ours, rt) = (t.join("loader"), t.join("rt"));
        fs::write(&ours, b"v1").unwrap();
        let ops = [
            Op::Dir(rt.clone()),
            Op::Refresh {
                from: ours.clone(),
                to: rt.join("loader"),
                exec: true,
            },
            Op::CopyOnce {
                from: t.join("gone.wav"),
                to: rt.join("gone.wav"),
            },
            Op::TreeOnce {
                from: t.join("gone"),
                to: rt.join("gone"),
            },
        ];
        apply(&ops, Occupied::Refuse).unwrap();
        fs::write(&ours, b"v2").unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        assert_eq!(fs::read(rt.join("loader")).unwrap(), b"v2");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(rt.join("loader"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        assert!(!rt.join("gone.wav").exists() && !rt.join("gone").exists());
        let missing = Op::Refresh {
            from: t.join("gone"),
            to: rt.join("x"),
            exec: false,
        };
        assert!(
            apply(&[missing], Occupied::Refuse).is_err(),
            "our own file must exist"
        );
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn mirrored_files_are_copied_once_and_follow_the_source() {
        let t = test_dir("linkfarm-mirror");
        let src = t.join("game.exe");
        let at = t.join("rt.exe");
        fs::write(&src, b"v1").unwrap();
        assert_eq!(
            mirror_action(&fs::metadata(&src).unwrap(), None),
            Mirror::Copy
        );
        mirror_file(&src, &at).unwrap();
        assert_eq!(fs::read(&at).unwrap(), b"v1");
        let (s, a) = (fs::metadata(&src).unwrap(), fs::metadata(&at).unwrap());
        assert_eq!(a.modified().unwrap(), s.modified().unwrap());
        assert_eq!(mirror_action(&s, Some(&a)), Mirror::Keep);
        // the game folder's file changes: the copy follows
        fs::write(&src, b"v2 longer").unwrap();
        mirror_file(&src, &at).unwrap();
        assert_eq!(fs::read(&at).unwrap(), b"v2 longer");
        assert_eq!(fs::read_dir(&t).unwrap().count(), 2, "no temp file left");
        // a folder where the file goes is the game's and is kept
        let dir = t.join("dir.exe");
        fs::create_dir(&dir).unwrap();
        let d = fs::symlink_metadata(&dir).unwrap();
        assert_eq!(
            mirror_action(&fs::metadata(&src).unwrap(), Some(&d)),
            Mirror::Replace
        );
        mirror_file(&src, &dir).unwrap();
        assert!(dir.is_dir());
        assert_eq!(fs::read(&src).unwrap(), b"v2 longer");
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn dangling_links_are_found_inside_real_folders_too() {
        let t = test_dir("linkfarm-dangling");
        let rt = t.join("rt");
        let (gone, kept) = (t.join("gone"), t.join("kept"));
        fs::create_dir_all(&gone).unwrap();
        fs::create_dir_all(&kept).unwrap();
        let link = |target: &Path, at: PathBuf| Op::Link {
            target: target.to_path_buf(),
            at,
        };
        let ops = [
            Op::Dir(rt.join("Rally")),
            link(&gone, rt.join("Rally/x")),
            link(&kept, rt.join("ok")),
        ];
        apply(&ops, Occupied::Refuse).unwrap();
        fs::remove_dir(&gone).unwrap();
        let found = dangling(&rt).unwrap();
        assert_eq!(found, [rt.join("Rally/x")]);
        remove_link(&found[0]).unwrap();
        assert!(dangling(&rt).unwrap().is_empty());
        assert!(kept.is_dir());
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_cartridge_is_packed_into_one_archive_made_again_only_when_it_changes() {
        let t = test_dir("linkfarm-archive");
        let (chip, key) = (t.join("epr-23399.ic22"), t.join("f355twn2-key.bin"));
        fs::write(&chip, b"program").unwrap();
        fs::write(&key, b"key!").unwrap();
        let zip = t.join("rt/f355twn2.zip");
        let ops = [
            Op::Dir(t.join("rt")),
            Op::Archive {
                files: vec![chip.clone(), key.clone()],
                to: zip.clone(),
            },
        ];
        apply(&ops, Occupied::Refuse).unwrap();
        let first = fs::metadata(&zip).unwrap().modified().unwrap();
        assert_eq!(
            fs::read_to_string(zip.with_extension("sources")).unwrap(),
            sources(&[chip.clone(), key.clone()]).unwrap()
        );
        // unchanged: the archive stays as it is
        std::thread::sleep(std::time::Duration::from_millis(20));
        apply(&ops, Occupied::Refuse).unwrap();
        assert_eq!(fs::metadata(&zip).unwrap().modified().unwrap(), first);
        // a chip replaced by one of another size: made again
        fs::write(&chip, b"program, v2").unwrap();
        apply(&ops, Occupied::Refuse).unwrap();
        let bytes = fs::read(&zip).unwrap();
        assert!(bytes.windows(11).any(|w| w == b"program, v2"));
        assert_eq!(
            fs::read_dir(t.join("rt")).unwrap().count(),
            2,
            "no temp file left"
        );
        fs::remove_dir_all(t).unwrap();
    }

    #[test]
    fn a_stored_zip_is_well_formed() {
        let t = test_dir("linkfarm-zip");
        let (a, b) = (t.join("epr-1.ic22"), t.join("key.bin"));
        fs::write(&a, vec![7u8; 300_000]).unwrap();
        fs::write(&b, b"123456789").unwrap();
        let zip = t.join("set.zip");
        write_stored_zip(&[a, b], &zip).unwrap();
        let bytes = fs::read(&zip).unwrap();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 2);
        // key.bin's CRC, the standard check value of "123456789"
        let local = 30 + "epr-1.ic22".len() + 300_000;
        assert_eq!(&bytes[local..local + 4], b"PK\x03\x04");
        assert_eq!(bytes[local + 14..local + 18], 0xCBF4_3926u32.to_le_bytes());
        // the real tool, when it is installed (STANDARDS "Test behaviour")
        match std::process::Command::new("unzip")
            .arg("-tq")
            .arg(&zip)
            .output()
        {
            Ok(o) => assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout)),
            Err(_) => eprintln!("unzip not installed: skipped the round trip"),
        }
        write_stored_zip(&[], &zip).unwrap();
        assert_eq!(fs::metadata(&zip).unwrap().len(), 22);
        fs::remove_dir_all(t).unwrap();
    }

    /// Windows: a linked file is a copy, refreshed when the game's file
    /// changes; a linked folder is a junction.
    #[test]
    #[cfg(windows)]
    fn on_windows_a_linked_file_is_a_copy_kept_in_step() {
        let t = test_dir("linkfarm-windows");
        let (src, at) = (t.join("game.exe"), t.join("rt-game.exe"));
        fs::write(&src, b"v1").unwrap();
        let op = Op::Link {
            target: src.clone(),
            at: at.clone(),
        };
        apply(std::slice::from_ref(&op), Occupied::Refuse).unwrap();
        assert!(!is_link(&at));
        assert_eq!(fs::read(&at).unwrap(), b"v1");
        fs::write(&src, b"v2, longer").unwrap();
        apply(std::slice::from_ref(&op), Occupied::Refuse).unwrap();
        assert_eq!(fs::read(&at).unwrap(), b"v2, longer");
        let dir = Op::Link {
            target: t.clone(),
            at: t.join("junction"),
        };
        apply(std::slice::from_ref(&dir), Occupied::Refuse).unwrap();
        assert!(is_link(&t.join("junction")));
        remove_link(&t.join("junction")).unwrap();
        fs::remove_dir_all(t).unwrap();
    }
}
