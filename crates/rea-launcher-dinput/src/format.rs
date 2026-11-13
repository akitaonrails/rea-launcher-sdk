//! The virtual wheel's DirectInput objects, and a game's data format
//! (`DIDATAFORMAT`) translated onto them.
//!
//! A game hands `SetDataFormat` a list of entries (a GUID or none, an
//! offset in its state buffer, a type and instance filter, an aspect) and
//! DirectInput gives each entry, in order, the first unused object that
//! fits it. The CMR games pass `c_dfDIJoystick2` but nothing here assumes
//! it: any format the game builds is translated the same way.

use rea_launcher::wheel::dinput::{Axis, VIRTUAL};

/// `DIDFT_*`: object types and type filters.
pub mod didft {
    pub const ABSAXIS: u32 = 0x02;
    pub const AXIS: u32 = 0x03;
    pub const PSHBUTTON: u32 = 0x04;
    pub const BUTTON: u32 = 0x0C;
    pub const POV: u32 = 0x10;
    pub const ANYINSTANCE: u32 = 0x00FF_FF00;
    pub const FFACTUATOR: u32 = 0x0100_0000;
    pub const OPTIONAL: u32 = 0x8000_0000;

    pub const fn make_instance(n: u16) -> u32 {
        (n as u32) << 8
    }

    pub const fn instance(t: u32) -> u16 {
        (t >> 8) as u16
    }

    pub const fn kind(t: u32) -> u32 {
        t & 0xFF
    }
}

/// `DIDOI_*`: object flags.
pub mod didoi {
    pub const FFACTUATOR: u32 = 0x01;
    pub const ASPECTPOSITION: u32 = 0x100;
    pub const ASPECTVELOCITY: u32 = 0x200;
    pub const ASPECTACCEL: u32 = 0x300;
    pub const ASPECTFORCE: u32 = 0x400;
    pub const ASPECTMASK: u32 = 0xF00;
}

/// What a format entry's or an object's GUID names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guid {
    XAxis,
    YAxis,
    ZAxis,
    RxAxis,
    RyAxis,
    RzAxis,
    Slider,
    Button,
    Key,
    Pov,
    /// A GUID no object of a joystick has.
    Other,
}

/// One object of the virtual wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Object {
    Axis(Axis),
    Button(u8),
    Pov,
}

/// The virtual wheel's buttons, as a count of the frozen layout.
pub const BUTTONS: u8 = VIRTUAL.buttons as u8;

/// `DIJOYSTATE2`: 272 bytes, `rgdwPOV` at 32, `rgbButtons` at 48.
pub const JOYSTATE2_SIZE: u32 = 272;
const JOY_POV: u32 = 32;
const JOY_BUTTONS: u32 = 48;

impl Object {
    /// Every object in `EnumObjects` order: the axes, the buttons, the hat.
    pub fn all() -> impl Iterator<Item = Object> {
        Axis::ALL
            .into_iter()
            .map(Object::Axis)
            .chain((0..BUTTONS).map(Object::Button))
            .chain([Object::Pov])
    }

    pub fn guid(self) -> Guid {
        match self {
            Object::Axis(Axis::X) => Guid::XAxis,
            Object::Axis(Axis::Y) => Guid::YAxis,
            Object::Axis(Axis::Z) => Guid::ZAxis,
            Object::Axis(Axis::Rz) => Guid::RzAxis,
            Object::Button(_) => Guid::Button,
            Object::Pov => Guid::Pov,
        }
    }

    /// The object's instance number among objects of its type.
    pub fn instance(self) -> u16 {
        match self {
            Object::Axis(a) => Axis::ALL.iter().position(|x| *x == a).unwrap_or(0) as u16,
            Object::Button(n) => u16::from(n),
            Object::Pov => 0,
        }
    }

    /// `dwType` as `EnumObjects` reports it (the object's ID).
    pub fn type_id(self) -> u32 {
        let base = match self {
            Object::Axis(Axis::X) => didft::ABSAXIS | didft::FFACTUATOR,
            Object::Axis(_) => didft::ABSAXIS,
            Object::Button(_) => didft::PSHBUTTON,
            Object::Pov => didft::POV,
        };
        base | didft::make_instance(self.instance())
    }

    /// `dwFlags` as `EnumObjects` reports it.
    pub fn flags(self) -> u32 {
        match self {
            Object::Axis(Axis::X) => didoi::ASPECTPOSITION | didoi::FFACTUATOR,
            Object::Axis(_) => didoi::ASPECTPOSITION,
            Object::Button(_) | Object::Pov => 0,
        }
    }

    /// Where `c_dfDIJoystick2` puts it in a `DIJOYSTATE2`.
    pub fn joystate2_offset(self) -> u32 {
        match self {
            Object::Axis(Axis::X) => 0,
            Object::Axis(Axis::Y) => 4,
            Object::Axis(Axis::Z) => 8,
            Object::Axis(Axis::Rz) => 20,
            Object::Button(n) => JOY_BUTTONS + u32::from(n),
            Object::Pov => JOY_POV,
        }
    }

    /// The name a game's binding menu shows.
    pub fn name(self) -> String {
        match self {
            Object::Axis(Axis::X) => "Steering".into(),
            Object::Axis(Axis::Y) => "Pedals (combined)".into(),
            Object::Axis(Axis::Z) => "Throttle".into(),
            Object::Axis(Axis::Rz) => "Brake".into(),
            Object::Button(n) => format!("Button {}", n + 1),
            Object::Pov => "Hat".into(),
        }
    }

    /// HID usage page and usage (`DIPH_BYUSAGE`).
    pub fn usage(self) -> (u16, u16) {
        match self {
            Object::Axis(Axis::X) => (1, 0x30),
            Object::Axis(Axis::Y) => (1, 0x31),
            Object::Axis(Axis::Z) => (1, 0x32),
            Object::Axis(Axis::Rz) => (1, 0x35),
            Object::Button(n) => (9, u16::from(n) + 1),
            Object::Pov => (1, 0x39),
        }
    }

    /// Whether `id` (a `DIPH_BYID` or `GetObjectInfo` ID) names it: the
    /// same instance of an overlapping type.
    pub fn has_id(self, id: u32) -> bool {
        didft::instance(id) == self.instance() && didft::kind(id) & didft::kind(self.type_id()) != 0
    }

    /// Whether `EnumObjects` with `filter` (`DIDFT_ALL` = 0, or types and
    /// `DIDFT_FFACTUATOR`) lists it.
    pub fn in_filter(self, filter: u32) -> bool {
        let kind = didft::kind(filter);
        (kind == 0 || kind & didft::kind(self.type_id()) != 0)
            && (filter & didft::FFACTUATOR == 0 || self.flags() & didoi::FFACTUATOR != 0)
    }

    pub fn by_usage(page: u16, usage: u16) -> Option<Object> {
        Object::all().find(|o| o.usage() == (page, usage))
    }

    pub fn by_id(id: u32) -> Option<Object> {
        Object::all().find(|o| o.has_id(id))
    }

    pub fn is_axis(self) -> bool {
        matches!(self, Object::Axis(_))
    }

    /// Bytes the object takes in a state buffer.
    fn width(self) -> u32 {
        match self {
            Object::Button(_) => 1,
            _ => 4,
        }
    }
}

/// One `DIOBJECTDATAFORMAT` of a game's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// `pguid`; `None` for a null pointer (any object of the type).
    pub guid: Option<Guid>,
    pub offset: u32,
    /// `dwType`: type filter, instance (or any), `DIDFT_OPTIONAL`.
    pub kind: u32,
    /// `dwFlags`: the aspect.
    pub flags: u32,
}

impl Entry {
    fn fits(&self, object: Object) -> bool {
        let kind = didft::kind(self.kind);
        let instance = didft::instance(self.kind);
        let aspect = self.flags & didoi::ASPECTMASK;
        self.guid.is_none_or(|g| g == object.guid())
            && (kind == 0 || kind & didft::kind(object.type_id()) != 0)
            && (instance == 0xFFFF || instance == object.instance())
            && (self.kind & didft::FFACTUATOR == 0 || object.flags() & didoi::FFACTUATOR != 0)
            && (aspect == 0 || aspect == object.flags() & didoi::ASPECTMASK)
    }

    fn is_pov(&self) -> bool {
        self.guid == Some(Guid::Pov) || didft::kind(self.kind) == didft::POV
    }
}

/// `c_dfDIJoystick2`'s 164 entries, in its order: six axes and two
/// sliders, four hats, 128 buttons, then the velocity, acceleration and
/// force aspects of the axes and sliders.
pub fn joystate2() -> Vec<Entry> {
    use Guid::*;
    const AXES: [Guid; 8] = [XAxis, YAxis, ZAxis, RxAxis, RyAxis, RzAxis, Slider, Slider];
    let axis = didft::OPTIONAL | didft::AXIS | didft::ANYINSTANCE;
    let at = |guid, offset, kind, flags| Entry {
        guid,
        offset,
        kind,
        flags,
    };
    let mut entries: Vec<Entry> = AXES
        .iter()
        .zip((0..).step_by(4))
        .map(|(g, o)| at(Some(*g), o, axis, didoi::ASPECTPOSITION))
        .collect();
    let pov = didft::OPTIONAL | didft::POV | didft::ANYINSTANCE;
    entries.extend((0..4).map(|i| at(Some(Pov), JOY_POV + 4 * i, pov, 0)));
    let button = didft::OPTIONAL | didft::BUTTON | didft::ANYINSTANCE;
    entries.extend((0..128).map(|i| at(None, JOY_BUTTONS + i, button, 0)));
    let aspects = [
        (176, didoi::ASPECTVELOCITY),
        (208, didoi::ASPECTACCEL),
        (240, didoi::ASPECTFORCE),
    ];
    for (start, aspect) in aspects {
        entries.extend(
            AXES.iter()
                .zip((start..).step_by(4))
                .map(|(g, o)| at(Some(*g), o, axis, aspect)),
        );
    }
    entries
}

/// Why a data format was refused (`DIERR_INVALIDPARAM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    /// The state size is not a multiple of four.
    Size(u32),
    /// The entry at this index lies outside the state or is misaligned.
    Offset(usize),
    /// The entry at this index is required and no object fits it.
    Missing(usize),
}

/// The virtual device's state in the game's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Values {
    /// In [`Axis::ALL`] order, inside each axis's range.
    pub axes: [i32; 4],
    /// Bit n: button n is down.
    pub buttons: u32,
    /// Hundredths of a degree clockwise from north, or centred
    /// ([`POV_CENTRED`]).
    pub pov: u32,
}

/// A hat at rest.
pub const POV_CENTRED: u32 = u32::MAX;

impl Values {
    /// What `object` reads, as `DIDEVICEOBJECTDATA::dwData` carries it.
    pub fn of(&self, object: Object) -> u32 {
        match object {
            Object::Axis(a) => {
                let i = Axis::ALL.iter().position(|x| *x == a).unwrap_or(0);
                self.axes[i] as u32
            }
            Object::Button(n) if self.buttons & (1 << n) != 0 => 0x80,
            Object::Button(_) => 0,
            Object::Pov => self.pov,
        }
    }
}

/// A game's data format translated onto the virtual wheel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// `dwDataSize`: the size of the game's state buffer.
    pub size: u32,
    /// Each object the format uses, with its offset.
    slots: Vec<(Object, u32)>,
    /// Hats the format asks for that the wheel doesn't have: they read
    /// centred, as on a real device, never "north".
    idle_povs: Vec<u32>,
}

impl Layout {
    /// The format `entries` of a `size`-byte state, as DirectInput maps it.
    pub fn translate(size: u32, entries: &[Entry]) -> Result<Layout, FormatError> {
        if !size.is_multiple_of(4) {
            return Err(FormatError::Size(size));
        }
        let mut used = Vec::new();
        let mut idle_povs = Vec::new();
        for (i, entry) in entries.iter().enumerate() {
            let object =
                Object::all().find(|o| !used.iter().any(|(u, _)| u == o) && entry.fits(*o));
            let width = object.map_or(if entry.is_pov() { 4 } else { 1 }, Object::width);
            let aligned = width == 1 || entry.offset % 4 == 0;
            if !aligned || entry.offset.checked_add(width).is_none_or(|end| end > size) {
                return Err(FormatError::Offset(i));
            }
            match object {
                Some(o) => used.push((o, entry.offset)),
                None if entry.kind & didft::OPTIONAL == 0 => return Err(FormatError::Missing(i)),
                None if entry.is_pov() => idle_povs.push(entry.offset),
                None => {}
            }
        }
        Ok(Layout {
            size,
            slots: used,
            idle_povs,
        })
    }

    /// `c_dfDIJoystick2`'s layout.
    pub fn joystate2() -> Layout {
        Self::translate(JOYSTATE2_SIZE, &joystate2()).expect("c_dfDIJoystick2 fits")
    }

    pub fn offset_of(&self, object: Object) -> Option<u32> {
        self.slots
            .iter()
            .find(|(o, _)| *o == object)
            .map(|(_, at)| *at)
    }

    pub fn object_at(&self, offset: u32) -> Option<Object> {
        self.slots
            .iter()
            .find(|(_, at)| *at == offset)
            .map(|(o, _)| *o)
    }

    /// The objects the format uses, with their offsets.
    pub fn slots(&self) -> &[(Object, u32)] {
        &self.slots
    }

    /// What changed from `old` to `new`, as buffered data reports it:
    /// each object's offset and new `dwData`, in format order.
    pub fn changes(&self, old: &Values, new: &Values) -> Vec<(u32, u32)> {
        self.slots
            .iter()
            .filter(|(o, _)| old.of(*o) != new.of(*o))
            .map(|&(o, at)| (at, new.of(o)))
            .collect()
    }

    /// `values` written into `buffer` (the game's `size` bytes): unused
    /// bytes zero, unused hats centred.
    pub fn write(&self, values: &Values, buffer: &mut [u8]) {
        buffer.fill(0);
        let mut put = |at: u32, bytes: &[u8]| {
            if let Some(dst) = buffer.get_mut(at as usize..at as usize + bytes.len()) {
                dst.copy_from_slice(bytes);
            }
        };
        for &(object, at) in &self.slots {
            let value = values.of(object);
            match object {
                Object::Button(_) => put(at, &[value as u8]),
                _ => put(at, &value.to_le_bytes()),
            }
        }
        for &at in &self.idle_povs {
            put(at, &POV_CENTRED.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neutral() -> Values {
        Values {
            axes: [1, 2, 3, 4],
            buttons: 0b101,
            pov: 9000,
        }
    }

    fn word(buffer: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(buffer[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn joystate2_puts_each_object_where_dijoystate2_has_it() {
        let layout = Layout::joystate2();
        for object in Object::all() {
            assert_eq!(
                layout.offset_of(object),
                Some(object.joystate2_offset()),
                "{object:?}"
            );
        }
        let mut buffer = vec![0xAA; JOYSTATE2_SIZE as usize];
        layout.write(&neutral(), &mut buffer);
        assert_eq!(
            [
                word(&buffer, 0),
                word(&buffer, 4),
                word(&buffer, 8),
                word(&buffer, 20)
            ],
            [1, 2, 3, 4]
        );
        assert_eq!(word(&buffer, 12), 0, "Rx is not on the wheel");
        assert_eq!(word(&buffer, 32), 9000);
        for hat in 1..4 {
            assert_eq!(word(&buffer, 32 + 4 * hat), POV_CENTRED, "hat {hat}");
        }
        assert_eq!(&buffer[48..52], &[0x80, 0, 0x80, 0]);
        assert_eq!(buffer[48 + 32], 0, "only 32 buttons");
        assert_eq!(word(&buffer, 176), 0, "no velocity aspect");
    }

    #[test]
    fn a_format_of_its_own_is_translated_too() {
        // Steering and brake by GUID, two buttons by instance, any axis.
        let entries = [
            Entry {
                guid: Some(Guid::RzAxis),
                offset: 0,
                kind: didft::AXIS | didft::ANYINSTANCE,
                flags: 0,
            },
            Entry {
                guid: None,
                offset: 4,
                kind: didft::BUTTON | didft::make_instance(3),
                flags: 0,
            },
            Entry {
                guid: None,
                offset: 5,
                kind: didft::BUTTON | didft::make_instance(0),
                flags: 0,
            },
            Entry {
                guid: None,
                offset: 8,
                kind: didft::AXIS | didft::ANYINSTANCE,
                flags: 0,
            },
        ];
        let layout = Layout::translate(12, &entries).unwrap();
        assert_eq!(
            layout.slots(),
            &[
                (Object::Axis(Axis::Rz), 0),
                (Object::Button(3), 4),
                (Object::Button(0), 5),
                (Object::Axis(Axis::X), 8),
            ]
        );
        let mut buffer = [0u8; 12];
        let values = Values {
            buttons: 1 << 3,
            ..neutral()
        };
        layout.write(&values, &mut buffer);
        assert_eq!(
            (word(&buffer, 0), buffer[4], buffer[5], word(&buffer, 8)),
            (4, 0x80, 0, 1)
        );
        assert_eq!(layout.object_at(8), Some(Object::Axis(Axis::X)));
        assert_eq!(layout.object_at(12), None);
    }

    #[test]
    fn changes_are_reported_per_object_at_its_offset() {
        let layout = Layout::joystate2();
        let old = neutral();
        let new = Values {
            axes: [1, 2, 9, 4],
            buttons: 0b110,
            pov: 9000,
        };
        assert_eq!(
            layout.changes(&old, &new),
            vec![(8, 9), (48, 0), (49, 0x80)]
        );
        assert!(layout.changes(&new, &new).is_empty());
    }

    #[test]
    fn force_feedback_axes_can_be_asked_for() {
        let ff = |instance| Entry {
            guid: None,
            offset: 0,
            kind: didft::AXIS | didft::FFACTUATOR | instance,
            flags: 0,
        };
        let layout = Layout::translate(4, &[ff(didft::ANYINSTANCE)]).unwrap();
        assert_eq!(layout.object_at(0), Some(Object::Axis(Axis::X)));
        // Y is no actuator.
        assert_eq!(
            Layout::translate(4, &[ff(didft::make_instance(1))]),
            Err(FormatError::Missing(0))
        );
    }

    #[test]
    fn bad_formats_are_refused() {
        let x = Entry {
            guid: Some(Guid::XAxis),
            offset: 0,
            kind: didft::AXIS | didft::ANYINSTANCE,
            flags: 0,
        };
        assert_eq!(Layout::translate(6, &[x]), Err(FormatError::Size(6)));
        assert_eq!(
            Layout::translate(4, &[Entry { offset: 2, ..x }]),
            Err(FormatError::Offset(0))
        );
        assert_eq!(
            Layout::translate(4, &[Entry { offset: 4, ..x }]),
            Err(FormatError::Offset(0))
        );
        let slider = Entry {
            guid: Some(Guid::Slider),
            ..x
        };
        assert_eq!(
            Layout::translate(4, &[slider]),
            Err(FormatError::Missing(0))
        );
        let optional = Entry {
            kind: slider.kind | didft::OPTIONAL,
            ..slider
        };
        assert!(Layout::translate(4, &[optional]).is_ok());
        // A second X has nothing left to map.
        assert_eq!(
            Layout::translate(8, &[x, Entry { offset: 4, ..x }]),
            Err(FormatError::Missing(1))
        );
    }

    #[test]
    fn objects_are_found_by_id_usage_and_filter() {
        for object in Object::all() {
            assert_eq!(Object::by_id(object.type_id()), Some(object));
            let (page, usage) = object.usage();
            assert_eq!(Object::by_usage(page, usage), Some(object));
        }
        assert_eq!(
            Object::by_id(didft::AXIS | didft::make_instance(3)),
            Some(Object::Axis(Axis::Rz))
        );
        let listed = |filter| Object::all().filter(|o| o.in_filter(filter)).count();
        assert_eq!(listed(0), 4 + 32 + 1);
        assert_eq!(listed(didft::AXIS), 4);
        assert_eq!(listed(didft::BUTTON), 32);
        assert_eq!(listed(didft::POV), 1);
        assert_eq!(listed(didft::AXIS | didft::FFACTUATOR), 1);
    }
}
