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

use crate::identify::{hex, sha256};

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
}
