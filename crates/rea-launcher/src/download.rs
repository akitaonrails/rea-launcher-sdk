//! Pinned downloads the player asks for (packs, optional extras).
//!
//! From OR's `hdpack.rs` (the audit's pick), made generic. One job runs on
//! a worker thread: check free space, download (resumable with an HTTP
//! range; a server that ignores the range and sends the whole file starts
//! over), verify the SHA-256 while streaming (bytes already on disk
//! included), then — with the `zip` feature — unpack into a temporary
//! folder (names that would escape it are skipped), rename it into place,
//! mark it complete with the hash and remove older versions. The window
//! polls [`Job::phase`] and can [`Job::cancel`]; a cancelled or broken
//! download resumes next time. Fetching sits behind [`Fetch`] (WinHTTP in
//! `rea-launcher-win32`), so all of this is tested without a network.
//!
//! Everything lives in one folder per component (`root`), which must hold
//! nothing else: older versions there are removed.

use std::cell::RefCell;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::identify::{hex, parse_hex32, sha256_file};

/// Written last into an unpacked version, holding its archive's SHA-256.
pub const MARKER: &str = ".complete";

const CHUNK: usize = 1 << 20;

/// A pinned release: where it is and what it must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pin {
    pub url: String,
    pub sha256: [u8; 32],
    /// The download's size in bytes.
    pub size: u64,
    /// The unpacked files' size in bytes (0 for a file kept as it is).
    pub unpacked_size: u64,
}

impl Pin {
    /// The first `URL SHA-256 size unpacked-size` line of a pin file
    /// (`#` comments and blank lines skipped).
    pub fn parse(text: &str) -> Option<Pin> {
        let line = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'))?;
        let mut field = line.split_whitespace();
        Some(Pin {
            url: field.next()?.to_owned(),
            sha256: parse_hex32(field.next()?)?,
            size: field.next()?.parse().ok()?,
            unpacked_size: field.next()?.parse().ok()?,
        })
    }

    /// The download's file name: the URL's last part, query dropped.
    pub fn file_name(&self) -> &str {
        let path = self.url.split(['?', '#']).next().unwrap_or(&self.url);
        path.rsplit('/').next().unwrap_or(path)
    }

    /// The version's name: the file name without `.zip`.
    pub fn name(&self) -> &str {
        let file = self.file_name();
        file.strip_suffix(".zip").unwrap_or(file)
    }

    /// Where this version is unpacked below `root`.
    pub fn dir(&self, root: &Path) -> PathBuf {
        root.join(self.name())
    }

    /// Free space the rest of the download and the unpacked files need,
    /// with 5 % headroom.
    pub fn space_needed(&self, downloaded: u64) -> u64 {
        self.size.saturating_sub(downloaded) + self.unpacked_size + self.unpacked_size / 20
    }
}

fn part_path(pin: &Pin, root: &Path) -> PathBuf {
    root.join(format!("{}.part", pin.file_name()))
}

fn file_path(pin: &Pin, root: &Path) -> PathBuf {
    root.join(pin.file_name())
}

/// The unpacked version, when a complete one is there.
pub fn installed(pin: &Pin, root: &Path) -> Option<PathBuf> {
    let dir = pin.dir(root);
    let mark = fs::read_to_string(dir.join(MARKER)).ok()?;
    (mark.trim() == hex(&pin.sha256)).then_some(dir)
}

/// Bytes of an interrupted download waiting to be resumed.
pub fn partial(pin: &Pin, root: &Path) -> u64 {
    fs::metadata(part_path(pin, root)).map_or(0, |m| m.len())
}

/// Deletes this version and anything left of its download.
pub fn remove(pin: &Pin, root: &Path) -> std::io::Result<()> {
    // For a file that is not unpacked, `dir` is the file itself.
    let dir = pin.dir(root);
    if fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(&dir).map_err(crate::fsx::at(&dir))?;
    }
    for file in [part_path(pin, root), file_path(pin, root)] {
        crate::fsx::remove(&file)?;
    }
    Ok(())
}

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Downloading {
        done: u64,
        total: u64,
    },
    Verifying,
    Unpacking {
        done: u64,
        total: u64,
    },
    /// The unpacked folder, or the verified file for [`download`].
    Done(PathBuf),
    Failed(String),
    Cancelled,
}

impl Phase {
    pub fn running(&self) -> bool {
        matches!(
            self,
            Phase::Starting
                | Phase::Downloading { .. }
                | Phase::Verifying
                | Phase::Unpacking { .. }
        )
    }

    /// How far a download or unpack is, 0.0 to 1.0.
    pub fn fraction(&self) -> Option<f32> {
        match *self {
            Phase::Downloading { done, total } | Phase::Unpacking { done, total } => {
                Some(if total == 0 {
                    1.0
                } else {
                    (done as f64 / total as f64).min(1.0) as f32
                })
            }
            _ => None,
        }
    }
}

/// A job shared between the worker thread and the window (wrap it in an
/// `Arc`).
#[derive(Debug)]
pub struct Job {
    phase: Mutex<Phase>,
    cancel: AtomicBool,
}

impl Default for Job {
    fn default() -> Job {
        Job {
            phase: Mutex::new(Phase::Starting),
            cancel: AtomicBool::new(false),
        }
    }
}

impl Job {
    pub fn phase(&self) -> Phase {
        self.phase.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Asks the worker to stop; what was downloaded stays for next time.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn set(&self, phase: Phase) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = phase;
    }
}

/// An HTTP GET that streams its body; the platform supplies it.
pub trait Fetch {
    /// Streams `url` from byte `from` (a `Range: bytes=from-` request when
    /// above 0). Before the first chunk, `start` is told whether the body
    /// begins at `from` (true: 206, or `from` is 0) or at byte 0 because
    /// the server ignored the range (false: 200). `chunk` receives the
    /// body and returns false to stop. An error is a short reason.
    fn fetch(
        &self,
        url: &str,
        from: u64,
        start: &mut dyn FnMut(bool),
        chunk: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<(), String>;
}

/// Free space on the volume holding a folder, when known.
pub type FreeSpace<'a> = &'a dyn Fn(&Path) -> Option<u64>;

fn finish(job: &Job, result: Result<PathBuf, String>) -> Phase {
    let phase = match result {
        Ok(path) => Phase::Done(path),
        Err(_) if job.cancelled() => Phase::Cancelled,
        Err(e) => Phase::Failed(e),
    };
    job.set(phase.clone());
    phase
}

/// Downloads `pin` into `root` as its file, verified, without unpacking.
/// Returns (and leaves in `job`) the final phase; `Done` holds the file.
pub fn download(pin: &Pin, root: &Path, free: FreeSpace, fetch: &dyn Fetch, job: &Job) -> Phase {
    let result = (|| {
        let file = file_path(pin, root);
        if verified(pin, &file, job) {
            return Ok(file);
        }
        fetch_to(pin, root, free, fetch, job)
    })();
    finish(job, result)
}

/// True when `file` is there with the pinned hash.
fn verified(pin: &Pin, file: &Path, job: &Job) -> bool {
    file.is_file() && {
        job.set(Phase::Verifying);
        sha256_file(file).is_ok_and(|digest| digest == pin.sha256)
    }
}

/// Downloads into the part file, verifies it and renames it to the file.
fn fetch_to(
    pin: &Pin,
    root: &Path,
    free: FreeSpace,
    fetch: &dyn Fetch,
    job: &Job,
) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|e| format!("cannot create {}: {e}", root.display()))?;
    let part = part_path(pin, root);
    stream(pin, &part, free(root), fetch, job)?;
    let file = file_path(pin, root);
    fs::rename(&part, &file).map_err(|e| format!("cannot keep the download: {e}"))?;
    Ok(file)
}

/// What the download writes as it streams.
struct Sink {
    file: File,
    hasher: Sha256,
    done: u64,
    error: Option<String>,
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000.0
}

fn stream(
    pin: &Pin,
    part: &Path,
    free: Option<u64>,
    fetch: &dyn Fetch,
    job: &Job,
) -> Result<(), String> {
    job.set(Phase::Starting);
    let mut have = fs::metadata(part).map_or(0, |m| m.len());
    if have > pin.size {
        have = 0;
    }
    if let Some(free) = free {
        let need = pin.space_needed(have);
        if free < need {
            return Err(format!(
                "Not enough free space: {:.1} GB needed, {:.1} GB free.",
                need as f64 / 1e9,
                free as f64 / 1e9
            ));
        }
    }
    if job.cancelled() {
        return Err("cancelled".into());
    }
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(part)
        .map_err(|e| format!("cannot write {}: {e}", part.display()))?;
    file.set_len(have).map_err(|e| e.to_string())?;
    // The bytes already downloaded count towards the digest.
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let sink = RefCell::new(Sink {
        file,
        hasher,
        done: have,
        error: None,
    });
    job.set(Phase::Downloading {
        done: have,
        total: pin.size,
    });
    let result = fetch.fetch(
        &pin.url,
        have,
        &mut |resumed| {
            if !resumed {
                // The server sent the whole file: start over.
                let mut s = sink.borrow_mut();
                s.hasher = Sha256::new();
                s.done = 0;
                if let Err(e) = s
                    .file
                    .set_len(0)
                    .and_then(|_| s.file.seek(SeekFrom::Start(0)))
                {
                    s.error = Some(e.to_string());
                }
            }
        },
        &mut |bytes| {
            if job.cancelled() {
                return false;
            }
            let mut s = sink.borrow_mut();
            if s.error.is_some() {
                return false;
            }
            if let Err(e) = s.file.write_all(bytes) {
                s.error = Some(format!("cannot write the download: {e}"));
                return false;
            }
            s.hasher.update(bytes);
            s.done += bytes.len() as u64;
            job.set(Phase::Downloading {
                done: s.done,
                total: pin.size,
            });
            true
        },
    );
    let sink = sink.into_inner();
    if job.cancelled() {
        return Err("cancelled".into());
    }
    if let Some(e) = sink.error {
        return Err(e);
    }
    result.map_err(|e| format!("The download stopped ({e}); it resumes from there next time."))?;
    drop(sink.file);
    if sink.done != pin.size {
        return Err(format!(
            "The download ended early ({:.0} of {:.0} MB); it resumes from there next time.",
            megabytes(sink.done),
            megabytes(pin.size)
        ));
    }
    job.set(Phase::Verifying);
    if <[u8; 32]>::from(sink.hasher.finalize()) != pin.sha256 {
        let _ = fs::remove_file(part);
        return Err(
            "The download arrived damaged (its checksum does not match); try again.".into(),
        );
    }
    Ok(())
}

/// Downloads and unpacks `pin` below `root`, reporting through `job`;
/// a version already complete is returned at once. `free` tells the free
/// space under `root`. Returns (and leaves in `job`) the final phase;
/// `Done` holds the unpacked folder.
#[cfg(feature = "zip")]
pub fn install(pin: &Pin, root: &Path, free: FreeSpace, fetch: &dyn Fetch, job: &Job) -> Phase {
    let result = (|| {
        if let Some(dir) = installed(pin, root) {
            return Ok(dir);
        }
        // A download finished before an interrupted unpack is used again.
        let archive = file_path(pin, root);
        if !verified(pin, &archive, job) {
            fetch_to(pin, root, free, fetch, job)?;
        }
        let dir = unpack(pin, &archive, root, job)?;
        let _ = fs::remove_file(&archive);
        remove_other_versions(root, pin);
        Ok(dir)
    })();
    finish(job, result)
}

/// Unpacks into a temporary folder and renames it into place only when
/// every file is out, so a half-unpacked version is never used.
#[cfg(feature = "zip")]
fn unpack(pin: &Pin, archive: &Path, root: &Path, job: &Job) -> Result<PathBuf, String> {
    let dir = pin.dir(root);
    let tmp = root.join(format!("{}.unpacking", pin.name()));
    for stale in [&tmp, &dir] {
        if stale.exists() {
            fs::remove_dir_all(stale)
                .map_err(|e| format!("cannot clear {}: {e}", stale.display()))?;
        }
    }
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("the archive can't be read: {e}"))?;
    let mut total = 0;
    for i in 0..zip.len() {
        total += zip.by_index_raw(i).map_err(|e| e.to_string())?.size();
    }
    let mut done = 0;
    job.set(Phase::Unpacking { done, total });
    let mut buffer = vec![0; CHUNK];
    for i in 0..zip.len() {
        if job.cancelled() {
            return Err("cancelled".into());
        }
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("the archive can't be read: {e}"))?;
        // Names that would land outside the folder are skipped.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let out = tmp.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut target =
            File::create(&out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        loop {
            let n = entry
                .read(&mut buffer)
                .map_err(|e| format!("the archive is damaged: {e}"))?;
            if n == 0 {
                break;
            }
            target
                .write_all(&buffer[..n])
                .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            done += n as u64;
            job.set(Phase::Unpacking { done, total });
        }
    }
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    fs::write(tmp.join(MARKER), hex(&pin.sha256)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &dir).map_err(|e| format!("cannot finish {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Older versions (folders with the marker) and leftovers of their
/// downloads, once this one is in place. Anything else in `root` stays.
pub fn remove_other_versions(root: &Path, pin: &Pin) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == pin.name() || name.starts_with(pin.file_name()) {
            continue;
        }
        if path.join(MARKER).is_file() {
            let _ = fs::remove_dir_all(&path);
        } else if name.ends_with(".zip.part") || name.ends_with(".zip") {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::sha256;
    use crate::test_dir;
    use std::cell::Cell;

    /// Serves `body`, honouring ranges unless told not to, optionally
    /// stopping after some bytes.
    struct Fake {
        body: Vec<u8>,
        ranges: bool,
        stop_after: Option<usize>,
        asked_from: Cell<Option<u64>>,
    }

    impl Fake {
        fn new(body: &[u8]) -> Fake {
            Fake {
                body: body.to_vec(),
                ranges: true,
                stop_after: None,
                asked_from: Cell::new(None),
            }
        }
    }

    impl Fetch for Fake {
        fn fetch(
            &self,
            _url: &str,
            from: u64,
            start: &mut dyn FnMut(bool),
            chunk: &mut dyn FnMut(&[u8]) -> bool,
        ) -> Result<(), String> {
            self.asked_from.set(Some(from));
            let at = if self.ranges { from as usize } else { 0 };
            start(self.ranges || from == 0);
            let end = self
                .stop_after
                .map_or(self.body.len(), |n| (at + n).min(self.body.len()));
            for piece in self.body[at..end].chunks(700) {
                if !chunk(piece) {
                    return Err("stopped".into());
                }
            }
            match self.stop_after {
                Some(_) => Err("connection reset".into()),
                None => Ok(()),
            }
        }
    }

    fn no_space_check(_: &Path) -> Option<u64> {
        None
    }

    /// Bytes that do not compress.
    fn noise(len: usize) -> Vec<u8> {
        let mut x: u32 = 0x2006;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (x >> 24) as u8
            })
            .collect()
    }

    fn pin_for(bytes: &[u8], file: &str, unpacked: u64) -> Pin {
        Pin {
            url: format!("https://example.org/releases/{file}?raw=1"),
            sha256: sha256(bytes),
            size: bytes.len() as u64,
            unpacked_size: unpacked,
        }
    }

    #[test]
    fn pins_parse() {
        let digest = "ab".repeat(32);
        let pin = Pin::parse(&format!(
            "# a pack\n\nhttps://x.org/r/Pack-v1.zip {digest} 10 20\n"
        ))
        .unwrap();
        assert_eq!((pin.name(), pin.file_name()), ("Pack-v1", "Pack-v1.zip"));
        assert_eq!((pin.size, pin.unpacked_size), (10, 20));
        assert_eq!(pin.sha256, [0xab; 32]);
        assert!(Pin::parse("# only a comment\n").is_none());
        assert!(Pin::parse("https://x/y.zip abc 1 2\n").is_none());
        assert_eq!(pin.space_needed(4), 6 + 20 + 1);
    }

    #[test]
    fn a_file_downloads_resumes_and_is_verified() {
        let root = test_dir("dl-file");
        let body = noise(5000);
        let pin = pin_for(&body, "tool.bin", 0);
        let mut cut = Fake::new(&body);
        cut.stop_after = Some(1000);
        let first = download(&pin, &root, &no_space_check, &cut, &Job::default());
        assert!(
            matches!(first, Phase::Failed(ref e) if e.contains("resumes")),
            "{first:?}"
        );
        assert_eq!(partial(&pin, &root), 1000);
        let rest = Fake::new(&body);
        let job = Job::default();
        let phase = download(&pin, &root, &no_space_check, &rest, &job);
        assert_eq!(rest.asked_from.get(), Some(1000));
        assert_eq!(phase, Phase::Done(root.join("tool.bin")));
        assert_eq!(job.phase(), phase);
        assert_eq!(fs::read(root.join("tool.bin")).unwrap(), body);
        // Already there: not fetched again.
        let again = Fake::new(&body);
        assert!(matches!(
            download(&pin, &root, &no_space_check, &again, &Job::default()),
            Phase::Done(_)
        ));
        assert_eq!(again.asked_from.get(), None);
        remove(&pin, &root).unwrap();
        assert!(!root.join("tool.bin").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_server_that_ignores_ranges_starts_over() {
        let root = test_dir("dl-norange");
        let body = noise(3000);
        let pin = pin_for(&body, "a.bin", 0);
        fs::write(part_path(&pin, &root), &body[..1000]).unwrap();
        let mut whole = Fake::new(&body);
        whole.ranges = false;
        assert_eq!(
            download(&pin, &root, &no_space_check, &whole, &Job::default()),
            Phase::Done(root.join("a.bin"))
        );
        assert_eq!(fs::read(root.join("a.bin")).unwrap(), body);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_damaged_download_is_thrown_away() {
        let root = test_dir("dl-damaged");
        let body = noise(2000);
        let mut pin = pin_for(&body, "a.bin", 0);
        pin.sha256 = [0; 32];
        let phase = download(
            &pin,
            &root,
            &no_space_check,
            &Fake::new(&body),
            &Job::default(),
        );
        assert!(
            matches!(phase, Phase::Failed(ref e) if e.contains("damaged")),
            "{phase:?}"
        );
        assert_eq!(partial(&pin, &root), 0);
        assert!(!root.join("a.bin").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelling_and_too_little_space_stop_before_fetching() {
        let root = test_dir("dl-stop");
        let body = noise(2000);
        let pin = pin_for(&body, "a.bin", 10_000);
        let job = Job::default();
        job.cancel();
        let fake = Fake::new(&body);
        assert_eq!(
            download(&pin, &root, &no_space_check, &fake, &job),
            Phase::Cancelled
        );
        let tight = |_: &Path| Some(10);
        let phase = download(&pin, &root, &tight, &fake, &Job::default());
        assert!(
            matches!(phase, Phase::Failed(ref e) if e.contains("free space")),
            "{phase:?}"
        );
        assert_eq!(fake.asked_from.get(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn progress_is_a_fraction() {
        assert_eq!(
            Phase::Downloading { done: 1, total: 4 }.fraction(),
            Some(0.25)
        );
        assert_eq!(Phase::Unpacking { done: 0, total: 0 }.fraction(), Some(1.0));
        assert_eq!(Phase::Verifying.fraction(), None);
        assert!(Phase::Verifying.running() && !Phase::Cancelled.running());
    }

    #[cfg(feature = "zip")]
    mod archives {
        use super::*;

        /// A zip with a folder, two files and a hostile name.
        fn pack() -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let deflated = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.add_directory("textures/load/", deflated).unwrap();
            zip.start_file("textures/load/a.dds", deflated).unwrap();
            zip.write_all(&noise(5000)).unwrap();
            zip.start_file("Readme.txt", deflated).unwrap();
            zip.write_all(b"credits").unwrap();
            zip.start_file("../escape.txt", deflated).unwrap();
            zip.write_all(b"no").unwrap();
            zip.finish().unwrap().into_inner()
        }

        #[test]
        fn downloads_unpacks_and_marks_complete() {
            let root = test_dir("dl-zip");
            let bytes = pack();
            let pin = pin_for(&bytes, "Pack-vtest.zip", 5009);
            let job = Job::default();
            let phase = install(&pin, &root, &no_space_check, &Fake::new(&bytes), &job);
            let dir = pin.dir(&root);
            assert_eq!(phase, Phase::Done(dir.clone()));
            assert_eq!(
                fs::read(dir.join("textures/load/a.dds")).unwrap(),
                noise(5000)
            );
            assert!(!root.join("escape.txt").exists());
            assert!(!root.parent().unwrap().join("escape.txt").exists());
            assert_eq!(installed(&pin, &root), Some(dir.clone()));
            assert!(
                !root.join("Pack-vtest.zip").exists(),
                "the archive goes once unpacked"
            );
            let again = Fake::new(&bytes);
            assert_eq!(
                install(&pin, &root, &no_space_check, &again, &Job::default()),
                Phase::Done(dir)
            );
            assert_eq!(again.asked_from.get(), None);
            remove(&pin, &root).unwrap();
            assert_eq!(installed(&pin, &root), None);
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn an_interrupted_download_resumes_then_unpacks() {
            let root = test_dir("dl-zip-resume");
            let bytes = pack();
            let pin = pin_for(&bytes, "Pack-vtest.zip", 5009);
            let mut cut = Fake::new(&bytes);
            cut.stop_after = Some(1000);
            assert!(matches!(
                install(&pin, &root, &no_space_check, &cut, &Job::default()),
                Phase::Failed(_)
            ));
            let rest = Fake::new(&bytes);
            assert_eq!(
                install(&pin, &root, &no_space_check, &rest, &Job::default()),
                Phase::Done(pin.dir(&root))
            );
            assert_eq!(rest.asked_from.get(), Some(1000));
            fs::remove_dir_all(root).unwrap();
        }

        #[test]
        fn a_newer_version_replaces_the_old_one() {
            let root = test_dir("dl-zip-newer");
            let bytes = pack();
            let pin = pin_for(&bytes, "Pack-vtest.zip", 5009);
            let old = root.join("Pack-vold");
            fs::create_dir_all(&old).unwrap();
            fs::write(old.join(MARKER), "x").unwrap();
            fs::write(root.join("Pack-vold.zip.part"), "x").unwrap();
            let mine = root.join("my-own-textures");
            fs::create_dir_all(&mine).unwrap();
            install(
                &pin,
                &root,
                &no_space_check,
                &Fake::new(&bytes),
                &Job::default(),
            );
            assert!(!old.exists() && !root.join("Pack-vold.zip.part").exists());
            assert!(mine.exists(), "folders the launcher did not install stay");
            fs::remove_dir_all(root).unwrap();
        }
    }
}
