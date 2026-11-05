//! Editing a PE image on disk: a new section, a new import.
//!
//! What lets a launcher load its own hook DLL without touching a running
//! game: the game's private copy gets the code it needs in a section of its
//! own, and the hook DLL in its import table, so Windows loads the DLL and
//! fills in its function addresses before the game starts. CMR1 used to
//! start the game suspended and inject the DLL with `VirtualAllocEx`,
//! `WriteProcessMemory` and `CreateRemoteThread`: the pattern antivirus
//! heuristics take for a trojan (a player's report on Quiver, 2026-11-04).
//!
//! Only the runtime copy is ever edited; the player's game folder stays
//! read-only (docs/STANDARDS.md). The edits keep every existing byte in
//! place: new headers go in the free space after the section table, new
//! data at the end of the file.

use super::{Error as PeError, Format, Pe, Section, format, u16_at, u32_at};

/// Initialised data, readable and writable: an import section.
pub const SCN_DATA_RW: u32 = 0x0000_0040 | 0x4000_0000 | 0x8000_0000;
/// Code and data, readable, writable and executable: a fix section that
/// holds both code and the table its import fills.
pub const SCN_CODE_RWX: u32 = 0x0000_0020 | 0x2000_0000 | 0x4000_0000 | 0x8000_0000;

/// `IMAGE_DLLCHARACTERISTICS_DYNAMIC_BASE`.
const DYNAMIC_BASE: u16 = 0x0040;
const DIR_IMPORT: usize = 1;
const DIR_BOUND_IMPORT: usize = 11;

/// Why an image could not be edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The headers could not be read.
    Pe(PeError),
    /// No free space for one more section header before the first section.
    NoRoomForHeader,
    /// A section name longer than 8 bytes, or empty.
    BadSectionName(String),
    /// A DLL or function name that is empty or not ASCII.
    BadName(String),
    /// An import descriptor is bound and has no name table, so it can't be
    /// unbound to make room for the new import.
    BoundWithoutNames { dll_rva: u32 },
    /// The import table's address `rva` is not inside a writable section.
    IatOutsideWritableSection { rva: u32 },
    /// A field points outside the file.
    Outside { what: &'static str, rva: u32 },
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::Pe(e) => e.fmt(f),
            EditError::NoRoomForHeader => {
                f.write_str("no free space for another section header in the executable")
            }
            EditError::BadSectionName(name) => write!(f, "bad section name {name:?}"),
            EditError::BadName(name) => write!(f, "bad import name {name:?}"),
            EditError::BoundWithoutNames { dll_rva } => write!(
                f,
                "a bound import without a name table (name at rva {dll_rva:#x}) can't be unbound"
            ),
            EditError::IatOutsideWritableSection { rva } => {
                write!(
                    f,
                    "the import address table at rva {rva:#x} is not in a writable section"
                )
            }
            EditError::Outside { what, rva } => {
                write!(f, "{what} at rva {rva:#x} is outside the file")
            }
        }
    }
}

impl std::error::Error for EditError {}

impl From<PeError> for EditError {
    fn from(e: PeError) -> Self {
        EditError::Pe(e)
    }
}

/// One DLL in an image's import table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub dll: String,
    /// Functions imported by name, in table order (`#N` for an ordinal).
    pub functions: Vec<String>,
    /// Where the loader writes the function addresses (`FirstThunk`).
    pub iat_rva: u32,
}

/// Where the header fields live in this image.
struct Layout {
    format: Format,
    /// `NumberOfSections`.
    count_at: usize,
    optional: usize,
    /// The data directory array.
    dirs: usize,
    dir_count: usize,
    /// The section table.
    table: usize,
    sections: usize,
    section_align: u32,
    file_align: u32,
}

impl Layout {
    fn read(bytes: &[u8]) -> Result<Layout, EditError> {
        let (pe, format) = format(bytes)?;
        let optional = pe + 24;
        let optional_size = u16_at(bytes, pe + 20, "optional header size")? as usize;
        let (count_field, dirs) = match format {
            Format::Pe32 => (optional + 92, optional + 96),
            Format::Pe32Plus => (optional + 108, optional + 112),
        };
        // Synthetic images (tests) leave alignments at zero: use Windows'
        // defaults then.
        let nonzero = |v: u32, default: u32| if v == 0 { default } else { v };
        Ok(Layout {
            format,
            count_at: pe + 6,
            optional,
            dirs,
            dir_count: u32_at(bytes, count_field, "data directory count")? as usize,
            table: optional + optional_size,
            sections: u16_at(bytes, pe + 6, "section count")? as usize,
            section_align: nonzero(u32_at(bytes, optional + 32, "section alignment")?, 0x1000),
            file_align: nonzero(u32_at(bytes, optional + 36, "file alignment")?, 0x200),
        })
    }

    fn thunk(&self) -> usize {
        match self.format {
            Format::Pe32 => 4,
            Format::Pe32Plus => 8,
        }
    }

    fn dir(&self, bytes: &[u8], index: usize) -> Result<(u32, u32), EditError> {
        if index >= self.dir_count {
            return Ok((0, 0));
        }
        let at = self.dirs + index * 8;
        Ok((
            u32_at(bytes, at, "data directory")?,
            u32_at(bytes, at + 4, "data directory")?,
        ))
    }

    fn set_dir(&self, bytes: &mut [u8], index: usize, rva: u32, size: u32) {
        if index < self.dir_count {
            let at = self.dirs + index * 8;
            put_u32(bytes, at, rva);
            put_u32(bytes, at + 4, size);
        }
    }
}

fn put_u16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn align_up(value: u32, to: u32) -> u32 {
    value.div_ceil(to) * to
}

/// The file offset of `rva`, when a section's file bytes hold it.
fn offset_of(pe: &Pe, rva: u32, what: &'static str) -> Result<usize, EditError> {
    pe.va_to_offset(pe.image_base + u64::from(rva))
        .ok_or(EditError::Outside { what, rva })
}

/// The NUL-terminated ASCII string at `rva`.
fn string_at(bytes: &[u8], pe: &Pe, rva: u32, what: &'static str) -> Result<String, EditError> {
    let at = offset_of(pe, rva, what)?;
    let rest = &bytes[at..];
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or(EditError::Outside { what, rva })?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// The relative address the next section added to `bytes` gets.
pub fn next_section_rva(bytes: &[u8]) -> Result<u32, EditError> {
    let layout = Layout::read(bytes)?;
    let pe = Pe::parse(bytes)?;
    let end = pe
        .sections
        .iter()
        .map(|s| s.rva + s.virtual_size.max(s.raw_size))
        .max()
        .unwrap_or(layout.section_align);
    Ok(align_up(end, layout.section_align))
}

/// Appends a section `name` holding `data` (its virtual size is
/// `data.len()`), placed at [`next_section_rva`]. Returns the new image and
/// the section. The new header goes in the zero bytes after the section
/// table; no existing byte moves.
pub fn add_section(
    bytes: &[u8],
    name: &str,
    data: &[u8],
    characteristics: u32,
) -> Result<(Vec<u8>, Section), EditError> {
    if name.is_empty() || name.len() > 8 || !name.is_ascii() {
        return Err(EditError::BadSectionName(name.into()));
    }
    if data.is_empty() {
        return Err(EditError::BadSectionName(format!("{name} (no data)")));
    }
    let layout = Layout::read(bytes)?;
    let pe = Pe::parse(bytes)?;
    let header = layout.table + layout.sections * 40;
    // The headers end where the first section's file bytes start.
    let first_raw = pe
        .sections
        .iter()
        .filter(|s| s.raw_size > 0)
        .map(|s| s.raw_offset as usize)
        .min()
        .unwrap_or(bytes.len());
    let free = bytes
        .get(header..header + 40)
        .is_some_and(|slot| slot.iter().all(|&b| b == 0));
    if header + 40 > first_raw || !free {
        return Err(EditError::NoRoomForHeader);
    }
    let rva = next_section_rva(bytes)?;
    let raw_offset = align_up(bytes.len() as u32, layout.file_align);
    let raw_size = align_up(data.len() as u32, layout.file_align);
    let section = Section {
        name: name.into(),
        rva,
        virtual_size: data.len() as u32,
        raw_offset,
        raw_size,
        characteristics,
    };
    let mut out = bytes.to_vec();
    let mut padded = [0u8; 8];
    padded[..name.len()].copy_from_slice(name.as_bytes());
    out[header..header + 8].copy_from_slice(&padded);
    put_u32(&mut out, header + 8, section.virtual_size);
    put_u32(&mut out, header + 12, rva);
    put_u32(&mut out, header + 16, raw_size);
    put_u32(&mut out, header + 20, raw_offset);
    put_u32(&mut out, header + 36, characteristics);
    put_u16(&mut out, layout.count_at, (layout.sections + 1) as u16);
    let size_of_image = align_up(rva + section.virtual_size, layout.section_align);
    put_u32(&mut out, layout.optional + 56, size_of_image);
    // The checksum is only checked for drivers; zero says "not computed".
    put_u32(&mut out, layout.optional + 64, 0);
    out.resize(raw_offset as usize, 0);
    out.extend_from_slice(data);
    out.resize((raw_offset + raw_size) as usize, 0);
    Ok((out, section))
}

/// The image's imports, read through the import directory.
pub fn imports(bytes: &[u8]) -> Result<Vec<Import>, EditError> {
    let layout = Layout::read(bytes)?;
    let pe = Pe::parse(bytes)?;
    let (rva, _) = layout.dir(bytes, DIR_IMPORT)?;
    if rva == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let start = offset_of(&pe, rva, "import directory")?;
    for i in 0.. {
        let at = start + i * 20;
        let entry = bytes.get(at..at + 20).ok_or(EditError::Outside {
            what: "import descriptor",
            rva: rva + (i * 20) as u32,
        })?;
        if entry.iter().all(|&b| b == 0) {
            break;
        }
        let names = u32_at(bytes, at, "import names")?;
        let dll = string_at(
            bytes,
            &pe,
            u32_at(bytes, at + 12, "import dll")?,
            "dll name",
        )?;
        let iat_rva = u32_at(bytes, at + 16, "import addresses")?;
        let table = if names != 0 { names } else { iat_rva };
        let mut functions = Vec::new();
        let thunks = offset_of(&pe, table, "import name table")?;
        for k in 0.. {
            let t = thunks + k * layout.thunk();
            let (value, ordinal) = match layout.format {
                Format::Pe32 => {
                    let v = u32_at(bytes, t, "import thunk")?;
                    (u64::from(v), v & 0x8000_0000 != 0)
                }
                Format::Pe32Plus => {
                    let lo = u32_at(bytes, t, "import thunk")?;
                    let hi = u32_at(bytes, t + 4, "import thunk")?;
                    let v = u64::from(lo) | (u64::from(hi) << 32);
                    (v, hi & 0x8000_0000 != 0)
                }
            };
            if value == 0 {
                break;
            }
            functions.push(if ordinal {
                format!("#{}", value & 0xffff)
            } else {
                string_at(bytes, &pe, value as u32 + 2, "import name")?
            });
        }
        out.push(Import {
            dll,
            functions,
            iat_rva,
        });
    }
    Ok(out)
}

/// Adds `dll` to the import table with `functions` by name, in a new data
/// section `section`. The loader writes the functions' addresses at
/// `iat_rva` (consecutive slots, in order) when given — inside a writable
/// section the caller laid out, such as a fix's table — else in the new
/// section. Bound imports are unbound (the loader then resolves every DLL
/// by name, as for any unbound image). Windows refuses to start the image
/// when `dll` or a function is missing, so the caller makes sure the DLL
/// sits next to the executable.
pub fn add_import(
    bytes: &[u8],
    section: &str,
    dll: &str,
    functions: &[&str],
    iat_rva: Option<u32>,
) -> Result<Vec<u8>, EditError> {
    for name in std::iter::once(&dll).chain(functions) {
        if name.is_empty() || !name.is_ascii() || name.contains('\0') {
            return Err(EditError::BadName((*name).into()));
        }
    }
    if functions.is_empty() {
        return Err(EditError::BadName(format!("{dll} (no functions)")));
    }
    let layout = Layout::read(bytes)?;
    let pe = Pe::parse(bytes)?;
    if let Some(rva) = iat_rva {
        let span = (functions.len() * layout.thunk()) as u32;
        let writable = pe.sections.iter().any(|s| {
            s.characteristics & 0x8000_0000 != 0
                && rva >= s.rva
                && rva + span <= s.rva + s.virtual_size.max(s.raw_size)
        });
        if !writable {
            return Err(EditError::IatOutsideWritableSection { rva });
        }
    }
    let mut image = bytes.to_vec();
    // The existing descriptors, unbound.
    let (old_rva, _) = layout.dir(bytes, DIR_IMPORT)?;
    let mut descriptors: Vec<[u8; 20]> = Vec::new();
    if old_rva != 0 {
        let start = offset_of(&pe, old_rva, "import directory")?;
        for i in 0.. {
            let at = start + i * 20;
            let mut entry = [0u8; 20];
            entry.copy_from_slice(bytes.get(at..at + 20).ok_or(EditError::Outside {
                what: "import descriptor",
                rva: old_rva + (i * 20) as u32,
            })?);
            if entry.iter().all(|&b| b == 0) {
                break;
            }
            let stamp = u32::from_le_bytes(entry[4..8].try_into().unwrap());
            if stamp != 0 {
                if entry[0..4] == [0; 4] {
                    return Err(EditError::BoundWithoutNames {
                        dll_rva: u32::from_le_bytes(entry[12..16].try_into().unwrap()),
                    });
                }
                entry[4..12].fill(0);
            }
            descriptors.push(entry);
        }
    }
    layout.set_dir(&mut image, DIR_BOUND_IMPORT, 0, 0);

    // The new section: descriptors (old, new, end), the name table, the
    // address table (unless the caller placed it), hint/name entries, the
    // DLL name.
    let base = next_section_rva(&image)?;
    let thunk = layout.thunk();
    let descriptors_size = (descriptors.len() + 2) * 20;
    let names_at = descriptors_size;
    let table_size = (functions.len() + 1) * thunk;
    let own_iat_at = names_at + table_size;
    let fixed = own_iat_at + if iat_rva.is_none() { table_size } else { 0 };
    let mut data = vec![0u8; fixed];
    let mut thunks = Vec::new();
    for name in functions {
        // Hint 0, the name, NUL, padded to an even address.
        thunks.push(base + data.len() as u32);
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(name.as_bytes());
        data.push(0);
        if data.len() % 2 == 1 {
            data.push(0);
        }
    }
    let dll_at = data.len();
    data.extend_from_slice(dll.as_bytes());
    data.push(0);
    for (i, entry) in descriptors.iter().enumerate() {
        data[i * 20..i * 20 + 20].copy_from_slice(entry);
    }
    let first_thunk = iat_rva.unwrap_or(base + own_iat_at as u32);
    let new = descriptors.len() * 20;
    put_u32(&mut data, new, base + names_at as u32);
    put_u32(&mut data, new + 12, base + dll_at as u32);
    put_u32(&mut data, new + 16, first_thunk);
    for (k, value) in thunks.iter().enumerate() {
        let mut slots = vec![names_at + k * thunk];
        if iat_rva.is_none() {
            slots.push(own_iat_at + k * thunk);
        }
        for at in slots {
            put_u32(&mut data, at, *value);
        }
    }
    let (mut image, added) = add_section(&image, section, &data, SCN_DATA_RW)?;
    debug_assert_eq!(added.rva, base);
    layout.set_dir(&mut image, DIR_IMPORT, base, descriptors_size as u32);
    Ok(image)
}

/// Clears `DYNAMIC_BASE`, so the image always loads at its preferred base:
/// code written for fixed addresses (a fix section) stays right. Old games
/// have no relocations and never set it; returns whether it was set.
pub fn fix_base(bytes: &mut [u8]) -> Result<bool, EditError> {
    let layout = Layout::read(bytes)?;
    let at = layout.optional + 70;
    let flags = u16_at(bytes, at, "dll characteristics")?;
    put_u16(bytes, at, flags & !DYNAMIC_BASE);
    Ok(flags & DYNAMIC_BASE != 0)
}

#[cfg(test)]
mod tests {
    use super::super::synthetic::{CODE, DATA, image};
    use super::*;

    /// `bytes` with 0x200 more bytes of header space (linkers leave room
    /// after the section table; the synthetic stub has almost none).
    fn roomy(bytes: Vec<u8>) -> Vec<u8> {
        let layout = Layout::read(&bytes).unwrap();
        let mut out = bytes[..0x200].to_vec();
        out.resize(0x400, 0);
        out.extend_from_slice(&bytes[0x200..]);
        for i in 0..layout.sections {
            let at = layout.table + i * 40 + 20;
            let raw = u32_at(&out, at, "raw").unwrap();
            put_u32(&mut out, at, raw + 0x200);
        }
        put_u32(&mut out, layout.optional + 60, 0x400);
        out
    }

    /// A PE32 image importing `KERNEL32.dll!ExitProcess` the usual way:
    /// descriptors in `.rdata`, the address table in `.data`.
    fn with_imports(format: Format) -> Vec<u8> {
        // .rdata at 0x2000: descriptor (0..40), name table (0x40), hint/name
        // (0x60), dll name (0x70).
        let mut rdata = vec![0u8; 0x80];
        put_u32(&mut rdata, 0, 0x2040);
        put_u32(&mut rdata, 12, 0x2070);
        put_u32(&mut rdata, 16, 0x3000);
        put_u32(&mut rdata, 0x40, 0x2060);
        rdata[0x62..0x62 + 11].copy_from_slice(b"ExitProcess");
        rdata[0x70..0x70 + 12].copy_from_slice(b"KERNEL32.dll");
        let mut data = vec![0u8; 0x40];
        put_u32(&mut data, 0, 0x2060);
        let mut bytes = image(
            format,
            0x40_0000,
            &[
                (".text", 0x1000, &[0xc3], CODE),
                (".rdata", 0x2000, &rdata, DATA),
                (".data", 0x3000, &data, DATA | 0x8000_0000),
            ],
        );
        let layout = Layout::read(&bytes).unwrap();
        layout.set_dir(&mut bytes, DIR_IMPORT, 0x2000, 40);
        roomy(bytes)
    }

    #[test]
    fn a_section_is_appended_without_moving_anything() {
        let before = with_imports(Format::Pe32);
        let (after, s) = add_section(&before, ".rea", &[0xcc; 0x30], SCN_CODE_RWX).unwrap();
        assert_eq!(&after[..0x80], &before[..0x80], "the stub is untouched");
        assert_eq!(
            after[0x400..before.len()],
            before[0x400..],
            "section data stays put"
        );
        let pe = Pe::parse(&after).unwrap();
        assert_eq!(pe.sections.len(), 4);
        assert_eq!((s.rva, s.virtual_size, s.raw_size), (0x4000, 0x30, 0x200));
        assert_eq!(s.raw_offset as usize, before.len(), "appended at the end");
        assert_eq!(pe.section(".rea"), Some(&s));
        assert_eq!(pe.va_to_offset(0x40_4000), Some(s.raw_offset as usize));
        assert_eq!(after[s.raw_offset as usize], 0xcc);
        let size_of_image =
            u32_at(&after, Layout::read(&after).unwrap().optional + 56, "x").unwrap();
        assert_eq!(size_of_image, 0x5000);
        assert_eq!(next_section_rva(&after).unwrap(), 0x5000);
    }

    #[test]
    fn a_section_needs_free_header_space_and_a_good_name() {
        let bytes = with_imports(Format::Pe32);
        for name in ["", ".toolongname", "é"] {
            assert!(matches!(
                add_section(&bytes, name, &[1], DATA),
                Err(EditError::BadSectionName(_))
            ));
        }
        // Something already sits where the next header would go.
        let mut busy = bytes.clone();
        let layout = Layout::read(&busy).unwrap();
        busy[layout.table + 3 * 40] = 1;
        assert_eq!(
            add_section(&busy, ".rea", &[1], DATA).unwrap_err(),
            EditError::NoRoomForHeader
        );
    }

    #[test]
    fn an_import_is_added_after_the_existing_ones() {
        for format in [Format::Pe32, Format::Pe32Plus] {
            let bytes = with_imports(format);
            assert_eq!(
                imports(&bytes).unwrap(),
                [Import {
                    dll: "KERNEL32.dll".into(),
                    functions: vec!["ExitProcess".into()],
                    iat_rva: 0x3000
                }]
            );
            let out = add_import(&bytes, ".reaimp", "hooks.dll", &["one", "two_x"], None).unwrap();
            let found = imports(&out).unwrap();
            assert_eq!(found.len(), 2, "{format:?}");
            assert_eq!(found[0].dll, "KERNEL32.dll");
            assert_eq!(found[0].iat_rva, 0x3000, "the old address table stays");
            assert_eq!(found[1].dll, "hooks.dll");
            assert_eq!(found[1].functions, ["one", "two_x"]);
            let pe = Pe::parse(&out).unwrap();
            let s = pe.section(".reaimp").unwrap();
            assert!(found[1].iat_rva >= s.rva && found[1].iat_rva < s.rva + s.virtual_size);
        }
    }

    #[test]
    fn the_address_table_can_go_where_the_fix_reads_it() {
        let bytes = with_imports(Format::Pe32);
        let fix_rva = next_section_rva(&bytes).unwrap();
        let (bytes, fix) = add_section(&bytes, ".rea", &[0; 0x100], SCN_CODE_RWX).unwrap();
        let table = fix.rva + 0x40;
        let out = add_import(
            &bytes,
            ".reaimp",
            "cmrhooks.dll",
            &["a", "b", "c"],
            Some(table),
        )
        .unwrap();
        let found = imports(&out).unwrap();
        assert_eq!(found[1].iat_rva, fix_rva + 0x40);
        // The fix section's bytes are unchanged: the loader fills the slots.
        let pe = Pe::parse(&out).unwrap();
        let at = pe.va_to_offset(pe.image_base + u64::from(table)).unwrap();
        assert_eq!(&out[at..at + 12], &[0; 12]);
        // A table in a read-only section is refused.
        assert_eq!(
            add_import(&bytes, ".reaimp", "x.dll", &["a"], Some(0x2000)).unwrap_err(),
            EditError::IatOutsideWritableSection { rva: 0x2000 }
        );
    }

    #[test]
    fn bound_imports_are_unbound_and_unbindable_ones_refused() {
        let mut bytes = with_imports(Format::Pe32);
        let pe = Pe::parse(&bytes).unwrap();
        let desc = pe.va_to_offset(0x40_2000).unwrap();
        put_u32(&mut bytes, desc + 4, u32::MAX);
        let layout = Layout::read(&bytes).unwrap();
        layout.set_dir(&mut bytes, DIR_BOUND_IMPORT, 0x200, 0x20);
        let out = add_import(&bytes, ".reaimp", "hooks.dll", &["f"], None).unwrap();
        assert_eq!(layout.dir(&out, DIR_BOUND_IMPORT).unwrap(), (0, 0));
        let new_dir = layout.dir(&out, DIR_IMPORT).unwrap().0;
        let pe = Pe::parse(&out).unwrap();
        let at = pe.va_to_offset(pe.image_base + u64::from(new_dir)).unwrap();
        assert_eq!(u32_at(&out, at + 4, "stamp").unwrap(), 0);
        // Bound with no name table: the names are gone, so refuse.
        put_u32(&mut bytes, desc, 0);
        assert!(matches!(
            add_import(&bytes, ".reaimp", "hooks.dll", &["f"], None),
            Err(EditError::BoundWithoutNames { .. })
        ));
    }

    #[test]
    fn names_are_checked() {
        let bytes = with_imports(Format::Pe32);
        assert!(matches!(
            add_import(&bytes, ".i", "", &["f"], None),
            Err(EditError::BadName(_))
        ));
        assert!(matches!(
            add_import(&bytes, ".i", "a.dll", &[], None),
            Err(EditError::BadName(_))
        ));
        assert!(matches!(
            add_import(&bytes, ".i", "a.dll", &["f\0g"], None),
            Err(EditError::BadName(_))
        ));
    }

    #[test]
    fn an_image_without_imports_gets_its_first() {
        let bytes = roomy(image(
            Format::Pe32,
            0x40_0000,
            &[(".text", 0x1000, &[0xc3], CODE)],
        ));
        assert!(imports(&bytes).unwrap().is_empty());
        let out = add_import(&bytes, ".reaimp", "hooks.dll", &["f"], None).unwrap();
        assert_eq!(imports(&out).unwrap()[0].functions, ["f"]);
    }

    #[test]
    fn the_base_is_fixed() {
        let mut bytes = with_imports(Format::Pe32);
        let at = Layout::read(&bytes).unwrap().optional + 70;
        put_u16(&mut bytes, at, 0x8140);
        assert!(fix_base(&mut bytes).unwrap());
        assert_eq!(u16_at(&bytes, at, "x").unwrap(), 0x8100);
        assert!(!fix_base(&mut bytes).unwrap());
    }
}
