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
//! the file unreadable, and the launcher replaced it with a seed).
//!
//! Keys before the first header belong to the section `""`.

/// The UTF-8 byte-order mark some editors write.
const BOM: &str = "\u{feff}";

/// An INI file being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ini {
    lines: Vec<String>,
    crlf: bool,
    /// Ends with a newline (an empty file gains one when edited).
    trailing_newline: bool,
    bom: bool,
    /// Read one byte per character (see [`Ini::from_bytes`]).
    latin1: bool,
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
        Ini {
            lines: text.lines().map(str::to_owned).collect(),
            crlf: text.contains("\r\n"),
            trailing_newline: text.is_empty() || text.ends_with('\n'),
            bom,
            latin1: false,
            separator: " = ",
        }
    }

    /// Parses raw bytes one byte per character, so a file in any encoding
    /// (valid UTF-8 or not) is written back byte for byte by
    /// [`Ini::to_bytes`]. Values read this way are Latin-1 decoded.
    pub fn from_bytes(bytes: &[u8]) -> Ini {
        let (bom, body) = match bytes.strip_prefix(BOM.as_bytes()) {
            Some(body) => (true, body),
            None => (false, bytes),
        };
        let text: String = body.iter().map(|&b| char::from(b)).collect();
        Ini {
            bom,
            latin1: true,
            ..Ini::parse(&text)
        }
    }

    /// Writes `key=value` instead of `key = value` (SRR's `config.ini`).
    pub fn tight(mut self) -> Ini {
        self.separator = "=";
        self
    }

    /// The edited text (a BOM read is written back).
    pub fn to_text(&self) -> String {
        let eol = if self.crlf { "\r\n" } else { "\n" };
        let mut out = String::new();
        if self.bom {
            out.push_str(BOM);
        }
        out.push_str(&self.lines.join(eol));
        if self.trailing_newline {
            out.push_str(eol);
        }
        out
    }

    /// The edited file as bytes: one byte per character after
    /// [`Ini::from_bytes`] (characters we wrote beyond Latin-1 go as
    /// UTF-8), else UTF-8.
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = self.to_text();
        if !self.latin1 {
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

    /// The line range of `[section]`'s body (`""`: before any header), and
    /// the header's index.
    fn section(&self, section: &str) -> Option<(Option<usize>, std::ops::Range<usize>)> {
        let next_header = |from: usize| {
            self.lines[from..]
                .iter()
                .position(|l| header(l).is_some())
                .map_or(self.lines.len(), |n| from + n)
        };
        if section.is_empty() {
            return Some((None, 0..next_header(0)));
        }
        let start = self
            .lines
            .iter()
            .position(|l| header(l).is_some_and(|h| h.eq_ignore_ascii_case(section)))?;
        Some((Some(start), start + 1..next_header(start + 1)))
    }

    fn matches(&self, range: std::ops::Range<usize>, key: &str) -> Vec<usize> {
        range
            .filter(|&n| key_of(&self.lines[n]).is_some_and(|k| k.eq_ignore_ascii_case(key)))
            .collect()
    }

    /// The value of `key` in `[section]` (the first one), trimmed.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let (_, range) = self.section(section)?;
        let n = *self.matches(range, key).first()?;
        self.lines[n].split_once('=').map(|(_, v)| v.trim())
    }

    /// Sets `key` in `[section]`: the first match is replaced in place in
    /// our spelling, later duplicates in the section are dropped, a missing
    /// key goes at the section's end (before its trailing blank lines), and
    /// a missing section is appended to the file.
    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let entry = format!("{key}{}{value}", self.separator);
        let Some((start, range)) = self.section(section) else {
            if self.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                self.lines.push(String::new());
            }
            self.lines.push(format!("[{section}]"));
            self.lines.push(entry);
            return;
        };
        let found = self.matches(range.clone(), key);
        if let Some((&first, rest)) = found.split_first() {
            self.lines[first] = entry;
            for &n in rest.iter().rev() {
                self.lines.remove(n);
            }
            return;
        }
        let floor = start.map_or(0, |s| s + 1);
        let mut at = range.end;
        while at > floor && self.lines[at - 1].trim().is_empty() {
            at -= 1;
        }
        self.lines.insert(at, entry);
    }

    /// Removes every active `key` line in `[section]`. True when any was.
    pub fn remove(&mut self, section: &str, key: &str) -> bool {
        let Some((_, range)) = self.section(section) else {
            return false;
        };
        let found = self.matches(range, key);
        for &n in found.iter().rev() {
            self.lines.remove(n);
        }
        !found.is_empty()
    }

    /// Every active `(section, key)` pair, in file order.
    pub fn section_keys(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut section = String::new();
        for line in &self.lines {
            if let Some(h) = header(line) {
                section = h.to_owned();
            } else if let Some(key) = key_of(line) {
                out.push((section.clone(), key.to_owned()));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "# OutRun2006Tweaks\n\n[Performance]\n# a comment\nFramerateLimit = 60\n\n[Controls]\nSteeringDeadZone = 0.2\nVibrationMode=0\n\n[Graphics]\nDrawDistanceIncrease = 0\n\n[CDTracks]\n01_Splash_wave.ogg = Splash Wave\nLast_Wave.ogg = Last Wave\nor2ed1.ogg = ending\n";

    fn set(text: &str, section: &str, key: &str, value: &str) -> String {
        let mut ini = Ini::parse(text);
        ini.set(section, key, value);
        ini.to_text()
    }

    fn tight(text: &str, section: &str, key: &str, value: &str) -> String {
        let mut ini = Ini::parse(text).tight();
        ini.set(section, key, value);
        ini.to_text()
    }

    fn remove(text: &str, section: &str, key: &str) -> String {
        let mut ini = Ini::parse(text);
        ini.remove(section, key);
        ini.to_text()
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
            ini.to_text(),
            "Width = 1920\nHeight=480\nFullscreen = 1\n\n[Other]\nWidth = 1\n"
        );
        let mut empty = Ini::parse("");
        empty.set("", "a", "b");
        assert_eq!(empty.to_text(), "a = b\n");
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
        // Valid UTF-8 survives byte for byte too.
        let utf8 = "[GameController]\nName=Manette étendue\n".as_bytes();
        let mut ini = Ini::from_bytes(utf8);
        ini.set("Graphics", "VSync", "1");
        assert!(ini.to_bytes().starts_with(utf8));
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
        assert_eq!(text.to_text(), "\u{feff}[Graphics]\nVSync = 2\n");
    }
}
