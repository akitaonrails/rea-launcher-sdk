//! Portable Executable headers and sections, bounds-checked.
//!
//! Enough of the format to place a patch: the image base, the section
//! table, and conversions between virtual addresses and file offsets. SRR
//! hard-coded `file offset = VA - 0x400000` for `.text` only; Screamer's LE
//! images (DOS/4GW) have their own parser in that launcher. PE32 (the
//! launchers' 32-bit games) and PE32+ are both read. Every read is checked;
//! an error names the offset it could not read. [`edit`] adds sections and
//! imports to a runtime copy.

pub mod edit;

/// Why a file could not be read as a PE image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// No `MZ` at the start.
    NotMz,
    /// No `PE\0\0` where `e_lfanew` points.
    NoPeSignature { offset: usize },
    /// The optional header's magic is neither PE32 (0x10b) nor PE32+ (0x20b).
    UnknownMagic { magic: u16, offset: usize },
    /// `what` runs past the end of the file.
    Truncated { what: &'static str, offset: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotMz => f.write_str("not a Windows executable (no MZ header)"),
            Error::NoPeSignature { offset } => {
                write!(
                    f,
                    "not a Windows executable (no PE signature at {offset:#x})"
                )
            }
            Error::UnknownMagic { magic, offset } => {
                write!(
                    f,
                    "unknown optional header magic {magic:#06x} at {offset:#x}"
                )
            }
            Error::Truncated { what, offset } => {
                write!(f, "the executable is truncated: {what} at {offset:#x}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// PE32 (32-bit) or PE32+ (64-bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pe32,
    Pe32Plus,
}

/// `IMAGE_SCN_CNT_CODE`.
pub const SCN_CODE: u32 = 0x0000_0020;
/// `IMAGE_SCN_MEM_EXECUTE`.
pub const SCN_EXECUTE: u32 = 0x2000_0000;

/// One section header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// Up to 8 bytes, NULs dropped (`.text`).
    pub name: String,
    /// Relative virtual address.
    pub rva: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub characteristics: u32,
}

impl Section {
    pub fn is_executable(&self) -> bool {
        self.characteristics & (SCN_CODE | SCN_EXECUTE) != 0
    }

    /// The file bytes this section occupies.
    pub fn raw_range(&self) -> std::ops::Range<usize> {
        let start = self.raw_offset as usize;
        start..start + self.raw_size as usize
    }
}

/// A parsed image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pe {
    pub format: Format,
    pub image_base: u64,
    /// Entry point, relative.
    pub entry_rva: u32,
    pub sections: Vec<Section>,
}

fn u16_at(bytes: &[u8], at: usize, what: &'static str) -> Result<u16, Error> {
    let raw = bytes
        .get(at..at.saturating_add(2))
        .ok_or(Error::Truncated { what, offset: at })?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn u32_at(bytes: &[u8], at: usize, what: &'static str) -> Result<u32, Error> {
    let raw = bytes
        .get(at..at.saturating_add(4))
        .ok_or(Error::Truncated { what, offset: at })?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn u64_at(bytes: &[u8], at: usize, what: &'static str) -> Result<u64, Error> {
    let lo = u32_at(bytes, at, what)?;
    let hi = u32_at(bytes, at + 4, what)?;
    Ok(u64::from(lo) | (u64::from(hi) << 32))
}

/// The PE signature's offset and the format, reading only the headers
/// needed to tell them (what [`crate::identify::validate_pe32`] checks).
pub fn format(bytes: &[u8]) -> Result<(usize, Format), Error> {
    if bytes.get(0..2) != Some(b"MZ") {
        return Err(Error::NotMz);
    }
    let pe = u32_at(bytes, 0x3c, "e_lfanew")? as usize;
    if bytes.get(pe..pe.saturating_add(4)) != Some(b"PE\0\0") {
        return Err(Error::NoPeSignature { offset: pe });
    }
    let at = pe + 24;
    match u16_at(bytes, at, "optional header magic")? {
        0x10b => Ok((pe, Format::Pe32)),
        0x20b => Ok((pe, Format::Pe32Plus)),
        magic => Err(Error::UnknownMagic { magic, offset: at }),
    }
}

impl Pe {
    /// Parses the headers and the section table.
    pub fn parse(bytes: &[u8]) -> Result<Pe, Error> {
        let (pe, format) = format(bytes)?;
        let sections = u16_at(bytes, pe + 6, "section count")? as usize;
        let optional_size = u16_at(bytes, pe + 20, "optional header size")? as usize;
        let optional = pe + 24;
        let entry_rva = u32_at(bytes, optional + 16, "entry point")?;
        let image_base = match format {
            Format::Pe32 => u64::from(u32_at(bytes, optional + 28, "image base")?),
            Format::Pe32Plus => u64_at(bytes, optional + 24, "image base")?,
        };
        let table = optional + optional_size;
        let sections = (0..sections)
            .map(|i| {
                let at = table + i * 40;
                let name = bytes
                    .get(at..at + 8)
                    .ok_or(Error::Truncated {
                        what: "section name",
                        offset: at,
                    })?
                    .iter()
                    .take_while(|&&b| b != 0)
                    .map(|&b| char::from(b))
                    .collect();
                Ok(Section {
                    name,
                    virtual_size: u32_at(bytes, at + 8, "section size")?,
                    rva: u32_at(bytes, at + 12, "section address")?,
                    raw_size: u32_at(bytes, at + 16, "section raw size")?,
                    raw_offset: u32_at(bytes, at + 20, "section raw offset")?,
                    characteristics: u32_at(bytes, at + 36, "section flags")?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(Pe {
            format,
            image_base,
            entry_rva,
            sections,
        })
    }

    /// The first section called `name`.
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The section whose file bytes hold `offset`.
    pub fn section_at_offset(&self, offset: usize) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| s.raw_range().contains(&offset))
    }

    /// The file offset of virtual address `va` (image base included), when
    /// a section has file bytes there (not the zero-filled tail).
    pub fn va_to_offset(&self, va: u64) -> Option<usize> {
        let rva = u32::try_from(va.checked_sub(self.image_base)?).ok()?;
        self.sections.iter().find_map(|s| {
            let into = rva.checked_sub(s.rva)?;
            (into < s.raw_size && into < s.virtual_size.max(s.raw_size))
                .then(|| s.raw_offset as usize + into as usize)
        })
    }

    /// The virtual address (image base included) of file `offset`, when a
    /// section maps it.
    pub fn offset_to_va(&self, offset: usize) -> Option<u64> {
        let s = self.section_at_offset(offset)?;
        Some(self.image_base + u64::from(s.rva) + (offset - s.raw_offset as usize) as u64)
    }

    /// True when `offset` lies in an executable section's file bytes.
    pub fn is_executable(&self, offset: usize) -> bool {
        self.section_at_offset(offset)
            .is_some_and(Section::is_executable)
    }
}

/// Builds small synthetic images for tests (launchers' too): an MZ stub,
/// PE headers and the given sections, each `(name, rva, raw bytes,
/// characteristics)`, laid out from file offset 0x200.
pub mod synthetic {
    use super::{Format, SCN_CODE, SCN_EXECUTE};

    /// Read and execute code: `.text`'s usual flags.
    pub const CODE: u32 = SCN_CODE | SCN_EXECUTE | 0x4000_0000;
    /// Initialised read-only data: `.rdata`'s usual flags.
    pub const DATA: u32 = 0x0000_0040 | 0x4000_0000;

    /// An image with `image_base` and `sections`; section raw data is
    /// placed one after another from 0x200, 0x200-aligned.
    pub fn image(format: Format, image_base: u64, sections: &[(&str, u32, &[u8], u32)]) -> Vec<u8> {
        let pe = 0x80usize;
        let optional_size: usize = match format {
            Format::Pe32 => 224,
            Format::Pe32Plus => 240,
        };
        let table = pe + 24 + optional_size;
        let mut out = vec![0u8; 0x200];
        out[0..2].copy_from_slice(b"MZ");
        out[0x3c..0x40].copy_from_slice(&(pe as u32).to_le_bytes());
        out[pe..pe + 4].copy_from_slice(b"PE\0\0");
        let machine: u16 = match format {
            Format::Pe32 => 0x14c,
            Format::Pe32Plus => 0x8664,
        };
        out[pe + 4..pe + 6].copy_from_slice(&machine.to_le_bytes());
        out[pe + 6..pe + 8].copy_from_slice(&(sections.len() as u16).to_le_bytes());
        out[pe + 20..pe + 22].copy_from_slice(&(optional_size as u16).to_le_bytes());
        let o = pe + 24;
        match format {
            Format::Pe32 => {
                out[o..o + 2].copy_from_slice(&0x10bu16.to_le_bytes());
                out[o + 28..o + 32].copy_from_slice(&(image_base as u32).to_le_bytes());
            }
            Format::Pe32Plus => {
                out[o..o + 2].copy_from_slice(&0x20bu16.to_le_bytes());
                out[o + 24..o + 32].copy_from_slice(&image_base.to_le_bytes());
            }
        }
        let first_rva = sections.first().map_or(0x1000, |s| s.1);
        out[o + 16..o + 20].copy_from_slice(&first_rva.to_le_bytes());
        // Alignments, sizes and the 16 data directories, as linkers write.
        out[o + 32..o + 36].copy_from_slice(&0x1000u32.to_le_bytes());
        out[o + 36..o + 40].copy_from_slice(&0x200u32.to_le_bytes());
        let end = sections
            .iter()
            .map(|s| s.1 + (s.2.len() as u32).max(1))
            .max()
            .unwrap_or(0x1000);
        out[o + 56..o + 60]
            .copy_from_slice(&end.div_ceil(0x1000).saturating_mul(0x1000).to_le_bytes());
        out[o + 60..o + 64].copy_from_slice(&0x200u32.to_le_bytes());
        let count_at = match format {
            Format::Pe32 => o + 92,
            Format::Pe32Plus => o + 108,
        };
        out[count_at..count_at + 4].copy_from_slice(&16u32.to_le_bytes());
        let mut raw = 0x200usize;
        for (i, (name, rva, data, flags)) in sections.iter().enumerate() {
            let at = table + i * 40;
            assert!(at + 40 <= 0x200, "too many sections for the stub");
            let mut padded = [0u8; 8];
            padded[..name.len().min(8)].copy_from_slice(&name.as_bytes()[..name.len().min(8)]);
            out[at..at + 8].copy_from_slice(&padded);
            let size = (data.len() as u32).div_ceil(0x200) * 0x200;
            out[at + 8..at + 12].copy_from_slice(&(data.len() as u32).to_le_bytes());
            out[at + 12..at + 16].copy_from_slice(&rva.to_le_bytes());
            out[at + 16..at + 20].copy_from_slice(&size.to_le_bytes());
            out[at + 20..at + 24].copy_from_slice(&(raw as u32).to_le_bytes());
            out[at + 36..at + 40].copy_from_slice(&flags.to_le_bytes());
            out.resize(raw + size as usize, 0);
            out[raw..raw + data.len()].copy_from_slice(data);
            raw += size as usize;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::synthetic::{CODE, DATA, image};
    use super::*;

    fn sample() -> Vec<u8> {
        image(
            Format::Pe32,
            0x40_0000,
            &[
                (".text", 0x1000, &[0x55, 0x8b, 0xec, 0xc3], CODE),
                (".rdata", 0x2000, &[1, 2, 3, 4], DATA),
            ],
        )
    }

    #[test]
    fn headers_and_sections_are_read() {
        let pe = Pe::parse(&sample()).unwrap();
        assert_eq!(pe.format, Format::Pe32);
        assert_eq!(pe.image_base, 0x40_0000);
        assert_eq!(pe.entry_rva, 0x1000);
        let names: Vec<_> = pe.sections.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [".text", ".rdata"]);
        let text = pe.section(".text").unwrap();
        assert_eq!(
            (text.raw_offset, text.raw_size, text.virtual_size),
            (0x200, 0x200, 4)
        );
        assert!(text.is_executable());
        assert!(!pe.section(".rdata").unwrap().is_executable());
        assert!(pe.section(".bss").is_none());
    }

    #[test]
    fn addresses_and_offsets_convert_both_ways() {
        let bytes = sample();
        let pe = Pe::parse(&bytes).unwrap();
        assert_eq!(pe.va_to_offset(0x40_1002), Some(0x202));
        assert_eq!(bytes[0x202], 0xec);
        assert_eq!(pe.offset_to_va(0x202), Some(0x40_1002));
        assert_eq!(pe.va_to_offset(0x40_2000), Some(0x400));
        assert_eq!(pe.offset_to_va(0x400), Some(0x40_2000));
        assert_eq!(pe.va_to_offset(0x3f_ffff), None, "below the image");
        assert_eq!(pe.va_to_offset(0x40_0000), None, "the headers");
        assert_eq!(pe.va_to_offset(0x40_3000), None, "no section there");
        assert_eq!(pe.offset_to_va(0x10), None);
        assert!(pe.is_executable(0x203));
        assert!(!pe.is_executable(0x400));
        assert!(!pe.is_executable(0x10));
    }

    #[test]
    fn pe32_plus_is_read_too() {
        let bytes = image(
            Format::Pe32Plus,
            0x1_4000_0000,
            &[(".text", 0x1000, &[0x90], CODE)],
        );
        let pe = Pe::parse(&bytes).unwrap();
        assert_eq!(pe.format, Format::Pe32Plus);
        assert_eq!(pe.image_base, 0x1_4000_0000);
        assert_eq!(pe.va_to_offset(0x1_4000_1000), Some(0x200));
    }

    #[test]
    fn errors_name_what_and_where() {
        assert_eq!(Pe::parse(b"hello"), Err(Error::NotMz));
        let mut bytes = sample();
        bytes[0x80] = b'X';
        assert_eq!(
            Pe::parse(&bytes),
            Err(Error::NoPeSignature { offset: 0x80 })
        );
        let mut bytes = sample();
        bytes[0x98] = 0x07;
        assert!(matches!(
            Pe::parse(&bytes),
            Err(Error::UnknownMagic { offset: 0x98, .. })
        ));
        let mut bytes = sample();
        bytes[0x86] = 0xff; // 255 sections: the table runs out
        let err = Pe::parse(&bytes).unwrap_err();
        assert!(
            matches!(
                err,
                Error::Truncated {
                    what: "section name",
                    ..
                }
            ),
            "{err:?}"
        );
        let short = &sample()[..0x90];
        assert_eq!(
            Pe::parse(short),
            Err(Error::Truncated {
                what: "optional header magic",
                offset: 0x98
            })
        );
        assert!(err.to_string().contains("truncated"));
        let mut far = sample();
        far[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(Pe::parse(&far), Err(Error::NoPeSignature { .. })));
    }
}
