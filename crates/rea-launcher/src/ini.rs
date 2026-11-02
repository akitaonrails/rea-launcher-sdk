//! Section-aware INI editing that keeps everything it does not manage.
//!
//! OR's `ini.rs` (the audit's pick: comments, unknown keys and the newline
//! shape survive) with the fixes the others found: keys and sections match
//! without regard to case or the spaces around `=`, `;` and `#` both start
//! comments (OR a4da4ed, SRR 5bae1ec — `Width = 1280` was not matched and
//! got a duplicate); later duplicates of a key we set are dropped; a new
//! section starts on its own line (CMR3 15b6aac); CRLF is kept (SRR); and
//! [`Ini::from_bytes`] edits a file in any encoding byte for byte, keeping
//! a UTF-8 BOM (SRR f7fbc67 — a controller name in another encoding made
//! the file unreadable, and the launcher replaced it with a seed). Each
//! line keeps its own ending, so an unedited line is unchanged to the byte.
//!
//! Keys before the first header belong to the section `""`.

/// The UTF-8 byte-order mark some editors write.
const BOM: &str = "\u{feff}";

/// One line and the line ending it had (`""`: the last line, without one).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    text: String,
    eol: &'static str,
}

/// An INI file being edited. Unedited lines keep their exact bytes,
/// line endings included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<Line>,
    bom: bool,
    /// Not UTF-8: held one byte per character (see [`Ini::from_bytes`]).
    bytes: bool,
    /// What goes between a key and its value when we write one.
    pub separator: &'static str,
}

fn header(line: &str) -> Option<&str> {
    let t = line.trim();
    t.strip_prefix('[')?.strip_suffix(']').map(str::trim)
}

/// The key of an active `key = value` line.
fn key_of(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.starts_with(['#', ';', '[']) {
        return None;
    }
    t.split_once('=').map(|(k, _)| k.trim())
}

impl Ini {
    /// Parses text; keys are written back as `key = value`.
    pub fn parse(text: &str) -> Ini {
        let (bom, text) = match text.strip_prefix(BOM) {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let mut lines = Vec::new();
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            if last && part.is_empty() {
                break;
            }
            let (text, eol) = match (last, part.strip_suffix('\r')) {
                (true, _) => (part, ""),
                (false, Some(text)) => (text, "\r\n"),
                (false, None) => (part, "\n"),
            };
            lines.push(Line {
                text: text.to_owned(),
                eol,
            });
        }
        Ini {
            lines,
            bom,
            bytes: false,
            separator: " = ",
        }
    }

    /// Parses a file's bytes. Valid UTF-8 is read as text (as
    /// [`Ini::parse`]); anything else is held one byte per character
    /// (Latin-1), so a name in any 8-bit encoding is written back byte for
    /// byte by [`Ini::to_bytes`]. A UTF-8 BOM is kept either way.
    pub fn from_bytes(bytes: &[u8]) -> Ini {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return Ini::parse(text);
        }
        let (bom, body) = match bytes.strip_prefix(BOM.as_bytes()) {
            Some(body) => (true, body),
            None => (false, bytes),
        };
        let text: String = body.iter().map(|&b| char::from(b)).collect();
        Ini {
            bom,
            bytes: true,
            ..Ini::parse(&text)
        }
    }

    /// Writes `key=value` instead of `key = value` (SRR's `config.ini`).
    pub fn tight(mut self) -> Ini {
        self.separator = "=";
        self
    }

    /// False when the file was not UTF-8: then only [`Ini::to_bytes`]
    /// writes it back faithfully.
    pub fn is_text(&self) -> bool {
        !self.bytes
    }

    /// The ending for new lines: CRLF when the file has any (Notepad).
    fn eol(&self) -> &'static str {
        if self.lines.iter().any(|l| l.eol == "\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    fn joined(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push_str(BOM);
        }
        for line in &self.lines {
            out.push_str(&line.text);
            out.push_str(line.eol);
        }
        out
    }

    /// The edited text, or `None` for a file that was not UTF-8: its
    /// characters stand for bytes, and writing them as UTF-8 would corrupt
    /// it. Use [`Ini::to_bytes`] to write any file.
    pub fn to_text(&self) -> Option<String> {
        self.is_text().then(|| self.joined())
    }

    /// The edited file as bytes: UTF-8 for a text file; for a file that
    /// was not UTF-8, its own bytes, with what we wrote in it (ASCII keys
    /// and values; other characters up to U+00FF as one byte, beyond as
    /// UTF-8).
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = self.joined();
        if !self.bytes {
            return text.into_bytes();
        }
        let mut out = Vec::with_capacity(text.len());
        let body = match text.strip_prefix(BOM) {
            Some(rest) => {
                out.extend_from_slice(BOM.as_bytes());
                rest
            }
            None => &text,
        };
        for c in body.chars() {
            match u8::try_from(c) {
                Ok(b) => out.push(b),
                Err(_) => out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
            }
        }
        out
    }

    /// Inserts a line at `at`. Appended after a last line without an
    /// ending, it takes over that shape (the file still ends without one).
    fn insert(&mut self, at: usize, text: String) {
        let eol = self.eol();
        let mut new = Line { text, eol };
        if at == self.lines.len()
            && let Some(last) = self.lines.last_mut()
            && last.eol.is_empty()
        {
            last.eol = eol;
            new.eol = "";
        }
        self.lines.insert(at, new);
    }

    /// Removes line `n`, keeping a missing final ending missing.
    fn remove_line(&mut self, n: usize) {
        let gone = self.lines.remove(n);
        if gone.eol.is_empty()
            && n == self.lines.len()
            && let Some(last) = self.lines.last_mut()
        {
            last.eol = "";
        }
    }

    /// The line range of `[section]`'s body (`""`: before any header), and
    /// the header's index.
    fn section(&self, section: &str) -> Option<(Option<usize>, std::ops::Range<usize>)> {
        let next_header = |from: usize| {
            self.lines[from..]
                .iter()
                .position(|l| header(&l.text).is_some())
                .map_or(self.lines.len(), |n| from + n)
        };
        if section.is_empty() {
            return Some((None, 0..next_header(0)));
        }
        let start = self
            .lines
            .iter()
            .position(|l| header(&l.text).is_some_and(|h| h.eq_ignore_ascii_case(section)))?;
        Some((Some(start), start + 1..next_header(start + 1)))
    }

    fn matches(&self, range: std::ops::Range<usize>, key: &str) -> Vec<usize> {
        range
            .filter(|&n| key_of(&self.lines[n].text).is_some_and(|k| k.eq_ignore_ascii_case(key)))
            .collect()
    }

    /// The value of `key` in `[section]` (the first one), trimmed. In a
    /// file that was not UTF-8, bytes above 0x7F read as Latin-1.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let (_, range) = self.section(section)?;
        let n = *self.matches(range, key).first()?;
        self.lines[n].text.split_once('=').map(|(_, v)| v.trim())
    }

    /// Sets `key` in `[section]`: the first match is replaced in place in
    /// our spelling, later duplicates in the section are dropped, a missing
    /// key goes at the section's end (before its trailing blank lines), and
    /// a missing section is appended to the file on its own line.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let entry = format!("{key}{}{value}", self.separator);
        let Some((start, range)) = self.section(section) else {
            if self.lines.last().is_some_and(|l| !l.text.trim().is_empty()) {
                self.insert(self.lines.len(), String::new());
            }
            self.insert(self.lines.len(), format!("[{section}]"));
            self.insert(self.lines.len(), entry);
            return;
        };
        let found = self.matches(range.clone(), key);
        if let Some((&first, rest)) = found.split_first() {
            self.lines[first].text = entry;
            for &n in rest.iter().rev() {
                self.remove_line(n);
            }
            return;
        }
        let floor = start.map_or(0, |s| s + 1);
        let mut at = range.end;
        while at > floor && self.lines[at - 1].text.trim().is_empty() {
            at -= 1;
        }
        self.insert(at, entry);
    }

    /// Removes every active `key` line in `[section]`. True when any was.
    pub fn remove(&mut self, section: &str, key: &str) -> bool {
        let Some((_, range)) = self.section(section) else {
            return false;
        };
        let found = self.matches(range, key);
        for &n in found.iter().rev() {
            self.remove_line(n);
        }
        !found.is_empty()
    }

    /// Every active `(section, key)` pair, in file order.
    pub fn section_keys(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut section = String::new();
        for line in &self.lines {
            if let Some(h) = header(&line.text) {
                section = h.to_owned();
            } else if let Some(key) = key_of(&line.text) {
                out.push((section.clone(), key.to_owned()));
            }
        }
        out
    }
}

/// One value the launcher wrote: what it wrote, and what was there before
/// (`None`: the key was absent).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Wrote {
    section: String,
    key: String,
    written: String,
    previous: Option<String>,
}

/// The launcher's record of the INI values it last wrote, kept in a small
/// file beside the runtime. CMR3 unset an option by deleting its key,
/// wiping a choice the player had since made in the game's own menu; with
/// this record an unset ([`release`]) only undoes what is still ours.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    /// By lower-case (section, key): sections and keys ignore case.
    entries: std::collections::BTreeMap<(String, String), Wrote>,
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\t', "\\t")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn slot(section: &str, key: &str) -> (String, String) {
    (section.to_lowercase(), key.to_lowercase())
}

impl Owned {
    /// Reads the record; a missing file is an empty one, malformed lines
    /// are skipped.
    pub fn load(path: &std::path::Path) -> std::io::Result<Owned> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Owned::default()),
            Err(e) => return Err(crate::fsx::at(path)(e)),
        };
        let mut owned = Owned::default();
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let (section, key, written, previous) = match fields.as_slice() {
                [s, k, w, "-"] => (s, k, w, None),
                [s, k, w, "=", p] => (s, k, w, Some(unescape(p))),
                _ => continue,
            };
            let wrote = Wrote {
                section: unescape(section),
                key: unescape(key),
                written: unescape(written),
                previous,
            };
            owned
                .entries
                .insert(slot(&wrote.section, &wrote.key), wrote);
        }
        Ok(owned)
    }

    /// Writes the record (whole-file swap).
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let text: String = self
            .entries
            .values()
            .map(|w| {
                let previous = w
                    .previous
                    .as_ref()
                    .map_or_else(|| "-".to_owned(), |p| format!("=\t{}", escape(p)));
                format!(
                    "{}\t{}\t{}\t{previous}\n",
                    escape(&w.section),
                    escape(&w.key),
                    escape(&w.written)
                )
            })
            .collect();
        crate::fsx::write_atomic(path, text)
    }

    /// The value the launcher last wrote to (section, key), if any.
    pub fn written(&self, section: &str, key: &str) -> Option<&str> {
        self.entries
            .get(&slot(section, key))
            .map(|w| w.written.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Sets `key` in `[section]` and records it as ours. The value that was
/// there before is remembered the first time (and again whenever the
/// player had changed ours since), so [`release`] can put it back.
pub fn set_owned(ini: &mut Ini, owned: &mut Owned, section: &str, key: &str, value: &str) {
    let value = value.trim();
    let current = ini.get(section, key).map(str::to_owned);
    let slot = slot(section, key);
    let previous = match owned.entries.get(&slot) {
        // Still ours: keep what was there before we first wrote.
        Some(w) if current.as_deref() == Some(w.written.as_str()) => w.previous.clone(),
        _ => current,
    };
    ini.set(section, key, value);
    owned.entries.insert(
        slot,
        Wrote {
            section: section.to_owned(),
            key: key.to_owned(),
            written: value.to_owned(),
            previous,
        },
    );
}

/// What [`release`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Released {
    /// Ours and unchanged: the earlier value is back (or the key is gone).
    Restored,
    /// The player changed it since: left as it is, record dropped.
    PlayerChanged,
    /// Nothing recorded for this key.
    NotOurs,
}

/// Undoes [`set_owned`] for an option the launcher no longer sets (unset
/// means "keep the game's value"): only when the file still holds what we
/// wrote is the earlier value restored or the key removed. Either way the
/// record goes.
pub fn release(ini: &mut Ini, owned: &mut Owned, section: &str, key: &str) -> Released {
    let Some(wrote) = owned.entries.remove(&slot(section, key)) else {
        return Released::NotOurs;
    };
    if ini.get(section, key) != Some(wrote.written.as_str()) {
        return Released::PlayerChanged;
    }
    match &wrote.previous {
        Some(previous) => ini.set(section, key, previous),
        None => {
            ini.remove(section, key);
        }
    }
    Released::Restored
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# OutRun2006Tweaks\n\n[Performance]\n# a comment\nFramerateLimit = 60\n\n[Controls]\nSteeringDeadZone = 0.2\nVibrationMode=0\n\n[Graphics]\nDrawDistanceIncrease = 0\n\n[CDTracks]\n01_Splash_wave.ogg = Splash Wave\nLast_Wave.ogg = Last Wave\nor2ed1.ogg = ending\n";

    fn set(text: &str, section: &str, key: &str, value: &str) -> String {
        let mut ini = Ini::parse(text);
        ini.set(section, key, value);
        ini.to_text().unwrap()
    }

    fn tight(text: &str, section: &str, key: &str, value: &str) -> String {
        let mut ini = Ini::parse(text).tight();
        ini.set(section, key, value);
        ini.to_text().unwrap()
    }

    fn remove(text: &str, section: &str, key: &str) -> String {
        let mut ini = Ini::parse(text);
        ini.remove(section, key);
        ini.to_text().unwrap()
    }

    #[test]
    fn replaces_keys_in_place_and_keeps_comments() {
        let out = set(FILE, "Performance", "FramerateLimit", "0");
        assert_eq!(
            out,
            FILE.replace("FramerateLimit = 60", "FramerateLimit = 0")
        );
        let out = set(FILE, "Controls", "VibrationMode", "1");
        assert_eq!(out, FILE.replace("VibrationMode=0", "VibrationMode = 1"));
        let out = set(FILE, "Graphics", "DrawDistanceIncrease", "4");
        assert!(out.contains("FramerateLimit = 60"));
    }

    #[test]
    fn adds_missing_keys_inside_the_section() {
        let out = set(FILE, "Controls", "UseNewInput", "true");
        let controls = &out[out.find("[Controls]").unwrap()..out.find("[Graphics]").unwrap()];
        assert!(controls.trim_end().ends_with("UseNewInput = true"));
        assert!(controls.contains("SteeringDeadZone = 0.2"));
        let out = set(FILE, "CDTracks", "New_Track.ogg", "New Track");
        assert!(out.ends_with("or2ed1.ogg = ending\nNew_Track.ogg = New Track\n"));
        // Inserted before the section's blank line (SRR).
        let srr = "[Audio]\nQuality=2\n\n[Graphics]\n# note\nCustomKey=keep\n\n[GameController]\nGUID={1234}\n";
        let out = tight(srr, "Graphics", "Quality", "9");
        assert!(out.contains("CustomKey=keep\nQuality=9\n\n[GameController]"));
        assert!(out.contains("[Audio]\nQuality=2\n"));
    }

    #[test]
    fn adds_missing_sections_on_their_own_line() {
        let out = set(FILE, "Overlay", "Enabled", "true");
        assert!(out.ends_with("ending\n\n[Overlay]\nEnabled = true\n"));
        assert_eq!(
            set("", "Performance", "VSync", "1"),
            "[Performance]\nVSync = 1\n"
        );
        // CMR3 15b6aac: no trailing newline must not glue the header on.
        assert_eq!(set("[A]\nx = 1", "B", "y", "2"), "[A]\nx = 1\n\n[B]\ny = 2");
    }

    #[test]
    fn duplicates_left_by_older_launchers_are_dropped() {
        let text = "[Graphics]\nWidth = 1280\nOther=1\nWidth=3840\n[Audio]\nWidth=5\n";
        let out = tight(text, "Graphics", "Width", "2560");
        assert_eq!(out, "[Graphics]\nWidth=2560\nOther=1\n[Audio]\nWidth=5\n");
    }

    #[test]
    fn removes_keys_from_one_section_only() {
        let out = remove(FILE, "CDTracks", "Last_Wave.ogg");
        assert!(!out.contains("Last_Wave.ogg"));
        assert!(out.contains("01_Splash_wave.ogg"));
        let with_comment = "[S]\n# key = 1\nother = 2\n";
        assert_eq!(remove(with_comment, "S", "key"), with_comment);
        let mut ini = Ini::parse(with_comment);
        assert!(!ini.remove("Nope", "other"));
        assert!(ini.remove("s", "OTHER"));
    }

    #[test]
    fn lists_section_and_key_pairs() {
        let keys = Ini::parse(FILE).section_keys();
        assert!(keys.contains(&("Performance".into(), "FramerateLimit".into())));
        assert!(keys.contains(&("Controls".into(), "VibrationMode".into())));
        assert!(!keys.iter().any(|(_, k)| k.starts_with('#')));
        assert_eq!(keys.len(), 7);
    }

    #[test]
    fn reads_values_back() {
        let ini = Ini::parse(FILE);
        assert_eq!(ini.get("Performance", "FramerateLimit"), Some("60"));
        assert_eq!(ini.get("Controls", "VibrationMode"), Some("0"));
        assert_eq!(ini.get("Controls", "Nope"), None);
        assert_eq!(ini.get("Nope", "FramerateLimit"), None);
    }

    #[test]
    fn keys_before_any_header_are_the_empty_section() {
        let text = "Width = 640\nHeight=480\n\n[Other]\nWidth = 1\n";
        let mut ini = Ini::parse(text);
        assert_eq!(ini.get("", "width"), Some("640"));
        ini.set("", "Width", "1920");
        ini.set("", "Fullscreen", "1");
        assert_eq!(
            ini.to_text().unwrap(),
            "Width = 1920\nHeight=480\nFullscreen = 1\n\n[Other]\nWidth = 1\n"
        );
        let mut empty = Ini::parse("");
        empty.set("", "a", "b");
        assert_eq!(empty.to_text().unwrap(), "a = b\n");
    }

    #[test]
    fn newline_shape_is_kept() {
        assert_eq!(
            tight("[Graphics]\r\nVSync=0\r\n", "Graphics", "VSync", "1"),
            "[Graphics]\r\nVSync=1\r\n"
        );
        assert_eq!(
            tight("[Graphics]\nVSync=0", "Graphics", "RunWindowed", "0"),
            "[Graphics]\nVSync=0\nRunWindowed=0"
        );
    }

    #[test]
    fn comments_and_case() {
        let text = "[S]\n; key = 1\n;other = 2\nkey = 3\n";
        assert_eq!(Ini::parse(text).get("S", "key"), Some("3"));
        assert_eq!(
            set(text, "S", "key", "9"),
            "[S]\n; key = 1\n;other = 2\nkey = 9\n"
        );
        assert_eq!(
            set("[S]\n;key = 1\n", "S", "key", "2"),
            "[S]\n;key = 1\nkey = 2\n"
        );
        assert_eq!(
            set(
                "[performance]\nframeratelimit = 30\n",
                "Performance",
                "FramerateLimit",
                "60"
            ),
            "[performance]\nFramerateLimit = 60\n"
        );
        assert_eq!(
            set("[ Spaced ]\nk=1\n", "spaced", "k", "2"),
            "[ Spaced ]\nk = 2\n"
        );
    }

    #[test]
    fn bytes_in_any_encoding_round_trip() {
        let existing = b"[GameController]\r\nGUID={1234}\r\nName=Manette \xE9tendue\r\n\r\n[Graphics]\r\nVSync=0\r\n";
        let mut ini = Ini::from_bytes(existing).tight();
        assert_eq!(ini.to_bytes(), existing, "unchanged when not edited");
        ini.set("Graphics", "VSync", "1");
        let out = ini.to_bytes();
        assert!(out.starts_with(b"[GameController]\r\nGUID={1234}\r\nName=Manette \xE9tendue\r\n"));
        assert!(out.ends_with(b"VSync=1\r\n"));
        assert!(
            !ini.is_text() && ini.to_text().is_none(),
            "never written as UTF-8 text"
        );
        // Valid UTF-8 survives byte for byte too.
        let utf8 = "[GameController]\nName=Manette étendue\n".as_bytes();
        let mut ini = Ini::from_bytes(utf8);
        ini.set("Graphics", "VSync", "1");
        assert!(ini.to_bytes().starts_with(utf8));
    }

    #[test]
    fn utf8_files_are_text_and_take_utf8_values() {
        let mut ini = Ini::from_bytes("[Pad]\nName=Manette étendue\n".as_bytes());
        assert!(ini.is_text());
        assert_eq!(ini.get("Pad", "Name"), Some("Manette étendue"));
        ini.set("Pad", "Alias", "Sôlo");
        assert_eq!(
            ini.to_bytes(),
            "[Pad]\nName=Manette étendue\nAlias = Sôlo\n".as_bytes()
        );
        // In a Latin-1 file a value up to U+00FF is one byte.
        let mut latin1 = Ini::from_bytes(b"[Pad]\nName=\xE9\n");
        assert_eq!(latin1.get("Pad", "Name"), Some("é"));
        latin1.set("Pad", "Alias", "Sôlo");
        assert_eq!(latin1.to_bytes(), b"[Pad]\nName=\xE9\nAlias = S\xF4lo\n");
    }

    #[test]
    fn mixed_line_endings_and_a_missing_last_one_round_trip() {
        let bytes = b"[A]\r\nx=1\ny=\xFF\r\nz=2";
        let mut ini = Ini::from_bytes(bytes);
        assert_eq!(ini.to_bytes(), bytes);
        ini.set("A", "w", "3");
        assert_eq!(ini.to_bytes(), b"[A]\r\nx=1\ny=\xFF\r\nz=2\r\nw = 3");
        ini.remove("A", "w");
        assert_eq!(ini.to_bytes(), bytes);
        let mut text = Ini::parse("a = 1\nb = 2");
        text.remove("", "b");
        assert_eq!(text.to_text().unwrap(), "a = 1");
    }

    #[test]
    fn a_bom_does_not_hide_the_first_section_and_is_kept() {
        let bytes = b"\xEF\xBB\xBF[Graphics]\nVSync=0\n";
        let mut ini = Ini::from_bytes(bytes).tight();
        ini.set("Graphics", "VSync", "1");
        assert_eq!(ini.to_bytes(), b"\xEF\xBB\xBF[Graphics]\nVSync=1\n");
        let mut text = Ini::parse("\u{feff}[Graphics]\nVSync=0\n");
        assert_eq!(text.get("Graphics", "VSync"), Some("0"));
        text.set("Graphics", "VSync", "2");
        assert_eq!(text.to_text().unwrap(), "\u{feff}[Graphics]\nVSync = 2\n");
    }

    mod owned {
        use super::super::*;
        use crate::test_dir;

        #[test]
        fn release_undoes_only_what_is_still_ours() {
            let mut ini = Ini::parse("[Graphics]\r\nVSync = 1\r\n");
            let mut owned = Owned::default();
            set_owned(&mut ini, &mut owned, "Graphics", "VSync", "0");
            set_owned(&mut ini, &mut owned, "Graphics", "Aniso", "16");
            // Setting again keeps the value from before our first write.
            set_owned(&mut ini, &mut owned, "graphics", "vsync", "0");
            assert_eq!(owned.written("GRAPHICS", "VSYNC"), Some("0"));
            assert_eq!(
                release(&mut ini, &mut owned, "Graphics", "VSync"),
                Released::Restored
            );
            assert_eq!(
                release(&mut ini, &mut owned, "Graphics", "Aniso"),
                Released::Restored
            );
            assert_eq!(
                ini.to_text().unwrap(),
                "[Graphics]\r\nVSync = 1\r\n",
                "as before, CRLF kept"
            );
            assert_eq!(
                release(&mut ini, &mut owned, "Graphics", "VSync"),
                Released::NotOurs
            );
            assert!(owned.is_empty());
        }

        #[test]
        fn a_value_the_player_changed_in_game_is_kept() {
            let mut ini = Ini::parse("[Graphics]\nVSync = 1\n");
            let mut owned = Owned::default();
            set_owned(&mut ini, &mut owned, "Graphics", "VSync", "0");
            // The game's own menu writes the file.
            ini.set("Graphics", "VSync", "2");
            assert_eq!(
                release(&mut ini, &mut owned, "Graphics", "VSync"),
                Released::PlayerChanged
            );
            assert_eq!(ini.get("Graphics", "VSync"), Some("2"));
            assert!(owned.is_empty(), "the record is dropped");
            // Set over a player's change: their value is what comes back.
            set_owned(&mut ini, &mut owned, "Graphics", "VSync", "0");
            ini.set("Graphics", "VSync", "3");
            set_owned(&mut ini, &mut owned, "Graphics", "VSync", "0");
            release(&mut ini, &mut owned, "Graphics", "VSync");
            assert_eq!(ini.get("Graphics", "VSync"), Some("3"));
        }

        #[test]
        fn latin1_files_and_the_record_round_trip() {
            let bytes = b"[Pad]\nName=Manette \xE9tendue\n";
            let mut ini = Ini::from_bytes(bytes).tight();
            let mut owned = Owned::default();
            set_owned(&mut ini, &mut owned, "Pad", "Name", "Pad\tone");
            set_owned(&mut ini, &mut owned, "Video", "Width", "1920");
            let dir = test_dir("ini-owned");
            let path = dir.join(".owned-ini");
            owned.save(&path).unwrap();
            let mut loaded = Owned::load(&path).unwrap();
            assert_eq!(loaded, owned);
            assert_eq!(
                release(&mut ini, &mut loaded, "Pad", "Name"),
                Released::Restored
            );
            assert_eq!(
                release(&mut ini, &mut loaded, "Video", "Width"),
                Released::Restored
            );
            assert_eq!(
                ini.to_bytes(),
                b"[Pad]\nName=Manette \xE9tendue\n\n[Video]\n"
            );
            assert!(Owned::load(&dir.join("missing")).unwrap().is_empty());
            std::fs::write(&path, "junk\nmore\tjunk\n").unwrap();
            assert!(Owned::load(&path).unwrap().is_empty());
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
