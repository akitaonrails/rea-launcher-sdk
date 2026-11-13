//! `rea_wheel.log` next to the DLL, when the settings say `log = on`:
//! the calls the game makes, the devices picked, effects and errors. A new
//! file per run, cut off at [`LIMIT`] so a long session can't fill a disk.

use std::fs::File;
use std::io::Write;
use std::path::Path;

/// The log's file name, next to the DLL.
pub const FILE: &str = "rea_wheel.log";

/// The most a run writes.
pub const LIMIT: u64 = 1 << 20;

/// An open log; writes nothing when the file couldn't be created.
#[derive(Debug, Default)]
pub struct Log {
    file: Option<File>,
    written: u64,
}

impl Log {
    /// A new, empty log at `path`.
    pub fn create(path: &Path) -> Log {
        Log {
            file: File::create(path).ok(),
            written: 0,
        }
    }

    /// Writes `text` as one line, until the log is full.
    pub fn line(&mut self, text: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let line = format!("{text}\n");
        let full = self.written + line.len() as u64 > LIMIT;
        let line = if full { "(log full)\n" } else { line.as_str() };
        if file.write_all(line.as_bytes()).is_err() || full {
            self.file = None;
        }
        self.written += line.len() as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_stops_at_its_limit() {
        let dir = std::env::temp_dir().join(format!("rea-dinput-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let mut log = Log::create(&path);
        log.line("first");
        let long = "x".repeat(1000);
        for _ in 0..2000 {
            log.line(&long);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("first\n"));
        assert!(text.ends_with("(log full)\n"));
        assert!(text.len() as u64 <= LIMIT + 16);
        // A new run starts empty.
        Log::create(&path).line("second");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unwritable_log_is_silent() {
        let mut log = Log::create(Path::new("/nonexistent-folder/rea_wheel.log"));
        log.line("nothing happens");
        assert!(log.file.is_none());
    }
}
