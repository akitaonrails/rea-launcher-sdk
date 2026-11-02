//! Byte patches on a private copy of an executable.
//!
//! - [`apply`] is SCR's `prepare`/`apply_to_image`: every site must hold
//!   its expected bytes or already the patched ones, checked before
//!   anything is written, so a launch is idempotent and an unknown build
//!   is refused whole.
//! - [`signature_toggle`] is CMR4's `patch.rs`: a pattern without a fixed
//!   offset must occur exactly once (original or patched), else nothing
//!   changes and the error carries the file's digest.
//! - [`check_caves`] is SRR's: code caves must be zero-filled, inside
//!   their section, and not overlap.
//! - [`verify_sha256`] is SRR 8c7cbf3: a patched result is accepted only
//!   when it hashes to the known patched build.
//! - [`PatchSet`] and [`apply_sets`] put all of that together as one
//!   declarative table per game (the binary-patching playbook): sites by
//!   virtual address or file offset, caves, the builds a set is for and
//!   the hash each build has after it; [`describe`] turns a set into the
//!   offset tables docs and PCGamingWiki show, so they are never retyped.
//!   [`testing`] has the golden and real-game checks.
//!
//! ## Combinations
//!
//! Sets are independent: no two sites of any sets passed together may
//! share a byte, and a cave may only be written by its own set's sites.
//! Each set's `results` hash is therefore the hash of the build with that
//! set alone applied, and [`apply_sets`] checks every enabled set that way
//! (on the pristine bytes) before combining them. Because the sets touch
//! disjoint bytes, the combination is exactly their union.

use crate::identify::{hex, sha256};
use crate::pe::Pe;

pub mod testing;

/// One patch site at a file offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site<'a> {
    /// For messages ("pacer spawn hook").
    pub name: &'a str,
    pub offset: usize,
    /// The original bytes; empty skips the check (a site that normalises
    /// whatever an earlier experiment left, verified by hash afterwards).
    pub expected: &'a [u8],
    pub patched: &'a [u8],
}

/// Why a patch was refused. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// A site's `expected` and `patched` bytes differ in length: a mistake
    /// in the launcher's site table, not in the player's file.
    LengthMismatch {
        name: String,
        expected: usize,
        patched: usize,
    },
    /// A site lies past the end of the file.
    Truncated {
        name: String,
        offset: usize,
    },
    /// A site holds neither the expected nor the patched bytes.
    Unexpected {
        name: String,
        offset: usize,
        found: Vec<u8>,
    },
    /// A signature occurs `original` + `patched` times instead of once.
    Signature {
        original: usize,
        patched: usize,
        sha256: [u8; 32],
    },
    CaveNotZero {
        name: String,
        offset: usize,
    },
    CaveOutside {
        name: String,
        offset: usize,
    },
    CavesOverlap {
        first: String,
        second: String,
    },
    /// The result is not the known patched build.
    Unverified {
        sha256: [u8; 32],
    },
    /// The file is not one of the builds the patch sets were made for.
    UnknownEdition {
        sha256: [u8; 32],
    },
    /// This set was not made for the file's build.
    NotForEdition {
        set: String,
        sha256: [u8; 32],
    },
    /// A virtual address needs the PE headers, which could not be read.
    Pe(crate::pe::Error),
    /// A virtual address no section maps to file bytes.
    Unmapped {
        name: String,
        va: u32,
    },
    /// Two sites (or two caves, or a cave and another set's site) share
    /// bytes.
    Overlap {
        first: String,
        second: String,
    },
    /// A cave lies outside the executable sections.
    CaveNotExecutable {
        name: String,
        offset: usize,
    },
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::LengthMismatch {
                name,
                expected,
                patched,
            } => write!(
                f,
                "{name}: the site table is wrong ({expected} expected bytes, {patched} patched)"
            ),
            PatchError::Truncated { name, offset } => {
                write!(f, "{name}: offset {offset:#x} is past the end of the file")
            }
            PatchError::Unexpected {
                name,
                offset,
                found,
            } => write!(
                f,
                "{name}: unexpected bytes at {offset:#x} ({}); not a supported build",
                hex(found)
            ),
            PatchError::Signature {
                original: 0,
                patched: 0,
                sha256,
            } => write!(
                f,
                "the patch signature is missing; SHA-256: {}",
                hex(sha256)
            ),
            PatchError::Signature {
                original,
                patched,
                sha256,
            } => write!(
                f,
                "the patch signature is ambiguous ({original} original, {patched} patched); SHA-256: {}",
                hex(sha256)
            ),
            PatchError::CaveNotZero { name, offset } => {
                write!(f, "{name}: cave space at {offset:#x} is not zero-filled")
            }
            PatchError::CaveOutside { name, offset } => {
                write!(f, "{name}: cave at {offset:#x} runs past its section")
            }
            PatchError::CavesOverlap { first, second } => {
                write!(f, "caves {first} and {second} overlap")
            }
            PatchError::UnknownEdition { sha256 } => write!(
                f,
                "not a build the fixes were made for; SHA-256: {}",
                hex(sha256)
            ),
            PatchError::NotForEdition { set, sha256 } => write!(
                f,
                "{set} was not made for this build; SHA-256: {}",
                hex(sha256)
            ),
            PatchError::Pe(e) => write!(f, "{e}"),
            PatchError::Unmapped { name, va } => {
                write!(f, "{name}: address {va:#x} is not in the file")
            }
            PatchError::Overlap { first, second } => write!(f, "{first} and {second} overlap"),
            PatchError::CaveNotExecutable { name, offset } => {
                write!(
                    f,
                    "{name}: cave at {offset:#x} is not in an executable section"
                )
            }
            PatchError::Unverified { sha256 } => write!(
                f,
                "the patched result (SHA-256 {}) is not the known patched build; nothing was written",
                hex(sha256)
            ),
        }
    }
}

impl std::error::Error for PatchError {}

fn slice<'d>(
    data: &'d [u8],
    name: &str,
    offset: usize,
    len: usize,
) -> Result<&'d [u8], PatchError> {
    offset
        .checked_add(len)
        .and_then(|end| data.get(offset..end))
        .ok_or_else(|| PatchError::Truncated {
            name: name.to_owned(),
            offset,
        })
}

/// Checks every site, then writes them all. Returns how many sites
/// changed (0: already fully patched). On error `data` is unchanged.
pub fn apply(data: &mut [u8], sites: &[Site]) -> Result<usize, PatchError> {
    for site in sites {
        if !site.expected.is_empty() && site.expected.len() != site.patched.len() {
            return Err(PatchError::LengthMismatch {
                name: site.name.to_owned(),
                expected: site.expected.len(),
                patched: site.patched.len(),
            });
        }
        let found = slice(data, site.name, site.offset, site.patched.len())?;
        if !site.expected.is_empty() && found != site.expected && found != site.patched {
            return Err(PatchError::Unexpected {
                name: site.name.to_owned(),
                offset: site.offset,
                found: found.to_vec(),
            });
        }
    }
    let mut changed = 0;
    for site in sites {
        let target = &mut data[site.offset..site.offset + site.patched.len()];
        if target != site.patched {
            target.copy_from_slice(site.patched);
            changed += 1;
        }
    }
    Ok(changed)
}

/// What [`signature_toggle`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Patched,
    AlreadyPatched,
    Restored,
    AlreadyOriginal,
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(at, window)| (window == needle).then_some(at))
        .collect()
}

/// Switches the one occurrence of `original`/`patched` in `data` to
/// `patched` (`enable`) or back. Exactly one occurrence of either must
/// exist; both already-done cases succeed.
pub fn signature_toggle(
    data: &mut [u8],
    original: &[u8],
    patched: &[u8],
    enable: bool,
) -> Result<Toggle, PatchError> {
    let o = occurrences(data, original);
    let p = occurrences(data, patched);
    let mut write = |at: usize, bytes: &[u8]| data[at..at + bytes.len()].copy_from_slice(bytes);
    match (enable, o.as_slice(), p.as_slice()) {
        (true, &[at], []) => {
            write(at, patched);
            Ok(Toggle::Patched)
        }
        (true, [], [_]) => Ok(Toggle::AlreadyPatched),
        (false, [], &[at]) => {
            write(at, original);
            Ok(Toggle::Restored)
        }
        (false, [_], []) => Ok(Toggle::AlreadyOriginal),
        _ => Err(PatchError::Signature {
            original: o.len(),
            patched: p.len(),
            sha256: sha256(data),
        }),
    }
}

/// A code cave: where new code goes in otherwise unused section space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cave<'a> {
    pub name: &'a str,
    pub offset: usize,
    pub len: usize,
}

/// Checks caves before writing them: each ends at or before `limit` (the
/// end of the section's raw data), is zero-filled now, and none overlap.
pub fn check_caves(data: &[u8], caves: &[Cave], limit: usize) -> Result<(), PatchError> {
    let mut spans = Vec::with_capacity(caves.len());
    for cave in caves {
        let end = cave.offset.saturating_add(cave.len);
        if end > limit {
            return Err(PatchError::CaveOutside {
                name: cave.name.to_owned(),
                offset: cave.offset,
            });
        }
        if slice(data, cave.name, cave.offset, cave.len)?
            .iter()
            .any(|&b| b != 0)
        {
            return Err(PatchError::CaveNotZero {
                name: cave.name.to_owned(),
                offset: cave.offset,
            });
        }
        spans.push((cave.offset, end, cave.name));
    }
    spans.sort_unstable();
    for pair in spans.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(PatchError::CavesOverlap {
                first: pair[0].2.to_owned(),
                second: pair[1].2.to_owned(),
            });
        }
    }
    Ok(())
}

/// Accepts `data` only when it hashes to `expected`.
pub fn verify_sha256(data: &[u8], expected: &[u8; 32]) -> Result<(), PatchError> {
    let sha256 = sha256(data);
    if &sha256 == expected {
        Ok(())
    } else {
        Err(PatchError::Unverified { sha256 })
    }
}

/// Where a site is: a virtual address (image base included, resolved
/// through the PE section table) or a file offset (non-PE images).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    Va(u32),
    Offset(usize),
}

/// One change: `original` bytes become `patched` at `at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchSite {
    pub at: At,
    pub original: &'static [u8],
    pub patched: &'static [u8],
    /// What this site does, for describe() tables and errors.
    pub note: &'static str,
}

/// Room for new code: `len` bytes at `at` that must be zero in the
/// pristine build and inside an executable section (when the file is PE).
/// The set's own sites write into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchCave {
    pub name: &'static str,
    pub at: At,
    pub len: usize,
}

/// One fix: what it does, which builds it is for, its sites and caves,
/// and each build's SHA-256 with this set alone applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchSet {
    /// Short id (`pacer`, `widescreen`).
    pub name: &'static str,
    /// What the player sees change, in plain words.
    pub summary: &'static str,
    /// SHA-256 of each pristine build it applies to.
    pub editions: &'static [[u8; 32]],
    pub sites: &'static [PatchSite],
    pub caves: &'static [PatchCave],
    /// `(pristine build, expected SHA-256 after this set alone)`.
    pub results: &'static [([u8; 32], [u8; 32])],
}

fn site_name(set: &PatchSet, i: usize) -> String {
    match set.sites[i].note {
        "" => format!("{} site {}", set.name, i + 1),
        note => format!("{}: {note}", set.name),
    }
}

fn resolve(at: At, pe: Option<&Pe>, name: &str) -> Result<usize, PatchError> {
    match at {
        At::Offset(offset) => Ok(offset),
        At::Va(va) => pe
            .ok_or_else(|| PatchError::Unmapped {
                name: name.to_owned(),
                va,
            })?
            .va_to_offset(u64::from(va))
            .ok_or_else(|| PatchError::Unmapped {
                name: name.to_owned(),
                va,
            }),
    }
}

/// A resolved site: its set, offset, bytes and name.
struct Write {
    set: usize,
    offset: usize,
    original: &'static [u8],
    patched: &'static [u8],
    name: String,
}

/// What the checks found: the pristine bytes, their hash, the writes.
pub(crate) struct Prepared {
    pub(crate) pristine: Vec<u8>,
    pub(crate) edition: [u8; 32],
    writes: Vec<Write>,
}

impl Prepared {
    /// The pristine bytes with the writes of the sets `keep` accepts.
    pub(crate) fn with(&self, keep: impl Fn(usize) -> bool) -> Vec<u8> {
        let mut out = self.pristine.clone();
        for w in self.writes.iter().filter(|w| keep(w.set)) {
            out[w.offset..w.offset + w.patched.len()].copy_from_slice(w.patched);
        }
        out
    }
}

fn overlaps(a: (usize, usize), b: (usize, usize)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Every check of [`apply_sets`] but the build's identity and result
/// hashes when `check_builds` is false (golden tests on synthetic files).
pub(crate) fn prepare(
    input: &[u8],
    sets: &[&PatchSet],
    check_builds: bool,
) -> Result<Prepared, PatchError> {
    let parsed = Pe::parse(input);
    let pe = parsed.as_ref().ok();
    let needs_pe = sets.iter().any(|s| {
        s.sites.iter().any(|x| matches!(x.at, At::Va(_)))
            || s.caves.iter().any(|c| matches!(c.at, At::Va(_)))
    });
    if needs_pe && let Err(e) = &parsed {
        return Err(PatchError::Pe(*e));
    }
    // Resolve and check every site against the file as it is.
    let mut writes = Vec::new();
    for (si, set) in sets.iter().enumerate() {
        for (i, site) in set.sites.iter().enumerate() {
            let name = site_name(set, i);
            if site.original.len() != site.patched.len() {
                return Err(PatchError::LengthMismatch {
                    name,
                    expected: site.original.len(),
                    patched: site.patched.len(),
                });
            }
            let offset = resolve(site.at, pe, &name)?;
            let found = slice(input, &name, offset, site.patched.len())?;
            if found != site.original && found != site.patched {
                return Err(PatchError::Unexpected {
                    name,
                    offset,
                    found: found.to_vec(),
                });
            }
            writes.push(Write {
                set: si,
                offset,
                original: site.original,
                patched: site.patched,
                name,
            });
        }
    }
    for (i, a) in writes.iter().enumerate() {
        for b in &writes[i + 1..] {
            let span = |w: &Write| (w.offset, w.offset + w.patched.len());
            if overlaps(span(a), span(b)) {
                return Err(PatchError::Overlap {
                    first: a.name.clone(),
                    second: b.name.clone(),
                });
            }
        }
    }
    // The pristine build: every site back to its original bytes.
    let mut pristine = input.to_vec();
    for w in &writes {
        pristine[w.offset..w.offset + w.original.len()].copy_from_slice(w.original);
    }
    let edition = sha256(&pristine);
    if check_builds {
        if !sets.iter().any(|s| s.editions.contains(&edition)) && !sets.is_empty() {
            return Err(PatchError::UnknownEdition { sha256: edition });
        }
        if let Some(set) = sets.iter().find(|s| !s.editions.contains(&edition)) {
            return Err(PatchError::NotForEdition {
                set: set.name.to_owned(),
                sha256: edition,
            });
        }
    }
    // Caves: zero in the pristine build, executable, apart, and written
    // only by their own set.
    let mut caves: Vec<(usize, usize, usize, String)> = Vec::new();
    for (si, set) in sets.iter().enumerate() {
        for cave in set.caves {
            let name = format!("{}: cave {}", set.name, cave.name);
            let offset = resolve(cave.at, pe, &name)?;
            let bytes = slice(&pristine, &name, offset, cave.len)?;
            if bytes.iter().any(|&b| b != 0) {
                return Err(PatchError::CaveNotZero { name, offset });
            }
            if let Some(pe) = pe
                && !(cave.len == 0
                    || (pe.is_executable(offset) && pe.is_executable(offset + cave.len - 1)))
            {
                return Err(PatchError::CaveNotExecutable { name, offset });
            }
            for (start, end, _, other) in &caves {
                if overlaps((offset, offset + cave.len), (*start, *end)) {
                    return Err(PatchError::CavesOverlap {
                        first: other.clone(),
                        second: name,
                    });
                }
            }
            if let Some(w) = writes.iter().find(|w| {
                w.set != si
                    && overlaps(
                        (offset, offset + cave.len),
                        (w.offset, w.offset + w.patched.len()),
                    )
            }) {
                return Err(PatchError::Overlap {
                    first: name,
                    second: w.name.clone(),
                });
            }
            caves.push((offset, offset + cave.len, si, name));
        }
    }
    Ok(Prepared {
        pristine,
        edition,
        writes,
    })
}

/// Applies `sets` to an executable's bytes and returns the patched file.
///
/// The bytes may be pristine or already carry some of these sets (each
/// site holds its original or its patched bytes); the pristine build is
/// rebuilt from the site tables, identified by SHA-256, and refused when a
/// set was not made for it. VAs go through the PE section table. Sites
/// may not share bytes; caves must be zero-filled and executable. Each
/// set with a `results` entry for the build is verified alone (see the
/// module's "Combinations"). All or nothing: on any error nothing is
/// returned. No sets: the pristine bytes.
pub fn apply_sets(input: &[u8], sets: &[&PatchSet]) -> Result<Vec<u8>, PatchError> {
    let prepared = prepare(input, sets, true)?;
    for (si, set) in sets.iter().enumerate() {
        if let Some((_, expected)) = set.results.iter().find(|(e, _)| *e == prepared.edition) {
            verify_sha256(&prepared.with(|s| s == si), expected)?;
        }
    }
    Ok(prepared.with(|_| true))
}

/// The pristine build from bytes that may carry some of `sets`, checked to
/// be a build they were made for (a fix switched off needs no copy of the
/// original).
pub fn unpatch(input: &[u8], sets: &[&PatchSet]) -> Result<Vec<u8>, PatchError> {
    Ok(prepare(input, sets, true)?.pristine)
}

/// One line of a patch table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub offset: usize,
    /// When the file is PE: the address the game sees.
    pub va: Option<u64>,
    pub original: &'static [u8],
    pub patched: &'static [u8],
    pub note: &'static str,
}

/// The rows of `set` for a build (`pe`: its headers, for VA sites and the
/// VA column; `None` for offset-only, non-PE files).
pub fn describe(set: &PatchSet, pe: Option<&Pe>) -> Result<Vec<Row>, PatchError> {
    set.sites
        .iter()
        .enumerate()
        .map(|(i, site)| {
            let offset = resolve(site.at, pe, &site_name(set, i))?;
            Ok(Row {
                offset,
                va: pe.and_then(|pe| pe.offset_to_va(offset)),
                original: site.original,
                patched: site.patched,
                note: site.note,
            })
        })
        .collect()
}

fn spaced_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A Markdown table of `rows` (a VA column when any row has one).
pub fn to_markdown(rows: &[Row]) -> String {
    let with_va = rows.iter().any(|r| r.va.is_some());
    let mut out = String::from(if with_va {
        "| File offset | VA | Original | Patched | Note |\n|---|---|---|---|---|\n"
    } else {
        "| File offset | Original | Patched | Note |\n|---|---|---|---|\n"
    });
    for r in rows {
        let va = match (with_va, r.va) {
            (false, _) => String::new(),
            (true, Some(va)) => format!(" `0x{va:08X}` |"),
            (true, None) => " |".to_owned(),
        };
        out.push_str(&format!(
            "| `0x{:08X}` |{va} `{}` | `{}` | {} |\n",
            r.offset,
            spaced_hex(r.original),
            spaced_hex(r.patched),
            r.note.replace('|', "\\|")
        ));
    }
    out
}

/// A MediaWiki table of `rows` for a PCGamingWiki fix box: file offset,
/// original and new bytes, notes.
pub fn to_wikitext(rows: &[Row]) -> String {
    let mut out = String::from(
        "{| class=\"wikitable\"\n|-\n! File offset !! Original bytes !! New bytes !! Notes\n",
    );
    for r in rows {
        out.push_str(&format!(
            "|-\n| <code>0x{:08X}</code> || <code>{}</code> || <code>{}</code> || {}\n",
            r.offset,
            spaced_hex(r.original),
            spaced_hex(r.patched),
            r.note.replace("||", "| |")
        ));
    }
    out.push_str("|}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITES: [Site; 2] = [
        Site {
            name: "first",
            offset: 1,
            expected: &[0x55, 0x8b],
            patched: &[0xc3, 0x90],
        },
        Site {
            name: "second",
            offset: 4,
            expected: &[0xec],
            patched: &[0xcc],
        },
    ];

    #[test]
    fn sites_apply_once_and_are_idempotent() {
        let mut data = [0x90, 0x55, 0x8b, 0x00, 0xec, 0x90];
        assert_eq!(apply(&mut data, &SITES), Ok(2));
        assert_eq!(data, [0x90, 0xc3, 0x90, 0x00, 0xcc, 0x90]);
        assert_eq!(apply(&mut data, &SITES), Ok(0));
        // Half-patched (an interrupted older launcher) completes.
        let mut half = [0x90, 0xc3, 0x90, 0x00, 0xec, 0x90];
        assert_eq!(apply(&mut half, &SITES), Ok(1));
    }

    #[test]
    fn unexpected_or_truncated_sites_write_nothing() {
        let mut data = [0x90, 0x55, 0x8b, 0x00, 0x00, 0x90];
        let before = data;
        let err = apply(&mut data, &SITES).unwrap_err();
        assert!(
            matches!(&err, PatchError::Unexpected { name, offset: 4, found } if name == "second" && found == &[0])
        );
        assert!(err.to_string().contains("unexpected bytes at 0x4"));
        assert_eq!(data, before, "all or nothing");
        let mut short = [0x90, 0x55, 0x8b];
        assert!(matches!(
            apply(&mut short, &SITES),
            Err(PatchError::Truncated { offset: 4, .. })
        ));
        // A table mistake is told apart from an unknown build.
        let mut data = [0x55, 0x8b, 0x00];
        let wrong = [Site {
            name: "typo",
            offset: 0,
            expected: &[0x55, 0x8b],
            patched: &[0xc3],
        }];
        let err = apply(&mut data, &wrong).unwrap_err();
        assert!(matches!(
            err,
            PatchError::LengthMismatch {
                expected: 2,
                patched: 1,
                ..
            }
        ));
        assert!(err.to_string().contains("site table"));
        assert_eq!(data, [0x55, 0x8b, 0x00]);
        // An unchecked site writes whatever is there.
        let mut any = [7, 7];
        let normalise = [Site {
            name: "n",
            offset: 0,
            expected: &[],
            patched: &[1, 2],
        }];
        assert_eq!(apply(&mut any, &normalise), Ok(1));
        assert_eq!(any, [1, 2]);
    }

    const ORIGINAL: [u8; 6] = [0x04, 0x00, 0x00, 0x80, 0x3f, 0xe8];
    const WIDESCREEN: [u8; 6] = [0x04, 0x00, 0x00, 0x40, 0x3f, 0xe8];

    #[test]
    fn signatures_patch_and_restore_exactly_once() {
        let mut bytes = [0x90, 0x90, 0x04, 0, 0, 0x80, 0x3f, 0xe8, 0x90];
        let t = |b: &mut [u8], on| signature_toggle(b, &ORIGINAL, &WIDESCREEN, on);
        assert_eq!(t(&mut bytes, true), Ok(Toggle::Patched));
        assert_eq!(&bytes[2..8], &WIDESCREEN);
        assert_eq!(t(&mut bytes, true), Ok(Toggle::AlreadyPatched));
        assert_eq!(t(&mut bytes, false), Ok(Toggle::Restored));
        assert_eq!(&bytes[2..8], &ORIGINAL);
        assert_eq!(t(&mut bytes, false), Ok(Toggle::AlreadyOriginal));
    }

    #[test]
    fn missing_and_ambiguous_signatures_fail_with_a_digest() {
        let t = |b: &mut [u8], on| signature_toggle(b, &ORIGINAL, &WIDESCREEN, on);
        let mut missing = [0; 16];
        let err = t(&mut missing, true).unwrap_err();
        assert!(
            matches!(err, PatchError::Signature { sha256, .. } if sha256 == crate::identify::sha256(&[0; 16]))
        );
        assert!(err.to_string().contains("missing"));
        let mut twice = [ORIGINAL, ORIGINAL].concat();
        let before = twice.clone();
        let err = t(&mut twice, true).unwrap_err();
        assert!(matches!(err, PatchError::Signature { original: 2, .. }));
        assert!(err.to_string().contains("ambiguous"));
        assert_eq!(twice, before, "a failed patch changes nothing");
        let mut mixed = [ORIGINAL, WIDESCREEN].concat();
        assert!(matches!(
            t(&mut mixed, false),
            Err(PatchError::Signature { patched: 1, .. })
        ));
    }

    #[test]
    fn caves_must_be_zero_inside_and_apart() {
        let mut data = vec![0u8; 64];
        let caves = [
            Cave {
                name: "a",
                offset: 0,
                len: 16,
            },
            Cave {
                name: "b",
                offset: 16,
                len: 16,
            },
        ];
        assert_eq!(check_caves(&data, &caves, 32), Ok(()));
        assert!(matches!(
            check_caves(&data, &caves, 31),
            Err(PatchError::CaveOutside { .. })
        ));
        let overlap = [
            caves[0],
            Cave {
                offset: 8,
                ..caves[1]
            },
        ];
        assert!(matches!(
            check_caves(&data, &overlap, 64),
            Err(PatchError::CavesOverlap { .. })
        ));
        data[20] = 1;
        assert!(matches!(
            check_caves(&data, &caves, 64),
            Err(PatchError::CaveNotZero { offset: 16, .. })
        ));
        assert!(matches!(
            check_caves(
                &data,
                &[Cave {
                    offset: 60,
                    ..caves[0]
                }],
                100
            ),
            Err(PatchError::Truncated { .. })
        ));
    }

    #[test]
    fn results_are_verified_by_digest() {
        assert_eq!(verify_sha256(b"a", &sha256(b"a")), Ok(()));
        let err = verify_sha256(b"b", &sha256(b"a")).unwrap_err();
        assert_eq!(
            err,
            PatchError::Unverified {
                sha256: sha256(b"b")
            }
        );
        assert!(err.to_string().contains("nothing was written"));
    }

    mod sets {
        use super::super::*;
        use crate::pe::Format;
        use crate::pe::synthetic::{CODE, DATA, image};

        fn leak<T>(items: Vec<T>) -> &'static [T] {
            Box::leak(items.into_boxed_slice())
        }

        /// `.text` at 0x401000: a hook site and 32 zero bytes of cave at
        /// 0x401010; `.rdata` at 0x402000: a 16:9 aspect constant.
        fn game() -> Vec<u8> {
            let mut text = vec![0x90u8; 0x40];
            text[0..5].copy_from_slice(&[0x51, 0x8d, 0x04, 0x24, 0x50]);
            text[0x10..0x30].fill(0);
            let aspect = (16.0f32 / 9.0).to_le_bytes();
            image(
                Format::Pe32,
                0x40_0000,
                &[
                    (".text", 0x1000, &text, CODE),
                    (".rdata", 0x2000, &aspect, DATA),
                ],
            )
        }

        const HOOK: &[u8] = &[0xe9, 0x0b, 0x00, 0x00, 0x00];
        const CAVE_CODE: &[u8] = &[0xb8, 0x01, 0x00, 0x00, 0x00, 0xc3];
        const WIDE: [u8; 4] = (21.0f32 / 9.0).to_le_bytes();
        const NARROW: [u8; 4] = (16.0f32 / 9.0).to_le_bytes();

        /// The sets for `edition`, with result hashes worked out on it.
        fn sets(edition: [u8; 32]) -> (&'static PatchSet, &'static PatchSet) {
            let editions = leak(vec![edition]);
            let hook = PatchSet {
                name: "pacer",
                summary: "Smooth frame pacing",
                editions,
                sites: leak(vec![
                    PatchSite {
                        at: At::Va(0x40_1000),
                        original: &[0x51, 0x8d, 0x04, 0x24, 0x50],
                        patched: HOOK,
                        note: "jump to the cave",
                    },
                    PatchSite {
                        at: At::Va(0x40_1010),
                        original: &[0; 6],
                        patched: CAVE_CODE,
                        note: "cave code",
                    },
                ]),
                caves: leak(vec![PatchCave {
                    name: "pacer",
                    at: At::Va(0x40_1010),
                    len: 0x20,
                }]),
                results: &[],
            };
            let aspect = PatchSet {
                name: "aspect",
                summary: "21:9 without stretching",
                editions,
                sites: leak(vec![PatchSite {
                    at: At::Offset(0x400),
                    original: &NARROW,
                    patched: &WIDE,
                    note: "aspect constant",
                }]),
                caves: &[],
                results: &[],
            };
            let pristine = game();
            let with = |set: &PatchSet| {
                let mut prepared = prepare(&pristine, &[set], false).unwrap().with(|_| true);
                sha256(std::mem::take(&mut prepared).as_slice())
            };
            let hook = Box::leak(Box::new(PatchSet {
                results: leak(vec![(edition, with(&hook))]),
                ..hook
            }));
            let aspect = Box::leak(Box::new(PatchSet {
                results: leak(vec![(edition, with(&aspect))]),
                ..aspect
            }));
            (hook, aspect)
        }

        #[test]
        fn sets_apply_by_address_and_offset_and_are_verified() {
            let pristine = game();
            let (pacer, aspect) = sets(sha256(&pristine));
            let out = apply_sets(&pristine, &[pacer, aspect]).unwrap();
            assert_eq!(&out[0x200..0x205], HOOK);
            assert_eq!(&out[0x210..0x216], CAVE_CODE);
            assert_eq!(&out[0x400..0x404], &WIDE);
            // The union of the two sets alone.
            let a = apply_sets(&pristine, &[pacer]).unwrap();
            let b = apply_sets(&pristine, &[aspect]).unwrap();
            let union: Vec<u8> = (0..out.len())
                .map(|i| if a[i] != pristine[i] { a[i] } else { b[i] })
                .collect();
            assert_eq!(out, union);
            // Already patched: accepted, and turned back.
            assert_eq!(apply_sets(&out, &[pacer, aspect]).unwrap(), out);
            assert_eq!(
                apply_sets(&a, &[pacer, aspect]).unwrap(),
                out,
                "one set already in"
            );
            assert_eq!(unpatch(&out, &[pacer, aspect]).unwrap(), pristine);
            assert_eq!(
                apply_sets(&out, &[aspect]).unwrap_err(),
                PatchError::UnknownEdition { sha256: sha256(&a) }
            );
        }

        #[test]
        fn unknown_builds_and_wrong_results_are_refused() {
            let pristine = game();
            let (pacer, aspect) = sets(sha256(&pristine));
            let mut other = pristine.clone();
            other[0x230] = 0x42;
            assert_eq!(
                apply_sets(&other, &[pacer]).unwrap_err(),
                PatchError::UnknownEdition {
                    sha256: sha256(&other)
                }
            );
            let foreign = PatchSet {
                editions: &[[7; 32]],
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[pacer, &foreign]),
                Err(PatchError::NotForEdition { set, .. }) if set == "aspect"
            ));
            let wrong = PatchSet {
                results: leak(vec![(sha256(&pristine), [0; 32])]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[&wrong]),
                Err(PatchError::Unverified { .. })
            ));
            assert_eq!(apply_sets(&pristine, &[]).unwrap(), pristine);
        }

        #[test]
        fn one_bad_site_refuses_everything() {
            let mut bytes = game();
            let (pacer, aspect) = sets(sha256(&bytes));
            bytes[0x402] = 0xff;
            let err = apply_sets(&bytes, &[pacer, aspect]).unwrap_err();
            assert!(
                matches!(&err, PatchError::Unexpected { name, offset: 0x400, .. } if name == "aspect: aspect constant")
            );
        }

        #[test]
        fn overlaps_caves_and_addresses_are_checked() {
            let pristine = game();
            let edition = sha256(&pristine);
            let (pacer, aspect) = sets(edition);
            let clash = PatchSet {
                name: "clash",
                sites: leak(vec![PatchSite {
                    at: At::Offset(0x203),
                    original: &[0x24, 0x50],
                    patched: &[0x90, 0x90],
                    note: "",
                }]),
                caves: &[],
                results: &[],
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[pacer, &clash]),
                Err(PatchError::Overlap { second, .. }) if second == "clash site 1"
            ));
            // Another set writing into this set's cave.
            let squatter = PatchSet {
                sites: leak(vec![PatchSite {
                    at: At::Offset(0x220),
                    original: &[0],
                    patched: &[1],
                    note: "squat",
                }]),
                ..clash
            };
            assert!(matches!(
                apply_sets(&pristine, &[pacer, &squatter]),
                Err(PatchError::Overlap { .. })
            ));
            let data_cave = PatchSet {
                caves: leak(vec![PatchCave {
                    name: "data",
                    at: At::Va(0x40_2010),
                    len: 4,
                }]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[&data_cave]),
                Err(PatchError::CaveNotExecutable { .. })
            ));
            let busy_cave = PatchSet {
                caves: leak(vec![PatchCave {
                    name: "busy",
                    at: At::Va(0x40_1000),
                    len: 8,
                }]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[&busy_cave]),
                Err(PatchError::CaveNotZero { .. })
            ));
            let twin = PatchSet {
                sites: &[],
                caves: leak(vec![PatchCave {
                    name: "twin",
                    at: At::Va(0x40_1020),
                    len: 4,
                }]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[pacer, &twin]),
                Err(PatchError::CavesOverlap { .. })
            ));
            let nowhere = PatchSet {
                sites: leak(vec![PatchSite {
                    at: At::Va(0x50_0000),
                    original: &[0],
                    patched: &[1],
                    note: "",
                }]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[&nowhere]),
                Err(PatchError::Unmapped { va: 0x50_0000, .. })
            ));
            assert!(matches!(
                apply_sets(b"not a PE file", &[&nowhere]),
                Err(PatchError::Pe(_))
            ));
            let typo = PatchSet {
                sites: leak(vec![PatchSite {
                    at: At::Offset(0),
                    original: &[1, 2],
                    patched: &[1],
                    note: "",
                }]),
                ..*aspect
            };
            assert!(matches!(
                apply_sets(&pristine, &[&typo]),
                Err(PatchError::LengthMismatch { .. })
            ));
        }

        #[test]
        fn offset_sets_work_without_pe_headers() {
            let le_image = vec![0x55u8, 0x8b, 0xec, 0x90];
            let fix = PatchSet {
                name: "f1",
                summary: "",
                editions: leak(vec![sha256(&le_image)]),
                sites: leak(vec![PatchSite {
                    at: At::Offset(2),
                    original: &[0xec],
                    patched: &[0xc3],
                    note: "return early",
                }]),
                caves: &[],
                results: &[],
            };
            assert_eq!(
                apply_sets(&le_image, &[&fix]).unwrap(),
                [0x55, 0x8b, 0xc3, 0x90]
            );
            let rows = describe(&fix, None).unwrap();
            assert_eq!(rows[0].va, None);
            assert_eq!(
                to_markdown(&rows),
                "| File offset | Original | Patched | Note |\n|---|---|---|---|\n| `0x00000002` | `EC` | `C3` | return early |\n"
            );
            assert!(testing::check_golden(&fix, &le_image).is_ok());
        }

        #[test]
        fn describe_gives_the_tables_docs_and_the_wiki_show() {
            let pristine = game();
            let (pacer, _) = sets(sha256(&pristine));
            let pe = Pe::parse(&pristine).unwrap();
            let rows = describe(pacer, Some(&pe)).unwrap();
            assert_eq!(rows[0].offset, 0x200);
            assert_eq!(rows[0].va, Some(0x40_1000));
            assert_eq!(
                to_markdown(&rows[..1]),
                "| File offset | VA | Original | Patched | Note |\n|---|---|---|---|---|\n| `0x00000200` | `0x00401000` | `51 8D 04 24 50` | `E9 0B 00 00 00` | jump to the cave |\n"
            );
            assert_eq!(
                to_wikitext(&rows[..1]),
                "{| class=\"wikitable\"\n|-\n! File offset !! Original bytes !! New bytes !! Notes\n|-\n| <code>0x00000200</code> || <code>51 8D 04 24 50</code> || <code>E9 0B 00 00 00</code> || jump to the cave\n|}\n"
            );
            assert!(matches!(
                describe(pacer, None),
                Err(PatchError::Unmapped { .. })
            ));
        }

        #[test]
        fn golden_and_real_game_checks() {
            let pristine = game();
            let (pacer, aspect) = sets(sha256(&pristine));
            testing::assert_golden(pacer, &pristine);
            assert_eq!(testing::check_golden(aspect, &pristine), Ok(()));
            let patched = apply_sets(&pristine, &[aspect]).unwrap();
            assert!(
                testing::check_golden(aspect, &patched)
                    .unwrap_err()
                    .contains("already carries")
            );

            let dir = crate::test_dir("patch-real-game");
            assert!(
                testing::real_game_in(&dir, &[pacer])
                    .unwrap_err()
                    .contains("no build")
            );
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::write(dir.join("bin/Game.EXE"), &pristine).unwrap();
            std::fs::write(dir.join("readme.txt"), &pristine).unwrap();
            let lines = testing::real_game_in(&dir, &[pacer, aspect]).unwrap();
            assert_eq!(lines.len(), 1);
            assert!(
                lines[0].starts_with("bin/Game.EXE (") && lines[0].ends_with("): pacer, aspect")
            );
            let broken = PatchSet {
                results: leak(vec![(sha256(&pristine), [0; 32])]),
                ..*aspect
            };
            assert!(
                testing::real_game_in(&dir, &[&broken])
                    .unwrap_err()
                    .contains("Game.EXE")
            );
            assert_eq!(
                testing::real_game("REA_TEST_NO_SUCH_VARIABLE", &[pacer]),
                Ok(Vec::new())
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
