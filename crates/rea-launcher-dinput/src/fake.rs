//! Synthetic real devices, **for tests only**: `fake = wheel` or
//! `fake = wheel+pedals` in `rea_wheel.ini` adds them to the devices the
//! layer finds, so the whole chain (picking, reading, force feedback on
//! the "real" wheel) runs under Wine without hardware. A test drives them
//! through the DLL's `rea_dinput_fake_*` exports. The key is not part of
//! [`rea_launcher::wheel::dinput::Setup`]: launchers never write it.

use rea_launcher::cfg;
use rea_launcher::wheel::rig::{self, Device};
use rea_launcher::wheel::{DeviceId, DeviceState};

/// The settings key.
pub const KEY: &str = "fake";

/// Which fake devices to add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fake {
    /// A force-feedback wheel with its pedals on it (axes Y and Z).
    Wheel,
    /// The wheel without pedals, and a pedal device of its own.
    WheelAndPedals,
}

/// A fake device: what the layer finds, and where its axes rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeDevice {
    pub device: Device,
    /// `DI8DEVTYPE_*`.
    pub dev_type: u32,
    /// `DIJOYSTATE2` axes at rest, at the real range.
    pub rest: [i32; 8],
}

/// The fake wheel has a Fanatec CSL DD's IDs (1080°), so the steering
/// lock has something to do; the pedals pid.codes' test IDs (a wheel
/// vendor's pedals would count as a wheel).
pub const WHEEL_GUID: &str = "{7265612d-6661-6b65-0000-000000000001}";
pub const PEDALS_GUID: &str = "{7265612d-6661-6b65-0000-000000000002}";

impl Fake {
    /// The `fake` key of a settings file.
    pub fn from_cfg(text: &str) -> Option<Fake> {
        let value = cfg::entries(text)
            .filter(|e| e.key.eq_ignore_ascii_case(KEY))
            .last()?
            .value
            .trim()
            .to_ascii_lowercase();
        match value.as_str() {
            "wheel" => Some(Fake::Wheel),
            "wheel+pedals" => Some(Fake::WheelAndPedals),
            _ => None,
        }
    }

    /// The devices, the wheel first.
    pub fn devices(self) -> Vec<FakeDevice> {
        let id = |name: &str, vid, pid, guid: &str| DeviceId {
            name: name.into(),
            vid,
            pid,
            guid: guid.into(),
        };
        let max = 32_767;
        let wheel = |rest| {
            fake(
                id("rea fake wheel", 0x0eb7, 0x0020, WHEEL_GUID),
                rig::DEVTYPE_DRIVING,
                3,
                12,
                true,
                rest,
            )
        };
        match self {
            Fake::Wheel => vec![wheel([0, max, max, 0, 0, 0, 0, 0])],
            Fake::WheelAndPedals => vec![
                wheel([0; 8]),
                fake(
                    id("rea fake pedals", 0x1209, 0x0001, PEDALS_GUID),
                    0x14,
                    2,
                    4,
                    false,
                    [max, max, 0, 0, 0, 0, 0, 0],
                ),
            ],
        }
    }
}

fn fake(
    id: DeviceId,
    dev_type: u32,
    axes: usize,
    buttons: usize,
    ffb: bool,
    rest: [i32; 8],
) -> FakeDevice {
    let state: DeviceState = rig::reading(rest, &[], 0);
    FakeDevice {
        device: Device {
            kind: rig::kind(id.vid, id.pid, dev_type, axes),
            id,
            axes,
            buttons,
            ffb,
            initial: state.clone(),
            state,
        },
        dev_type,
        rest,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::wheel::Kind;

    #[test]
    fn the_key_names_a_set_of_devices() {
        assert_eq!(Fake::from_cfg("fake = wheel\n"), Some(Fake::Wheel));
        assert_eq!(
            Fake::from_cfg("mode = on\nFAKE = Wheel+Pedals\n"),
            Some(Fake::WheelAndPedals)
        );
        assert_eq!(Fake::from_cfg("fake = car\n"), None);
        assert_eq!(Fake::from_cfg("mode = on\n"), None);
    }

    #[test]
    fn the_fake_wheel_is_a_wheel_and_the_pedals_are_not() {
        let devices = Fake::WheelAndPedals.devices();
        assert_eq!(devices[0].device.kind, Kind::Wheel);
        assert_eq!(devices[1].device.kind, Kind::Other);
        assert_eq!(Fake::Wheel.devices().len(), 1);
    }
}
