//! Identifying files by SHA-256, and a minimal PE32 check.
//!
//! The table lookup is SCR's and SRR's `edition.rs`: an unknown executable
//! reports its digest instead of a bare "unknown", so a player's bug report
//! names the build. `validate_pe32` is CMR4's `pe.rs`, which rejects a
//! wrong file before any patching is attempted.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// The SHA-256 of a file, read in 1 MiB pieces.
pub fn sha256_file(path: &Path) -> io::Result<[u8; 32]> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(hasher.finalize().into()),
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// Lower-case hex of any bytes.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("hex digit expected"),
    }
}

/// A 64-digit hex digest as bytes, at compile time (panics on bad input,
/// which fails the build in a `const`).
pub const fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    assert!(b.len() == 64, "64 hex digits expected");
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nibble(b[2 * i]) << 4) | nibble(b[2 * i + 1]);
        i += 1;
    }
    out
}

/// A 64-digit hex digest as bytes, at run time.
pub fn parse_hex32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(hex32(s))
}

/// One known file: its digest and what the launcher knows about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known<E> {
    pub sha256: [u8; 32],
    pub value: E,
}

impl<E> Known<E> {
    pub const fn new(sha256_hex: &str, value: E) -> Known<E> {
        Known {
            sha256: hex32(sha256_hex),
            value,
        }
    }
}

/// What `bytes` is per `table`, or its digest when unknown.
pub fn identify<'a, E>(bytes: &[u8], table: &'a [Known<E>]) -> Result<&'a E, [u8; 32]> {
    identify_digest(sha256(bytes), table)
}

/// [`identify`] from a digest already taken.
pub fn identify_digest<E>(digest: [u8; 32], table: &[Known<E>]) -> Result<&E, [u8; 32]> {
    table
        .iter()
        .find(|k| k.sha256 == digest)
        .map(|k| &k.value)
        .ok_or(digest)
}

/// Why a file is not a 32-bit Windows executable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeError {
    NotPe,
    Not32Bit,
    Truncated,
}

impl std::fmt::Display for PeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PeError::NotPe => "not a Windows executable",
            PeError::Not32Bit => "not a 32-bit Windows executable",
            PeError::Truncated => "the executable is truncated",
        })
    }
}

impl std::error::Error for PeError {}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, PeError> {
    let raw = bytes.get(at..at + 2).ok_or(PeError::Truncated)?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, PeError> {
    let raw = bytes.get(at..at + 4).ok_or(PeError::Truncated)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

/// Checks the MZ and PE signatures and the PE32 (not PE32+) optional
/// header magic.
pub fn validate_pe32(bytes: &[u8]) -> Result<(), PeError> {
    if bytes.get(0..2) != Some(b"MZ") {
        return Err(PeError::NotPe);
    }
    let pe = u32_at(bytes, 0x3c)? as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return Err(PeError::NotPe);
    }
    if u16_at(bytes, pe + 24)? != 0x10b {
        return Err(PeError::Not32Bit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn hex_round_trips() {
        let h = "1246afc95463f96598d5fee764fc99c847b6d3e0bf8ca2268bd362c664bb28a3";
        assert_eq!(hex(&hex32(h)), h);
        assert_eq!(parse_hex32(&h.to_ascii_uppercase()), Some(hex32(h)));
        assert_eq!(parse_hex32("abc"), None);
        assert_eq!(parse_hex32(&"g".repeat(64)), None);
    }

    #[test]
    fn sha256_is_wired_up_for_bytes_and_files() {
        assert_eq!(hex(&sha256(b"")), EMPTY);
        let dir = test_dir("identify-file");
        let path = dir.join("big.bin");
        let bytes: Vec<u8> = (0..3_000_000u32).map(|i| i as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(sha256_file(&path).unwrap(), sha256(&bytes));
        assert!(sha256_file(&dir.join("missing")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[derive(Debug, PartialEq)]
    enum Build {
        Retail,
        Patched,
    }

    static TABLE: [Known<Build>; 2] = [
        Known::new(EMPTY, Build::Retail),
        Known::new(
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
            Build::Patched,
        ),
    ];

    #[test]
    fn known_files_are_found_and_unknown_ones_report_their_digest() {
        assert_eq!(identify(b"", &TABLE), Ok(&Build::Retail));
        assert_eq!(identify(b"a", &TABLE), Ok(&Build::Patched));
        assert_eq!(identify(b"b", &TABLE), Err(sha256(b"b")));
        for (i, a) in TABLE.iter().enumerate() {
            for b in &TABLE[i + 1..] {
                assert_ne!(a.sha256, b.sha256);
            }
        }
    }

    fn synthetic_pe() -> Vec<u8> {
        let mut bytes = vec![0; 0x200];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x58..0x5a].copy_from_slice(&0x10bu16.to_le_bytes());
        bytes
    }

    #[test]
    fn accepts_pe32_and_rejects_other_files() {
        assert_eq!(validate_pe32(&synthetic_pe()), Ok(()));
        assert_eq!(validate_pe32(b"hello"), Err(PeError::NotPe));
        let mut pe64 = synthetic_pe();
        pe64[0x58..0x5a].copy_from_slice(&0x20bu16.to_le_bytes());
        assert_eq!(validate_pe32(&pe64), Err(PeError::Not32Bit));
        let mut truncated = synthetic_pe();
        truncated.truncate(0x44);
        assert_eq!(validate_pe32(&truncated), Err(PeError::Truncated));
        let mut far = synthetic_pe();
        far[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(validate_pe32(&far), Err(PeError::NotPe));
    }
}
