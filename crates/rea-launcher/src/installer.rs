//! A game's installer instead of an installed game: identify it, then
//! unpack it once into a folder of the launcher's own.
//!
//! Installers are never run. They open dialogs nobody answers, some hang
//! under Wine (GOG's Colin McRae Rally 2005), and they run code we can't
//! check. The launcher reads Inno Setup installers with the bundled
//! innoextract (`components/innoextract.exe`, zlib licence; under Linux it
//! runs in the launcher's Wine like the launcher itself) into a work folder
//! beside the destination, checks the game there, and moves it into place.
//! The installer files are only read. CMR2005's `source.rs`, generalised.
//!
//! The steps, which `rea_launcher_win32::innoextract::unpack` drives with
//! a progress window:
//!
//! 1. [`detect`] the installer in the folder the player chose (after the
//!    launcher found no installed game there).
//! 2. [`begin`]: [`Begin::Done`] when the destination already holds this
//!    installer's game, else an empty work folder and innoextract's
//!    arguments ([`innoextract_args`]).
//! 3. Run innoextract; [`unpacked_so_far`] measures the progress.
//! 4. [`finish`]: resolve file collisions ([`InstallerBuild::prefer`]),
//!    remove the files the launcher bundles itself ([`InstallerBuild::drop`]),
//!    run the launcher's game check, then move the game into place. Or
//!    [`abandon`] after a failure or Cancel.
//!
//! Only a folder holding the [`MARKER`] file is ever replaced or removed:
//! a destination the player made, or a stray folder with the work
//! folder's name, is refused with a message.
//!
//! ```no_run
//! use rea_launcher::identify::Known;
//! use rea_launcher::installer::{self, InstallerBuild};
//! # use std::path::Path;
//! static INSTALLERS: &[Known<InstallerBuild>] = &[Known::new(
//!     "0000000000000000000000000000000000000000000000000000000000000000",
//!     InstallerBuild {
//!         name: "the Magipack repack (v4)",
//!         setup: "Colin McRae Rally 3_Repack_v4_Setup.exe",
//!         parts: &[("Colin McRae Rally 3_Repack_v4_Setup-1.bin", 770_000_000)],
//!         unpacked: 1_700_000_000,
//!         root: "app",
//!         prefer: &[],
//!         drop: &["dinput8.dll", "SilentPatchCMR3.asi"],
//!     },
//! )];
//! # let dir = Path::new("game");
//! match installer::detect(dir, INSTALLERS, "https://example.org/issues/new")? {
//!     Some(found) => println!("unpack {}", found.build.name),
//!     None => println!("no game and no installer here"),
//! }
//! # Ok::<(), String>(())
//! ```

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::identify::{self, Known};
use crate::paths;

/// One installer a launcher knows: identified by its setup program's
/// SHA-256 (the key of its [`Known`] entry) and the exact size of each data
/// part (hashing gigabytes on every start would be slow; innoextract checks
/// every file's checksum while unpacking).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallerBuild {
    /// For the player and the log: "GOG offline installer (v1.1)".
    pub name: &'static str,
    /// The setup program's file name (found ignoring ASCII case).
    pub setup: &'static str,
    /// Each data part's file name, beside the setup program, and size.
    pub parts: &'static [(&'static str, u64)],
    /// The bytes of the unpacked game, for the progress bar and the free
    /// space check ([`space_needed`]).
    pub unpacked: u64,
    /// The folder in innoextract's output that is the game: `app` (Inno
    /// Setup's `{app}`) for every installer so far.
    pub root: &'static str,
    /// Components whose copy wins a file collision. Two components that
    /// install the same file (CMR4's repack: the default `base` and an
    /// optional livery, `base\p206`, both write `p206.big`) come out of
    /// innoextract's `--collisions=rename` as `p206.big` and
    /// `p206.big#base`. After the unpack, every `X#<component>` with the
    /// component in this list replaces `X`; every other such leftover is
    /// removed ([`Finished::preferred`], [`Finished::removed`]). Compared
    /// ignoring ASCII case.
    pub prefer: &'static [&'static str],
    /// Files the installer adds that the launcher bundles itself, `/`
    /// separated below [`InstallerBuild::root`], found ignoring ASCII case:
    /// removed before the game check, so a repack's older wrapper or fix
    /// doesn't pass for the player's own and win over the launcher's
    /// (CMR3's repack: `dinput8.dll`, `SilentPatchCMR3.asi`). A listed file
    /// that isn't there is fine.
    pub drop: &'static [&'static str],
}

/// An installer [`detect`] found and identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installer {
    /// The setup program, spelled as on disk.
    pub setup: PathBuf,
    pub sha256: [u8; 32],
    pub build: &'static InstallerBuild,
}

/// The file in an unpacked folder (and in a work folder) that records the
/// setup program's SHA-256. Only a folder holding it is the launcher's to
/// replace or remove.
pub const MARKER: &str = ".rea-unpacked";

/// The text innoextract looks for, and every Inno Setup installer holds
/// near its start ("Inno Setup Setup Data (5.4.2)").
pub const INNO_SIGNATURE: &[u8] = b"Inno Setup Setup Data (";

/// How far into an `.exe` [`is_inno_setup`] looks: the signature sits
/// within the first 2 MB in every installer seen (GOG's at 1.4 MB).
pub const INNO_SCAN_BYTES: u64 = 4 << 20;

/// What [`detect`] says about `dir`: `Ok(None)` when it holds no installer
/// (the launcher then gives its usual "no game here" message), the known
/// installer with every data part at its exact size, or an error for the
/// player: a known setup name with an unknown digest, or any other Inno
/// Setup installer (both left alone, with the SHA-256 and `report_url`),
/// or a missing or short data part.
///
/// Call it after looking for an installed game, which wins when both are
/// there.
pub fn detect(
    dir: &Path,
    builds: &'static [Known<InstallerBuild>],
    report_url: &str,
) -> Result<Option<Installer>, String> {
    let mut names: Vec<&str> = builds.iter().map(|known| known.value.setup).collect();
    names.sort_unstable_by_key(|name| name.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    for name in names {
        let Some(setup) = paths::find_ignoring_case(dir, name).filter(|p| p.is_file()) else {
            continue;
        };
        let sha256 = identify::sha256_file(&setup)
            .map_err(|error| format!("Cannot read {}: {error}", setup.display()))?;
        let Ok(build) = identify::identify_digest(sha256, builds) else {
            return Err(unknown(&setup, &sha256, report_url));
        };
        check_parts(dir, build)?;
        return Ok(Some(Installer {
            setup,
            sha256,
            build,
        }));
    }
    for exe in exes(dir) {
        if is_inno_setup(&exe).unwrap_or(false) {
            let sha256 = identify::sha256_file(&exe)
                .map_err(|error| format!("Cannot read {}: {error}", exe.display()))?;
            return Err(unknown(&exe, &sha256, report_url));
        }
    }
    Ok(None)
}

fn unknown(setup: &Path, sha256: &[u8; 32], report_url: &str) -> String {
    let name = setup.file_name().unwrap_or_default().to_string_lossy();
    format!(
        "{name} is an installer this launcher doesn't know, so it was left alone.\n\n\
         SHA-256: {}\n\n\
         Please report it, with that SHA-256 and where your copy came from:\n{report_url}",
        identify::hex(sha256)
    )
}

fn check_parts(dir: &Path, build: &InstallerBuild) -> Result<(), String> {
    for (name, size) in build.parts {
        let actual = paths::find_ignoring_case(dir, name)
            .filter(|p| p.is_file())
            .and_then(|p| fs::metadata(p).ok())
            .map(|m| m.len());
        if actual != Some(*size) {
            let what = match actual {
                None => "is missing".to_owned(),
                Some(actual) => format!("has {actual} bytes instead of {size}"),
            };
            return Err(format!(
                "{name} {what} in {}. It belongs with {}: copy or download it again.",
                dir.display(),
                build.setup
            ));
        }
    }
    Ok(())
}

/// The `.exe` files directly in `dir`, by name.
fn exes(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                && path.is_file()
        })
        .collect();
    found.sort();
    found
}

/// Whether `file` is an Inno Setup installer: [`INNO_SIGNATURE`] within
/// its first [`INNO_SCAN_BYTES`].
pub fn is_inno_setup(file: &Path) -> io::Result<bool> {
    let mut head = Vec::new();
    fs::File::open(file)?
        .take(INNO_SCAN_BYTES)
        .read_to_end(&mut head)?;
    Ok(head
        .windows(INNO_SIGNATURE.len())
        .any(|window| window == INNO_SIGNATURE))
}

/// The free space an unpack of `build` needs: the game plus a tenth (and
/// at least 64 MiB) for the file system and innoextract's own use.
pub fn space_needed(build: &InstallerBuild) -> u64 {
    build.unpacked + (build.unpacked / 10).max(64 << 20)
}

/// An error for the player when `free` bytes (as the system reports them,
/// `None` when it can't say) in `dir` are fewer than [`space_needed`].
pub fn check_space(build: &InstallerBuild, free: Option<u64>, dir: &Path) -> Result<(), String> {
    let need = space_needed(build);
    match free {
        Some(free) if free < need => Err(format!(
            "Unpacking the game needs about {:.1} GB free in {}; there are {:.1} GB.",
            need as f64 / 1e9,
            dir.display(),
            free as f64 / 1e9
        )),
        _ => Ok(()),
    }
}

/// Whether `dest` holds the game unpacked from `installer` (its
/// [`MARKER`] names the setup program's digest).
pub fn is_unpacked(dest: &Path, installer: &Installer) -> bool {
    marker(dest).is_some_and(|text| text == identify::hex(&installer.sha256))
}

fn marker(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(MARKER))
        .ok()
        .map(|text| text.trim().to_owned())
}

/// The folder innoextract writes into: `dest`'s name plus `.unpacking`,
/// beside it (the same disk, so the game moves into place by a rename).
pub fn work_dir(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".unpacking");
    dest.with_file_name(name)
}

/// innoextract's arguments: extract only the game ([`InstallerBuild::root`])
/// into `out`, test every file's checksum, no temporary files, errors only,
/// no progress bar, and colliding files kept under `name#component`
/// (resolved by [`finish`]).
pub fn innoextract_args(installer: &Installer, out: &Path) -> Vec<OsString> {
    [
        "--extract",
        "--test",
        "--exclude-temp",
        "--silent",
        "--progress=0",
        "--collisions=rename",
        "--include",
        installer.build.root,
        "--output-dir",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([
        out.as_os_str().to_owned(),
        installer.setup.as_os_str().to_owned(),
    ])
    .collect()
}

/// What an unpack needs to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Begin {
    /// `dest` already holds this installer's game.
    Done,
    /// Run innoextract with `args`; it writes into `work`.
    Run { args: Vec<OsString>, work: PathBuf },
}

/// Checks `dest` and makes an empty work folder beside it (marked as the
/// launcher's). A `dest`, or a leftover work folder, without the
/// [`MARKER`] is an error, never removed.
pub fn begin(installer: &Installer, dest: &Path) -> Result<Begin, String> {
    if is_unpacked(dest, installer) {
        return Ok(Begin::Done);
    }
    let args = begin_again(installer, dest)?;
    Ok(Begin::Run {
        args,
        work: work_dir(dest),
    })
}

/// [`begin`] without the [`Begin::Done`] shortcut: a fresh work folder
/// ([`work_dir`]) and innoextract's arguments. For a `dest` that is the
/// launcher's own (it holds the [`MARKER`]) but fails the game check, as
/// after the player deleted files: it is unpacked again and replaced by
/// [`finish`]. A `dest` without the marker is still refused.
pub fn begin_again(installer: &Installer, dest: &Path) -> Result<Vec<OsString>, String> {
    ours(dest)?;
    let work = work_dir(dest);
    ours(&work)?;
    remove_dir(&work).map_err(|error| cannot(&work, error))?;
    fs::create_dir_all(&work).map_err(|error| cannot(&work, error))?;
    fs::write(work.join(MARKER), identify::hex(&installer.sha256))
        .map_err(|error| cannot(&work, error))?;
    Ok(innoextract_args(installer, &work))
}

/// `Ok` when `dir` is missing, an empty folder, or holds the [`MARKER`].
fn ours(dir: &Path) -> Result<(), String> {
    let empty = || fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none());
    if !dir.exists() || dir.join(MARKER).is_file() || (dir.is_dir() && empty()) {
        return Ok(());
    }
    Err(format!(
        "{} already exists and is not a folder this launcher unpacked; \
         move it away or choose another folder for the launcher's copy.",
        dir.display()
    ))
}

/// The bytes innoextract has written into `work` so far (files that
/// vanish while the folder is read count as nothing).
pub fn unpacked_so_far(work: &Path) -> u64 {
    let mut total = 0;
    let mut dirs = vec![work.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => dirs.push(entry.path()),
                Ok(kind) if kind.is_file() => {
                    total += entry.metadata().map_or(0, |m| m.len());
                }
                _ => {}
            }
        }
    }
    total
}

/// A finished unpack: the launcher's game check result, and what
/// [`finish`] changed in innoextract's output, `/`-separated below the
/// game folder, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished<T> {
    pub game: T,
    /// `X#component` copies that replaced `X` ([`InstallerBuild::prefer`]).
    pub preferred: Vec<String>,
    /// Other `X#component` copies, removed.
    pub removed: Vec<String>,
    /// [`InstallerBuild::drop`] files that were there, removed.
    pub dropped: Vec<String>,
}

impl<T> Finished<T> {
    fn only(game: T) -> Finished<T> {
        Finished {
            game,
            preferred: Vec::new(),
            removed: Vec::new(),
            dropped: Vec::new(),
        }
    }

    /// One line per change, for `launcher.log`.
    pub fn log_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (what, list) in [
            ("kept the preferred copy", &self.preferred),
            ("removed the other copy", &self.removed),
            ("removed (the launcher bundles its own)", &self.dropped),
        ] {
            lines.extend(list.iter().map(|path| format!("unpack: {what}: {path}")));
        }
        lines
    }
}

/// After innoextract succeeded: resolves collisions, drops the files the
/// launcher bundles, runs `check` (the launcher's game check: a known
/// executable, say) on the result and, when it passes, moves the game to
/// `dest` (replacing only a folder with the [`MARKER`]) and returns
/// `check(dest)`. A failed check removes the work folder and leaves
/// `dest` as it was.
pub fn finish<T>(
    installer: &Installer,
    dest: &Path,
    check: impl Fn(&Path) -> Result<T, String>,
) -> Result<Finished<T>, String> {
    let work = work_dir(dest);
    let result = finish_in(installer, dest, &work, &check);
    if result.is_err() {
        let _ = abandon(dest);
    }
    let changes = result?;
    let game = check(dest)?;
    Ok(Finished {
        game,
        preferred: changes.preferred,
        removed: changes.removed,
        dropped: changes.dropped,
    })
}

fn finish_in<T>(
    installer: &Installer,
    dest: &Path,
    work: &Path,
    check: &impl Fn(&Path) -> Result<T, String>,
) -> Result<Finished<()>, String> {
    let root = paths::find_ignoring_case(work, installer.build.root)
        .filter(|p| p.is_dir())
        .ok_or_else(|| format!("The installer gave no game files in {}.", work.display()))?;
    let (preferred, removed) =
        resolve_collisions(&root, installer.build.prefer).map_err(|e| cannot(&root, e))?;
    let mut dropped = Vec::new();
    for rel in installer.build.drop {
        if let Some(file) = paths::find_ignoring_case(&root, rel).filter(|p| p.is_file()) {
            fs::remove_file(&file).map_err(|error| cannot(&file, error))?;
            dropped.push((*rel).to_owned());
        }
    }
    check(&root)?;
    fs::write(root.join(MARKER), identify::hex(&installer.sha256))
        .map_err(|error| cannot(&root, error))?;
    ours(dest)?;
    remove_dir(dest).map_err(|error| cannot(dest, error))?;
    fs::rename(&root, dest).map_err(|error| cannot(dest, error))?;
    remove_dir(work).map_err(|error| cannot(work, error))?;
    Ok(Finished {
        game: (),
        preferred,
        removed,
        dropped,
    })
}

/// innoextract's `--collisions=rename` leftovers below `root`: a file
/// `X#suffix` beside a file `X`. A preferred suffix replaces `X`; any
/// other leftover goes. A `#` in a name with no `X` beside it is a game
/// file and stays. Returns (preferred, removed), `/`-separated.
fn resolve_collisions(root: &Path, prefer: &[&str]) -> io::Result<(Vec<String>, Vec<String>)> {
    let (mut preferred, mut removed) = (Vec::new(), Vec::new());
    let mut dirs = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel)) = dirs.pop() {
        let mut files = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let child = format!("{rel}{name}");
            if entry.file_type()?.is_dir() {
                dirs.push((entry.path(), format!("{child}/")));
            } else {
                files.push(name);
            }
        }
        files.sort();
        // The preferred copies first, so a removed leftover never takes
        // the place of one that is kept.
        let mut leftovers: Vec<(String, String, bool)> = files
            .iter()
            .filter_map(|name| {
                let (base, suffix) = name.rsplit_once('#')?;
                let base = files.iter().find(|f| f.eq_ignore_ascii_case(base))?;
                let wins = prefer.iter().any(|p| p.eq_ignore_ascii_case(suffix));
                Some((name.clone(), base.clone(), wins))
            })
            .collect();
        leftovers.sort_by_key(|(_, _, wins)| !wins);
        let mut replaced: Vec<String> = Vec::new();
        for (name, base, wins) in leftovers {
            let file = dir.join(&name);
            if wins && !replaced.iter().any(|r| r.eq_ignore_ascii_case(&base)) {
                fs::remove_file(dir.join(&base))?;
                fs::rename(&file, dir.join(&base))?;
                replaced.push(base);
                preferred.push(format!("{rel}{name}"));
            } else {
                fs::remove_file(&file)?;
                removed.push(format!("{rel}{name}"));
            }
        }
    }
    preferred.sort();
    removed.sort();
    Ok((preferred, removed))
}

/// Unpacks `installer` into `dest` in one call: [`begin`], `run` (which
/// runs innoextract with [`innoextract_args`]; its error is returned as
/// is), [`finish`]. When `dest` already holds the game, only `check(dest)`
/// runs; when that check fails, the launcher's folder is unpacked again
/// ([`begin_again`]). For a launcher without a window; `rea_launcher_win32` drives the
/// same steps with progress.
pub fn unpack<T>(
    installer: &Installer,
    dest: &Path,
    run: impl FnOnce(&[OsString]) -> Result<(), String>,
    check: impl Fn(&Path) -> Result<T, String>,
) -> Result<Finished<T>, String> {
    let args = match begin(installer, dest)? {
        Begin::Done => match check(dest) {
            Ok(game) => return Ok(Finished::only(game)),
            Err(_) => begin_again(installer, dest)?,
        },
        Begin::Run { args, .. } => args,
    };
    if let Err(error) = run(&args) {
        let _ = abandon(dest);
        return Err(error);
    }
    finish(installer, dest, check)
}

/// A stopped or failed unpack: its work folder goes (only when it is the
/// launcher's, with the [`MARKER`]), `dest` is untouched.
pub fn abandon(dest: &Path) -> Result<(), String> {
    let work = work_dir(dest);
    ours(&work)?;
    remove_dir(&work).map_err(|error| cannot(&work, error))
}

/// An unpack in progress, as the progress window sees it
/// (`rea_launcher_ui::unpack`); `rea_launcher_win32::innoextract::Job`
/// implements it over innoextract's process.
pub trait UnpackJob {
    /// Bytes written so far.
    fn done(&mut self) -> u64;
    /// The result once the unpacker has ended; `None` while it runs.
    fn finished(&mut self) -> Option<Result<(), String>>;
    /// Stops it (Cancel, or the window closed) and removes its work.
    fn cancel(&mut self);
}

/// How watching an [`UnpackJob`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnpackOutcome {
    /// The unpacker ended, with this result.
    Finished(Result<(), String>),
    /// The player cancelled (the job is already stopped).
    Cancelled,
}

fn remove_dir(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn cannot(path: &Path, error: io::Error) -> String {
    format!("Cannot unpack the game into {}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const REPORT: &str = "https://example.org/issues/new?template=unknown-edition.yml";

    fn temp(name: &str) -> PathBuf {
        crate::test_dir(&format!("installer-{name}-Jogos d'Ação"))
    }

    static BUILD: InstallerBuild = InstallerBuild {
        name: "test installer",
        setup: "Game_Setup.exe",
        parts: &[("Game_Setup-1.bin", 10), ("Game_Setup-2.bin", 3)],
        unpacked: 8,
        root: "app",
        prefer: &["base"],
        drop: &["dinput8.dll", "Plugins/Old Fix.asi", "not-there.dll"],
    };

    /// [`BUILD`] under the digest of `b"setup"`, leaked for the 'static table.
    fn table() -> &'static [Known<InstallerBuild>] {
        Box::leak(Box::new([Known {
            sha256: identify::sha256(b"setup"),
            value: BUILD,
        }]))
    }

    /// A fake installer: a setup program, and data parts of the right
    /// sizes (sparse, so nothing big is written).
    fn fake_installer(dir: &Path) -> Installer {
        fs::write(dir.join("GAME_SETUP.EXE"), b"setup").unwrap();
        for (name, size) in BUILD.parts {
            fs::File::create(dir.join(name))
                .unwrap()
                .set_len(*size)
                .unwrap();
        }
        Installer {
            setup: dir.join("GAME_SETUP.EXE"),
            sha256: identify::sha256(b"setup"),
            build: &BUILD,
        }
    }

    /// The launcher's game check: a `game.exe` holding "good".
    fn check(dir: &Path) -> Result<PathBuf, String> {
        let exe = paths::find_ignoring_case(dir, "game.exe").ok_or("no game.exe")?;
        (fs::read(&exe).unwrap() == b"good")
            .then_some(exe)
            .ok_or_else(|| "game.exe is not a build this launcher knows".to_owned())
    }

    fn out_dir(args: &[OsString]) -> PathBuf {
        let at = args.iter().position(|a| a == "--output-dir").unwrap();
        PathBuf::from(&args[at + 1])
    }

    /// What innoextract writes for the fake installer: the game, a temp
    /// folder, a collision (`#base` preferred, `#extra` not), the files
    /// the launcher bundles itself, and a game file with `#` in its name.
    fn extract(args: &[OsString]) -> Result<(), String> {
        let app = out_dir(args).join("app");
        for (rel, bytes) in [
            ("game.exe", &b"good"[..]),
            ("Data/Cars/P206/p206.big", b"livery"),
            ("Data/Cars/P206/p206.big#base", b"default"),
            ("Data/Cars/P206/Two_play/p206.big", b"livery 2"),
            ("Data/Cars/P206/Two_play/p206.big#base", b"default 2"),
            ("Data/Cars/P206/Two_play/p206.big#extra", b"extra"),
            ("Data/notes#1.txt", b"a game file"),
            ("DINPUT8.DLL", b"repack's wrapper"),
            ("plugins/old fix.asi", b"repack's fix"),
        ] {
            let file = paths::native(&app, rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
        fs::create_dir_all(out_dir(args).join("tmp")).unwrap();
        Ok(())
    }

    #[test]
    fn a_folder_without_an_installer_is_none() {
        let dir = temp("none");
        fs::write(dir.join("readme.txt"), b"hi").unwrap();
        fs::write(dir.join("tool.exe"), b"MZ not an installer").unwrap();
        assert_eq!(detect(&dir, table(), REPORT).unwrap(), None);
        assert_eq!(detect(&dir.join("missing"), table(), REPORT).unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_known_installer_is_found_with_its_parts() {
        let dir = temp("known");
        let expected = fake_installer(&dir);
        let found = detect(&dir, table(), REPORT).unwrap().unwrap();
        assert_eq!(found.build.name, "test installer");
        assert_eq!(found.setup, expected.setup, "spelled as on disk");
        assert_eq!(found.sha256, expected.sha256);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_or_short_part_is_named() {
        let dir = temp("parts");
        fake_installer(&dir);
        fs::File::create(dir.join("Game_Setup-2.bin"))
            .unwrap()
            .set_len(2)
            .unwrap();
        let error = detect(&dir, table(), REPORT).unwrap_err();
        assert!(
            error.contains("Game_Setup-2.bin has 2 bytes instead of 3"),
            "{error}"
        );
        fs::remove_file(dir.join("Game_Setup-1.bin")).unwrap();
        let error = detect(&dir, table(), REPORT).unwrap_err();
        assert!(error.contains("Game_Setup-1.bin is missing"), "{error}");
        assert!(error.contains("Game_Setup.exe"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_known_name_with_another_digest_is_left_alone_and_reported() {
        let dir = temp("digest");
        fs::write(dir.join("game_setup.exe"), b"another build").unwrap();
        let error = detect(&dir, table(), REPORT).unwrap_err();
        assert!(
            error.contains(&identify::hex(&identify::sha256(b"another build"))),
            "{error}"
        );
        assert!(
            error.contains(REPORT) && error.contains("left alone"),
            "{error}"
        );
        assert!(error.contains("game_setup.exe"), "{error}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn any_other_inno_setup_installer_is_reported_too() {
        let dir = temp("inno");
        let mut bytes = vec![b'M', b'Z'];
        bytes.resize(300_000, 0);
        bytes.extend_from_slice(b"Inno Setup Setup Data (5.4.2) (u)");
        fs::write(dir.join("Other Game Setup.EXE"), &bytes).unwrap();
        assert!(is_inno_setup(&dir.join("Other Game Setup.EXE")).unwrap());
        let error = detect(&dir, table(), REPORT).unwrap_err();
        assert!(error.contains("Other Game Setup.EXE"), "{error}");
        assert!(
            error.contains(&identify::hex(&identify::sha256(&bytes))),
            "{error}"
        );
        // Past the scanned head it isn't seen (an installed game's exe that
        // happens to quote the text much later is not an installer).
        let mut late = vec![0; INNO_SCAN_BYTES as usize];
        late.extend_from_slice(INNO_SIGNATURE);
        fs::write(dir.join("Other Game Setup.EXE"), &late).unwrap();
        assert_eq!(detect(&dir, table(), REPORT).unwrap(), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn innoextract_extracts_only_the_game_checked_with_collisions_renamed() {
        let installer = Installer {
            setup: PathBuf::from(r"D:\Jogos d'Ação\Game_Setup.exe"),
            sha256: [0; 32],
            build: &BUILD,
        };
        let args = innoextract_args(&installer, Path::new(r"C:\x y"));
        assert_eq!(
            args,
            [
                "--extract",
                "--test",
                "--exclude-temp",
                "--silent",
                "--progress=0",
                "--collisions=rename",
                "--include",
                "app",
                "--output-dir",
                r"C:\x y",
                r"D:\Jogos d'Ação\Game_Setup.exe",
            ]
        );
        assert_eq!(
            work_dir(Path::new("/a/game")),
            PathBuf::from("/a/game.unpacking")
        );
    }

    #[test]
    fn free_space_is_checked_with_a_margin() {
        let big = InstallerBuild {
            unpacked: 3_000_000_000,
            ..BUILD
        };
        assert_eq!(space_needed(&big), 3_300_000_000);
        assert_eq!(space_needed(&BUILD), 8 + (64 << 20));
        assert!(check_space(&big, Some(3_300_000_000), Path::new("D:\\")).is_ok());
        assert!(check_space(&big, None, Path::new("D:\\")).is_ok());
        let error = check_space(&big, Some(1_000_000_000), Path::new("D:\\")).unwrap_err();
        assert_eq!(
            error,
            "Unpacking the game needs about 3.3 GB free in D:\\; there are 1.0 GB."
        );
    }

    #[test]
    fn the_player_unpacks_once_and_later_starts_reuse_it() {
        let root = temp("unpack");
        let installer = fake_installer(&root);
        let dest = root.join("game copy");
        let runs = Cell::new(0);
        let run = |args: &[OsString]| {
            runs.set(runs.get() + 1);
            assert_eq!(args.last().unwrap(), installer.setup.as_os_str());
            extract(args)
        };
        let done = unpack(&installer, &dest, run, check).unwrap();
        assert_eq!(done.game, dest.join("game.exe"));
        assert!(is_unpacked(&dest, &installer));
        assert!(!work_dir(&dest).exists(), "the work folder is removed");
        assert!(!dest.join("tmp").exists() && !dest.join("app").exists());

        // The preferred component's copy took the plain name, everywhere.
        let p206 = dest.join("Data/Cars/P206");
        assert_eq!(fs::read(p206.join("p206.big")).unwrap(), b"default");
        assert_eq!(
            fs::read(p206.join("Two_play/p206.big")).unwrap(),
            b"default 2"
        );
        assert!(!p206.join("p206.big#base").exists());
        assert!(!p206.join("Two_play/p206.big#extra").exists());
        assert_eq!(
            done.preferred,
            [
                "Data/Cars/P206/Two_play/p206.big#base",
                "Data/Cars/P206/p206.big#base"
            ]
        );
        assert_eq!(done.removed, ["Data/Cars/P206/Two_play/p206.big#extra"]);
        // A `#` with no plain twin is the game's own file.
        assert_eq!(
            fs::read(dest.join("Data/notes#1.txt")).unwrap(),
            b"a game file"
        );
        // The repack's own wrapper and fix are gone; a listed file that
        // wasn't there is no error.
        assert!(!dest.join("DINPUT8.DLL").exists());
        assert!(!dest.join("plugins/old fix.asi").exists());
        assert_eq!(done.dropped, ["dinput8.dll", "Plugins/Old Fix.asi"]);
        assert_eq!(done.log_lines().len(), 5);

        let again = unpack(&installer, &dest, run, check).unwrap();
        assert_eq!(runs.get(), 1, "the second start reuses the unpacked game");
        assert!(again.preferred.is_empty() && again.dropped.is_empty());

        // The player deleted game files: the folder is still the
        // launcher's (marked), so it is unpacked again, not refused.
        fs::remove_file(dest.join("game.exe")).unwrap();
        assert!(is_unpacked(&dest, &installer), "the marker stays");
        let repaired = unpack(&installer, &dest, run, check).unwrap();
        assert_eq!(runs.get(), 2, "unpacked again");
        assert_eq!(repaired.game, dest.join("game.exe"));
        assert!(!work_dir(&dest).exists());

        // Another installer build: the launcher's own folder is replaced.
        let newer = Installer {
            sha256: identify::sha256(b"newer"),
            ..installer.clone()
        };
        assert!(!is_unpacked(&dest, &newer));
        unpack(&newer, &dest, run, check).unwrap();
        assert_eq!(runs.get(), 3);
        assert!(is_unpacked(&dest, &newer));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_unpack_leaves_no_game_behind() {
        let root = temp("failed");
        let installer = fake_installer(&root);
        let dest = root.join("game");
        let error = unpack(
            &installer,
            &dest,
            |_| Err("innoextract failed".into()),
            check,
        );
        assert_eq!(error.unwrap_err(), "innoextract failed");
        assert!(!dest.exists() && !work_dir(&dest).exists());
        // innoextract "succeeded" without the game: refused, nothing kept.
        let error = unpack(&installer, &dest, |_| Ok(()), check).unwrap_err();
        assert!(error.contains("no game files"), "{error}");
        assert!(!dest.exists() && !work_dir(&dest).exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_game_the_check_refuses_is_not_kept() {
        let root = temp("badexe");
        let installer = fake_installer(&root);
        let dest = root.join("game");
        let error = unpack(
            &installer,
            &dest,
            |args| {
                extract(args)?;
                fs::write(out_dir(args).join("app/game.exe"), b"bad").unwrap();
                Ok(())
            },
            check,
        )
        .unwrap_err();
        assert!(error.contains("not a build this launcher knows"), "{error}");
        assert!(!dest.exists() && !work_dir(&dest).exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_the_player_made_is_never_replaced_or_removed() {
        let root = temp("players");
        let installer = fake_installer(&root);
        let dest = root.join("game");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("mine.txt"), b"keep").unwrap();
        let error = unpack(&installer, &dest, |_| panic!("must not run"), check).unwrap_err();
        assert!(
            error.contains("not a folder this launcher unpacked"),
            "{error}"
        );
        assert_eq!(fs::read(dest.join("mine.txt")).unwrap(), b"keep");

        // A folder of the player's with the work folder's name stays too.
        let other = root.join("other");
        let work = work_dir(&other);
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("mine.txt"), b"keep").unwrap();
        assert!(begin(&installer, &other).is_err());
        assert!(abandon(&other).is_err());
        assert_eq!(fs::read(work.join("mine.txt")).unwrap(), b"keep");

        // An empty folder is fine to take.
        let empty = root.join("empty");
        fs::create_dir_all(&empty).unwrap();
        unpack(&installer, &empty, extract, check).unwrap();
        assert!(is_unpacked(&empty, &installer));
        fs::remove_dir_all(&root).unwrap();
    }

    /// Real innoextract output (read-only; copied first): CMR4's repack,
    /// `innoextract --collisions=rename --include app/Data/Cars/P206 -d OUT
    /// "Colin McRae Rally 04_Repack_Setup.exe"`, then
    /// `REA_INNO_OUTPUT=OUT/app cargo test -p rea-launcher -- --ignored real_innoextract`.
    /// The installed (default) game has the `#base` copies.
    #[test]
    #[ignore = "needs REA_INNO_OUTPUT"]
    fn real_innoextract_collisions_resolve_to_the_default_component() {
        let Some(from) = std::env::var_os("REA_INNO_OUTPUT").map(PathBuf::from) else {
            return;
        };
        let root = temp("real");
        let copy = |rel: &str| {
            let to = paths::native(&root, rel);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            fs::copy(paths::native(&from, rel), to).unwrap();
        };
        for rel in [
            "Data/Cars/P206/p206.big",
            "Data/Cars/P206/p206.big#base",
            "Data/Cars/P206/Two_play/p206.big",
            "Data/Cars/P206/Two_play/p206.big#base",
        ] {
            copy(rel);
        }
        let (preferred, removed) = resolve_collisions(&root, &["base"]).unwrap();
        assert_eq!(preferred.len(), 2);
        assert!(removed.is_empty());
        let size = |rel| fs::metadata(paths::native(&root, rel)).unwrap().len();
        assert_eq!(size("Data/Cars/P206/p206.big"), 3_641_344);
        assert_eq!(size("Data/Cars/P206/Two_play/p206.big"), 772_096);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn progress_counts_the_bytes_written_and_cancel_leaves_nothing() {
        let root = temp("progress");
        let installer = fake_installer(&root);
        let dest = root.join("game");
        let Begin::Run { args, work } = begin(&installer, &dest).unwrap() else {
            panic!("nothing unpacked yet");
        };
        assert_eq!(out_dir(&args), work);
        let marker = unpacked_so_far(&work);
        assert_eq!(marker, 64, "only the work folder's marker so far");
        fs::create_dir_all(work.join("app/data/deep")).unwrap();
        fs::write(work.join("app/game.exe"), b"1234").unwrap();
        fs::write(work.join("app/data/deep/a.big"), b"123456").unwrap();
        assert_eq!(unpacked_so_far(&work), marker + 10);
        assert_eq!(unpacked_so_far(&root.join("missing")), 0);
        // The player cancels: the work folder goes, no game is left behind.
        abandon(&dest).unwrap();
        assert!(!work.exists() && !dest.exists());
        abandon(&dest).unwrap();
        // A begin after a crash reuses the leftover work folder.
        let Begin::Run { .. } = begin(&installer, &dest).unwrap() else {
            panic!()
        };
        fs::write(work.join("half.big"), b"x").unwrap();
        assert!(matches!(
            begin(&installer, &dest).unwrap(),
            Begin::Run { .. }
        ));
        assert!(!work.join("half.big").exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
