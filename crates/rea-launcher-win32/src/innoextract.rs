//! Unpacking a game's installer with the bundled innoextract
//! (`components/innoextract.exe`): the process half of
//! [`rea_launcher::installer`]. CMR2005's `InnoJob` and `unpack`.
//!
//! [`unpack`] runs the whole thing: the free-space check, innoextract
//! started hidden, a progress window (`watch`, normally
//! `rea_launcher_ui::unpack::show`) or, when no window can open, a plain
//! wait, then [`installer::finish`]. Cancel stops only the innoextract
//! process it started and removes its work folder.
//!
//! ```no_run
//! # use std::path::Path;
//! # use rea_launcher::installer::Installer;
//! use rea_launcher_win32::innoextract::{self, Unpacked};
//! # fn run(found: &Installer, components: &Path, dest: &Path) -> Result<(), String> {
//! let check = |dir: &Path| -> Result<(), String> { Ok(()) }; // the launcher's game check
//! let mut log = |line: &str| println!("{line}");
//! match innoextract::unpack(
//!     &components.join(innoextract::EXE),
//!     found,
//!     dest,
//!     check,
//!     &mut log,
//!     |job| Err::<_, String>(format!("no window here {}", job.done())), // or the window
//! )? {
//!     Unpacked::Ready(done) => println!("the game is in {}", dest.display()),
//!     Unpacked::Cancelled => println!("cancelled"),
//! }
//! # Ok(()) }
//! ```

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rea_launcher::installer::{self, Begin, Finished, Installer, UnpackJob, UnpackOutcome};

use crate::disk;
use crate::process::{Launch, Running};

/// innoextract's file name in a launcher's `components/` folder.
pub const EXE: &str = "innoextract.exe";

/// How often [`Job::done`] measures the work folder (counting the files is
/// cheap, but not free).
const SCAN_EVERY: Duration = Duration::from_millis(500);

/// innoextract running into a work folder. Dropped while it still runs, it
/// is cancelled.
#[derive(Debug)]
pub struct Job {
    running: Running,
    work: PathBuf,
    dest: PathBuf,
    done: u64,
    scanned: Option<Instant>,
    ended: bool,
}

impl Job {
    /// Starts `innoextract` (hidden: no console window) with `args` from
    /// [`installer::begin`], writing into [`installer::work_dir`]`(dest)`.
    pub fn spawn(innoextract: &Path, args: &[OsString], dest: &Path) -> Result<Job, String> {
        Job::spawn_with(Launch::new(innoextract), args, dest)
    }

    /// [`Job::spawn`] from a prepared [`Launch`] (an environment, say).
    pub fn spawn_with(launch: Launch, args: &[OsString], dest: &Path) -> Result<Job, String> {
        let running = launch.args(args.iter().cloned()).hidden(true).spawn()?;
        Ok(Job {
            running,
            work: installer::work_dir(dest),
            dest: dest.to_path_buf(),
            done: 0,
            scanned: None,
            ended: false,
        })
    }

    /// Waits for innoextract to end (when no window can watch it).
    pub fn wait(&mut self) -> Result<(), String> {
        loop {
            if let Some(result) = self.finished() {
                return result;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl UnpackJob for Job {
    fn done(&mut self) -> u64 {
        if self.scanned.is_none_or(|at| at.elapsed() >= SCAN_EVERY) {
            self.done = installer::unpacked_so_far(&self.work);
            self.scanned = Some(Instant::now());
        }
        self.done
    }

    fn finished(&mut self) -> Option<Result<(), String>> {
        let code = self.running.exit_code()?;
        self.ended = true;
        Some(if code == 0 {
            Ok(())
        } else {
            Err(format!(
                "Unpacking the game failed (innoextract exit code {code}). \
                 An installer file may be damaged or incomplete: copy or download it again."
            ))
        })
    }

    fn cancel(&mut self) {
        // innoextract has no window: this ends it at once.
        let _ = self.running.close_gracefully(Duration::ZERO);
        self.ended = true;
        let _ = installer::abandon(&self.dest);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        if !self.ended && self.running.is_alive() {
            self.cancel();
        }
    }
}

/// How [`unpack`] ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unpacked<T> {
    /// The game is in `dest` (unpacked now, or already there).
    Ready(Finished<T>),
    /// The player cancelled; nothing was kept.
    Cancelled,
}

/// Unpacks `installer` into `dest` with `innoextract`, once: when `dest`
/// already holds this installer's game only `check(dest)` runs (when it
/// fails, the launcher's folder is unpacked again). Otherwise
/// checks the free space, starts innoextract, hands the job to `watch`
/// (the progress window; when it returns an error, as when no window can
/// open, the unpack is waited for all the same), then
/// [`installer::finish`]. A failure removes the work folder and leaves
/// `dest` as it was. `log` gets every step and change, for `launcher.log`.
pub fn unpack<T>(
    innoextract: &Path,
    installer: &Installer,
    dest: &Path,
    check: impl Fn(&Path) -> Result<T, String>,
    log: &mut dyn FnMut(&str),
    watch: impl FnOnce(&mut dyn UnpackJob) -> Result<UnpackOutcome, String>,
) -> Result<Unpacked<T>, String> {
    unpack_with(Launch::new(innoextract), installer, dest, check, log, watch)
}

/// [`unpack`] from a prepared [`Launch`] of innoextract.
pub fn unpack_with<T>(
    innoextract: Launch,
    installer: &Installer,
    dest: &Path,
    check: impl Fn(&Path) -> Result<T, String>,
    log: &mut dyn FnMut(&str),
    watch: impl FnOnce(&mut dyn UnpackJob) -> Result<UnpackOutcome, String>,
) -> Result<Unpacked<T>, String> {
    let parent = dest.parent().unwrap_or(dest);
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Cannot create {}: {error}", parent.display()))?;
    let args = match installer::begin(installer, dest)? {
        Begin::Done => match check(dest) {
            Ok(game) => {
                return Ok(Unpacked::Ready(Finished {
                    game,
                    preferred: Vec::new(),
                    removed: Vec::new(),
                    dropped: Vec::new(),
                }));
            }
            Err(error) => {
                log(&format!(
                    "{} fails the game check ({error}); unpacking again",
                    dest.display()
                ));
                installer::begin_again(installer, dest)?
            }
        },
        Begin::Run { args, .. } => args,
    };
    if let Err(error) = installer::check_space(installer.build, disk::free_space(parent), parent) {
        let _ = installer::abandon(dest);
        return Err(error);
    }
    log(&format!(
        "unpacking {} ({}, sha256 {}) into {}",
        installer.setup.display(),
        installer.build.name,
        rea_launcher::identify::hex(&installer.sha256),
        dest.display()
    ));
    let mut job = match Job::spawn_with(innoextract, &args, dest) {
        Ok(job) => job,
        Err(error) => {
            let _ = installer::abandon(dest);
            return Err(error);
        }
    };
    let result = match watch(&mut job) {
        Ok(UnpackOutcome::Finished(result)) => result,
        Ok(UnpackOutcome::Cancelled) => {
            job.cancel();
            log("unpacking cancelled by the player");
            return Ok(Unpacked::Cancelled);
        }
        Err(error) => {
            log(&format!(
                "unpack window unavailable ({error}); waiting for innoextract"
            ));
            job.wait()
        }
    };
    if let Err(error) = result {
        let _ = installer::abandon(dest);
        return Err(error);
    }
    let done = installer::finish(installer, dest, check)?;
    for line in done.log_lines() {
        log(&line);
    }
    log(&format!("unpacked into {}", dest.display()));
    Ok(Unpacked::Ready(done))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::identify;
    use std::fs;

    static BUILD: installer::InstallerBuild = installer::InstallerBuild {
        name: "stand-in installer",
        setup: "setup.exe",
        parts: &[],
        unpacked: 1_000,
        root: "app",
        prefer: &["base"],
        drop: &["dinput8.dll"],
    };

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rea-innoextract-{}-{name}-Jogos d'Ação",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn installer(dir: &Path) -> Installer {
        fs::write(dir.join("setup.exe"), b"setup").unwrap();
        Installer {
            setup: dir.join("setup.exe"),
            sha256: identify::sha256(b"setup"),
            build: &BUILD,
        }
    }

    /// The stand-in for innoextract (`examples/inno_child.rs`), told what
    /// to do through `REA_INNO_CHILD`.
    fn child(mode: &str) -> Launch {
        let exe = std::env::current_exe().unwrap();
        let examples = exe.parent().unwrap().parent().unwrap().join("examples");
        Launch::new(examples.join("inno_child.exe")).env("REA_INNO_CHILD", mode)
    }

    fn check(dir: &Path) -> Result<Vec<u8>, String> {
        fs::read(dir.join("game.exe")).map_err(|error| error.to_string())
    }

    fn wait_for(job: &mut dyn UnpackJob) -> Result<UnpackOutcome, String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if let Some(result) = job.finished() {
                return Ok(UnpackOutcome::Finished(result));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Err("the stand-in never ended".into())
    }

    #[test]
    fn the_game_is_unpacked_through_the_window_or_without_one() {
        let root = temp("ok");
        let found = installer(&root);
        let dest = root.join("game copy");
        let mut lines = Vec::new();
        let mut log = |line: &str| lines.push(line.to_owned());
        let done = unpack_with(child("ok"), &found, &dest, check, &mut log, |job| {
            wait_for(job)
        })
        .unwrap();
        let Unpacked::Ready(done) = done else {
            panic!("{done:?}")
        };
        assert_eq!(done.game, b"good");
        assert_eq!(done.preferred, ["p206.big#base"]);
        assert_eq!(done.dropped, ["dinput8.dll"]);
        assert_eq!(fs::read(dest.join("p206.big")).unwrap(), b"default");
        assert!(!dest.join("dinput8.dll").exists());
        assert!(installer::is_unpacked(&dest, &found));
        assert!(!installer::work_dir(&dest).exists());
        assert!(lines[0].starts_with("unpacking "), "{lines:?}");
        assert!(
            lines.last().unwrap().starts_with("unpacked into "),
            "{lines:?}"
        );

        // Already there: nothing runs (a failing stand-in would fail).
        let again = unpack_with(child("fail"), &found, &dest, check, &mut |_| {}, |_| {
            panic!("no window when nothing is unpacked")
        })
        .unwrap();
        assert!(matches!(again, Unpacked::Ready(_)));

        // Game files deleted: the launcher's own folder is unpacked again.
        fs::remove_file(dest.join("game.exe")).unwrap();
        let mut lines = Vec::new();
        let repaired = unpack_with(
            child("ok"),
            &found,
            &dest,
            check,
            &mut |line| lines.push(line.to_owned()),
            |job| wait_for(job),
        )
        .unwrap();
        let Unpacked::Ready(repaired) = repaired else {
            panic!("{repaired:?}")
        };
        assert_eq!(repaired.game, b"good");
        assert!(
            lines[0].contains("unpacking again"),
            "the reason is logged: {lines:?}"
        );

        // No window: the unpack is waited for all the same.
        let other = root.join("other");
        let mut lines = Vec::new();
        let done = unpack_with(
            child("ok"),
            &found,
            &other,
            check,
            &mut |line| lines.push(line.to_owned()),
            |_| Err("no display".into()),
        )
        .unwrap();
        assert!(matches!(done, Unpacked::Ready(_)));
        assert!(lines.iter().any(|l| l.contains("no display")), "{lines:?}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failing_innoextract_leaves_nothing() {
        let root = temp("fail");
        let found = installer(&root);
        let dest = root.join("game");
        let error =
            unpack_with(child("fail"), &found, &dest, check, &mut |_| {}, wait_for).unwrap_err();
        assert!(error.contains("exit code 3"), "{error}");
        assert!(!dest.exists() && !installer::work_dir(&dest).exists());
        let error = unpack(
            &root.join("no-innoextract.exe"),
            &found,
            &dest,
            check,
            &mut |_| {},
            wait_for,
        )
        .unwrap_err();
        assert!(error.contains("cannot start"), "{error}");
        assert!(!installer::work_dir(&dest).exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cancel_stops_innoextract_and_removes_its_work() {
        let root = temp("cancel");
        let found = installer(&root);
        let dest = root.join("game");
        let start = Instant::now();
        let outcome = unpack_with(child("slow"), &found, &dest, check, &mut |_| {}, |job| {
            // Progress shows, then the player presses Cancel.
            while job.done() < 100 {
                assert!(job.finished().is_none());
                assert!(start.elapsed() < Duration::from_secs(30), "no progress");
                std::thread::sleep(Duration::from_millis(50));
            }
            job.cancel();
            Ok(UnpackOutcome::Cancelled)
        })
        .unwrap();
        assert_eq!(outcome, Unpacked::Cancelled);
        assert!(start.elapsed() < Duration::from_secs(30), "not waited out");
        assert!(!dest.exists() && !installer::work_dir(&dest).exists());

        // A job dropped while running is stopped too.
        let installer::Begin::Run { args, .. } = installer::begin(&found, &dest).unwrap() else {
            panic!()
        };
        let mut job = Job::spawn_with(child("slow"), &args, &dest).unwrap();
        assert!(job.finished().is_none());
        drop(job);
        assert!(!installer::work_dir(&dest).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
