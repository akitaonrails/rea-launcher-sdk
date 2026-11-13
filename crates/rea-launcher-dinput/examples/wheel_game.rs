//! A stand-in for a Colin McRae game's input layer, for the layer's
//! tests (`tests/game.rs`): it imports `dinput8!DirectInput8Create`, and
//! does what CMR3/04/2005 do with a controller (EnumDevices of the
//! attached game controllers, `c_dfDIJoystick2`, every axis at 0..255 by
//! object ID, exclusive and foreground on a hidden window, force
//! feedback through Spring, Damper, Sine, Square and Constant effects).
//! It prints `key=value` lines of what it saw; its arguments are steps:
//!
//! - `read`: Poll and GetDeviceState, printed as `state=X,Y,Z,Rz
//!   buttons=… pov=…`;
//! - `input:<device>:<8 axes>:<buttons>:<pov>`: moves a fake device;
//! - `unplug:<device>`, `plug:<device>`: unplugs a fake device or plugs
//!   it back;
//! - `effects`: autocentre off, the five effects made and started, then
//!   the constant force changed with `DIEP_NORESTART`;
//! - `buffer:<n>`: buffered data of `n` events (unacquired around it);
//! - `data`: GetDeviceData, as `data=<code>:<offset>=<value>,…`;
//! - `update:<magnitude>`: the constant force changed again
//!   (`DIEP_NORESTART`);
//! - `incomplete`: a constant force made without parameters, started,
//!   then given them;
//! - `reset`: `DISFFC_RESET`, then every effect started again;
//! - `autocenter:<0|1>`: `DIPROP_AUTOCENTER`;
//! - `second`: a second virtual device made, read and released;
//!   `second-drop`: the same, released while still acquired;
//! - `unacquire`: the wheel unacquired;
//! - `gain:<n>`: `DIPROP_FFGAIN` set, then read back as `gain=<code>,<n>`;
//! - `events`: what reached the fake devices, as `fake <line>`;
//! - `wait:<ms>`.
//!
//! With `REA_WHEEL_GAME_LOAD=<file>` it first loads that DLL itself, as
//! an ASI loader would (the layer shipped as `rea_dinput.asi`).

#[cfg(windows)]
fn main() {
    game::run(std::env::args().skip(1).collect());
}

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
mod game {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    use rea_dinput::format::{self, Guid};
    use windows_sys::Win32::Devices::HumanInterfaceDevice::{
        DI8DEVCLASS_GAMECTRL, DICONDITION, DICONSTANTFORCE, DIDATAFORMAT, DIDEVCAPS,
        DIDEVICEINSTANCEA, DIDEVICEOBJECTDATA, DIDEVICEOBJECTINSTANCEA, DIDF_ABSAXIS, DIDFT_AXIS,
        DIEB_NOTRIGGER, DIEDFL_ATTACHEDONLY, DIEDFL_FORCEFEEDBACK, DIEFF_CARTESIAN,
        DIEFF_OBJECTOFFSETS, DIEFFECT, DIENUM_CONTINUE, DIEP_AXES, DIEP_DIRECTION, DIEP_DURATION,
        DIEP_NORESTART, DIEP_TYPESPECIFICPARAMS, DIJOYSTATE2, DIOBJECTDATAFORMAT, DIPERIODIC,
        DIPH_BYID, DIPH_DEVICE, DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DISCL_EXCLUSIVE,
        DISCL_FOREGROUND, DirectInput8Create, GUID_Button, GUID_ConstantForce, GUID_Damper,
        GUID_POV, GUID_RxAxis, GUID_RyAxis, GUID_RzAxis, GUID_Sine, GUID_Slider, GUID_Spring,
        GUID_Square, GUID_XAxis, GUID_YAxis, GUID_ZAxis,
    };
    use windows_sys::Win32::Foundation::{HMODULE, HWND};
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, RegisterClassW, WNDCLASSW,
    };
    use windows_sys::core::{BOOL, GUID, HRESULT};

    type This = *mut c_void;
    type SetProperty = unsafe extern "system" fn(This, usize, *const DIPROPHEADER) -> HRESULT;
    type CreateEffect =
        unsafe extern "system" fn(This, *const GUID, *const DIEFFECT, *mut This, This) -> HRESULT;
    type SetParameters = unsafe extern "system" fn(This, *const DIEFFECT, u32) -> HRESULT;
    type Start = unsafe extern "system" fn(This, u32, u32) -> HRESULT;

    /// `IID_IDirectInput8A`.
    const IID_DIRECTINPUT8A: GUID = GUID::from_u128(0xbf798030_483a_4da2_aa99_5d64ed369700);

    /// Method `n` of a COM object, as a function of type `F`.
    ///
    /// # Safety
    /// `this` is a live COM object whose method `n` has type `F`.
    unsafe fn method<F: Copy>(this: This, n: usize) -> F {
        // SAFETY: the caller's contract.
        unsafe {
            let vtbl = *this.cast::<*const usize>();
            std::mem::transmute_copy(&*vtbl.add(n))
        }
    }

    unsafe fn release(this: This) {
        // SAFETY: IUnknown::Release.
        unsafe { method::<unsafe extern "system" fn(This) -> u32>(this, 2)(this) };
    }

    /// The file name of the module that implements `this`'s methods.
    fn module_of(this: This) -> String {
        let mut module: HMODULE = null_mut();
        let mut name = [0u16; 260];
        // SAFETY: a live object's first method; plain module queries.
        let len = unsafe {
            let first = method::<usize>(this, 0);
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                first as *const u16,
                &mut module,
            );
            GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32)
        };
        let path = String::from_utf16_lossy(&name[..len as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    }

    fn guid_ptr(guid: Guid) -> *const GUID {
        match guid {
            Guid::XAxis => &GUID_XAxis,
            Guid::YAxis => &GUID_YAxis,
            Guid::ZAxis => &GUID_ZAxis,
            Guid::RxAxis => &GUID_RxAxis,
            Guid::RyAxis => &GUID_RyAxis,
            Guid::RzAxis => &GUID_RzAxis,
            Guid::Slider => &GUID_Slider,
            Guid::Button => &GUID_Button,
            Guid::Pov => &GUID_POV,
            Guid::Key | Guid::Other => null(),
        }
    }

    fn hidden_window() -> HWND {
        unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
            // SAFETY: the default procedure for our window.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        let class: Vec<u16> = "rea_wheel_game\0".encode_utf16().collect();
        // SAFETY: a window class and a hidden window of it; pointers to
        // live locals.
        unsafe {
            let wc = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: GetModuleHandleW(null()),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&wc);
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                64,
                64,
                null_mut(),
                null_mut(),
                wc.hInstance,
                null(),
            )
        }
    }

    /// The layer's file: the one the game loaded itself, else the DLL in
    /// its import table.
    fn layer() -> String {
        std::env::var("REA_WHEEL_GAME_LOAD").unwrap_or_else(|_| "rea_dinput.dll".into())
    }

    /// A function the DLL `dll` next to the game exports, if loaded (by
    /// path: the stand-in and the system's DirectInput share a name).
    fn export<F: Copy>(dll: &str, name: &std::ffi::CStr) -> Option<F> {
        let exe = std::env::current_exe().ok()?;
        let path: Vec<u16> = exe
            .with_file_name(dll)
            .as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .chain([0])
            .collect();
        // SAFETY: plain module queries; the caller names the type.
        unsafe {
            let module = GetModuleHandleW(path.as_ptr());
            if module.is_null() {
                return None;
            }
            GetProcAddress(module, name.as_ptr().cast()).map(|f| std::mem::transmute_copy(&f))
        }
    }

    unsafe extern "system" fn collect(instance: *mut DIDEVICEINSTANCEA, list: *mut c_void) -> BOOL {
        // SAFETY: `list` is the game's Vec; the instance is DirectInput's.
        unsafe { (*list.cast::<Vec<DIDEVICEINSTANCEA>>()).push(*instance) };
        DIENUM_CONTINUE as BOOL
    }

    unsafe extern "system" fn collect_axes(
        object: *mut DIDEVICEOBJECTINSTANCEA,
        list: *mut c_void,
    ) -> BOOL {
        // SAFETY: as above.
        unsafe { (*list.cast::<Vec<u32>>()).push((*object).dwType) };
        DIENUM_CONTINUE as BOOL
    }

    fn name(instance: &DIDEVICEINSTANCEA) -> String {
        let bytes: Vec<u8> = instance
            .tszProductName
            .iter()
            .take_while(|&&c| c != 0)
            .map(|&c| c as u8)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn run(steps: Vec<String>) {
        // An ASI loader's way in: the layer loaded after the import table
        // was bound, before the game creates DirectInput.
        if let Ok(layer) = std::env::var("REA_WHEEL_GAME_LOAD") {
            let name: Vec<u16> = layer.encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated name.
            let module = unsafe { LoadLibraryW(name.as_ptr()) };
            println!("loaded={}", !module.is_null());
        }
        let window = hidden_window();
        let mut di: This = null_mut();
        // SAFETY: the game's DirectInput8Create, through its import table.
        let code = unsafe {
            DirectInput8Create(
                GetModuleHandleW(null()),
                0x0800,
                &IID_DIRECTINPUT8A,
                &mut di,
                null_mut(),
            )
        };
        println!("create={code:#x}");
        if code < 0 || di.is_null() {
            return;
        }
        println!("wrapped={}", module_of(di));
        let calls = export::<unsafe extern "system" fn() -> u32>("dinput8.dll", c"stand_in_calls");
        // SAFETY: the stand-in's counter.
        println!(
            "stand_in={}",
            calls.map_or("none".into(), |f| unsafe { f() }.to_string())
        );
        let mut found: Vec<DIDEVICEINSTANCEA> = Vec::new();
        type EnumDevices =
            unsafe extern "system" fn(This, u32, *const c_void, *mut c_void, u32) -> HRESULT;
        // SAFETY: IDirectInput8A::EnumDevices with a callback that pushes
        // to `found`.
        unsafe {
            method::<EnumDevices>(di, 4)(
                di,
                DI8DEVCLASS_GAMECTRL,
                collect as *const c_void,
                (&raw mut found).cast(),
                DIEDFL_ATTACHEDONLY,
            )
        };
        println!(
            "devices={}",
            found.iter().map(name).collect::<Vec<_>>().join("|")
        );
        let mut force: Vec<DIDEVICEINSTANCEA> = Vec::new();
        // SAFETY: as above, only devices with force feedback.
        unsafe {
            method::<EnumDevices>(di, 4)(
                di,
                DI8DEVCLASS_GAMECTRL,
                collect as *const c_void,
                (&raw mut force).cast(),
                DIEDFL_ATTACHEDONLY | DIEDFL_FORCEFEEDBACK,
            )
        };
        println!(
            "ffdevices={}",
            force.iter().map(name).collect::<Vec<_>>().join("|")
        );
        if let Some(first) = found.first().filter(|f| name(f) == "rea wheel") {
            // SAFETY: a live DirectInput and the wheel it listed.
            unsafe { drive(di, first, window, &steps) };
        }
        // SAFETY: our reference.
        unsafe { release(di) };
        println!("done");
    }

    /// The CMR sequence on the listed wheel: the device made, set up and
    /// acquired, each step printed.
    ///
    /// # Safety
    /// `di` is a live IDirectInput8A.
    unsafe fn open_device(di: This, wheel: &DIDEVICEINSTANCEA, window: HWND) -> Option<This> {
        type CreateDevice =
            unsafe extern "system" fn(This, *const GUID, *mut This, This) -> HRESULT;
        let mut device: This = null_mut();
        // SAFETY: the caller's contract; COM calls on the device made here
        // with structures that live across each call.
        unsafe {
            let code =
                method::<CreateDevice>(di, 3)(di, &wheel.guidInstance, &mut device, null_mut());
            println!("device={code:#x}");
            if code < 0 {
                return None;
            }
            let mut info = DIDEVICEINSTANCEA {
                dwSize: size_of::<DIDEVICEINSTANCEA>() as u32,
                ..std::mem::zeroed()
            };
            method::<unsafe extern "system" fn(This, *mut DIDEVICEINSTANCEA) -> HRESULT>(
                device, 15,
            )(device, &mut info);
            println!(
                "info=type:{:#x} product:{:08x}",
                info.dwDevType, info.guidProduct.data1
            );
            let mut caps = DIDEVCAPS {
                dwSize: size_of::<DIDEVCAPS>() as u32,
                ..std::mem::zeroed()
            };
            method::<unsafe extern "system" fn(This, *mut DIDEVCAPS) -> HRESULT>(device, 3)(
                device, &mut caps,
            );
            println!(
                "caps=axes:{} buttons:{} povs:{} ff:{}",
                caps.dwAxes,
                caps.dwButtons,
                caps.dwPOVs,
                caps.dwFlags & 0x100 != 0
            );
            let objects: Vec<DIOBJECTDATAFORMAT> = format::joystate2()
                .iter()
                .map(|e| DIOBJECTDATAFORMAT {
                    pguid: e.guid.map_or(null(), guid_ptr),
                    dwOfs: e.offset,
                    dwType: e.kind,
                    dwFlags: e.flags,
                })
                .collect();
            let df = DIDATAFORMAT {
                dwSize: size_of::<DIDATAFORMAT>() as u32,
                dwObjSize: size_of::<DIOBJECTDATAFORMAT>() as u32,
                dwFlags: DIDF_ABSAXIS,
                dwDataSize: size_of::<DIJOYSTATE2>() as u32,
                dwNumObjs: objects.len() as u32,
                rgodf: objects.as_ptr().cast_mut(),
            };
            let code = method::<unsafe extern "system" fn(This, *const DIDATAFORMAT) -> HRESULT>(
                device, 11,
            )(device, &df);
            println!("format={code:#x}");
            // Every axis at 0..255, addressed by its object ID (CMR04).
            let mut axes: Vec<u32> = Vec::new();
            type EnumObjects =
                unsafe extern "system" fn(This, *const c_void, *mut c_void, u32) -> HRESULT;
            method::<EnumObjects>(device, 4)(
                device,
                collect_axes as *const c_void,
                (&raw mut axes).cast(),
                DIDFT_AXIS,
            );
            let set_property = method::<SetProperty>(device, 6);
            let ranged = axes
                .iter()
                .filter(|&&id| {
                    let range = DIPROPRANGE {
                        diph: DIPROPHEADER {
                            dwSize: size_of::<DIPROPRANGE>() as u32,
                            dwHeaderSize: size_of::<DIPROPHEADER>() as u32,
                            dwObj: id,
                            dwHow: DIPH_BYID,
                        },
                        lMin: 0,
                        lMax: 255,
                    };
                    set_property(device, 4, &range.diph) >= 0
                })
                .count();
            println!("ranges={ranged}/{}", axes.len());
            let code = method::<unsafe extern "system" fn(This, HWND, u32) -> HRESULT>(device, 13)(
                device,
                window,
                DISCL_EXCLUSIVE | DISCL_FOREGROUND,
            );
            println!("cooperate={code:#x}");
            let code = method::<unsafe extern "system" fn(This) -> HRESULT>(device, 7)(device);
            println!("acquire={code:#x}");
            let mut gain = DIPROPDWORD {
                diph: dword_header(),
                dwData: 0,
            };
            type GetProperty = unsafe extern "system" fn(This, usize, *mut DIPROPHEADER) -> HRESULT;
            let code = method::<GetProperty>(device, 5)(device, 7, &mut gain.diph);
            println!("ffgain={code:#x},{}", gain.dwData);
            Some(device)
        }
    }

    /// Unacquires and releases a device.
    ///
    /// # Safety
    /// `device` is a live device of ours.
    unsafe fn close(device: This) {
        // SAFETY: the caller's contract.
        unsafe {
            method::<unsafe extern "system" fn(This) -> HRESULT>(device, 8)(device);
            release(device);
        }
    }

    /// The wheel opened, then the steps.
    ///
    /// # Safety
    /// `di` is a live IDirectInput8A.
    unsafe fn drive(di: This, wheel: &DIDEVICEINSTANCEA, window: HWND, steps: &[String]) {
        // SAFETY: the caller's contract; devices and effects made here.
        unsafe {
            let Some(device) = open_device(di, wheel, window) else {
                return;
            };
            let set_property = method::<SetProperty>(device, 6);
            let mut effects = Vec::new();
            for step in steps {
                if step == "second" || step == "second-drop" {
                    if let Some(other) = open_device(di, wheel, window) {
                        run_step(other, "read", set_property, &mut effects);
                        if step == "second" {
                            close(other);
                        } else {
                            release(other);
                        }
                    }
                    continue;
                }
                run_step(device, step, set_property, &mut effects);
            }
            for effect in effects {
                release(effect);
            }
            close(device);
        }
    }

    fn dword_header() -> DIPROPHEADER {
        DIPROPHEADER {
            dwSize: size_of::<DIPROPDWORD>() as u32,
            dwHeaderSize: size_of::<DIPROPHEADER>() as u32,
            dwObj: 0,
            dwHow: DIPH_DEVICE,
        }
    }

    /// # Safety
    /// `device` is a live, acquired virtual wheel.
    unsafe fn run_step(
        device: This,
        step: &str,
        set_property: unsafe extern "system" fn(This, usize, *const DIPROPHEADER) -> HRESULT,
        effects: &mut Vec<This>,
    ) {
        let parts: Vec<&str> = step.split(':').collect();
        let number = |i: usize| {
            parts
                .get(i)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0)
        };
        // SAFETY: the caller's contract; structures live across each call.
        unsafe {
            match parts[0] {
                "read" => {
                    method::<unsafe extern "system" fn(This) -> HRESULT>(device, 25)(device);
                    let mut state: DIJOYSTATE2 = std::mem::zeroed();
                    type GetState = unsafe extern "system" fn(This, u32, *mut c_void) -> HRESULT;
                    let code = method::<GetState>(device, 9)(
                        device,
                        size_of::<DIJOYSTATE2>() as u32,
                        (&raw mut state).cast(),
                    );
                    let buttons = (0..32).fold(0u32, |bits, n| {
                        bits | (u32::from(state.rgbButtons[n] & 0x80 != 0) << n)
                    });
                    println!(
                        "state={code:#x}:{},{},{},{} buttons={buttons:#x} pov={}",
                        state.lX, state.lY, state.lZ, state.lRz, state.rgdwPOV[0] as i32
                    );
                }
                "input" => {
                    let axes: Vec<i32> = parts[2]
                        .split(',')
                        .map(|v| v.parse().unwrap_or(0))
                        .collect();
                    let input = export::<unsafe extern "system" fn(u32, *const i32, u32, u32) -> i32>(
                        &layer(),
                        c"rea_dinput_fake_input",
                    );
                    if let Some(input) = input {
                        input(
                            number(1) as u32,
                            axes.as_ptr(),
                            number(3) as u32,
                            number(4) as u32,
                        );
                    }
                }
                "unplug" | "plug" => {
                    let unplug = export::<unsafe extern "system" fn(u32, i32) -> i32>(
                        &layer(),
                        c"rea_dinput_fake_unplug",
                    );
                    if let Some(unplug) = unplug {
                        unplug(number(1) as u32, i32::from(parts[0] == "unplug"));
                    }
                }
                "events" => {
                    let events = export::<unsafe extern "system" fn(*mut u8, u32) -> u32>(
                        &layer(),
                        c"rea_dinput_fake_events",
                    );
                    let mut buffer = vec![0u8; 1 << 16];
                    let len = events.map_or(0, |f| f(buffer.as_mut_ptr(), buffer.len() as u32));
                    for line in String::from_utf8_lossy(&buffer[..len as usize]).lines() {
                        println!("fake {line}");
                    }
                }
                "buffer" => {
                    let acquire = method::<unsafe extern "system" fn(This) -> HRESULT>(device, 7);
                    method::<unsafe extern "system" fn(This) -> HRESULT>(device, 8)(device);
                    let size = DIPROPDWORD {
                        diph: dword_header(),
                        dwData: number(1) as u32,
                    };
                    println!("buffer={:#x}", set_property(device, 1, &size.diph));
                    acquire(device);
                }
                "data" => {
                    let mut data: [DIDEVICEOBJECTDATA; 16] = std::mem::zeroed();
                    let mut count = data.len() as u32;
                    type GetData = unsafe extern "system" fn(
                        This,
                        u32,
                        *mut DIDEVICEOBJECTDATA,
                        *mut u32,
                        u32,
                    ) -> HRESULT;
                    let code = method::<GetData>(device, 10)(
                        device,
                        size_of::<DIDEVICEOBJECTDATA>() as u32,
                        data.as_mut_ptr(),
                        &mut count,
                        0,
                    );
                    let read = if code < 0 { 0 } else { count as usize };
                    let events: Vec<String> = data[..read]
                        .iter()
                        .map(|d| format!("{}={}", d.dwOfs, d.dwData))
                        .collect();
                    println!("data={code:#x}:{}", events.join(","));
                }
                "effects" => make_effects(device, set_property, effects),
                "update" => update(effects, number(1) as i32),
                "incomplete" => incomplete(device, effects),
                "reset" => {
                    let code = method::<unsafe extern "system" fn(This, u32) -> HRESULT>(
                        device, 22,
                    )(device, 1);
                    let started: Vec<String> = effects
                        .iter()
                        .map(|&e| format!("{:#x}", method::<Start>(e, 7)(e, 1, 0)))
                        .collect();
                    println!("reset={code:#x}:{}", started.join(","));
                }
                "unacquire" => {
                    let code =
                        method::<unsafe extern "system" fn(This) -> HRESULT>(device, 8)(device);
                    println!("unacquire={code:#x}");
                }
                "gain" => {
                    let value = DIPROPDWORD {
                        diph: dword_header(),
                        dwData: number(1) as u32,
                    };
                    let set = set_property(device, 7, &value.diph);
                    let mut read = DIPROPDWORD {
                        diph: dword_header(),
                        dwData: 0,
                    };
                    type GetProperty =
                        unsafe extern "system" fn(This, usize, *mut DIPROPHEADER) -> HRESULT;
                    method::<GetProperty>(device, 5)(device, 7, &mut read.diph);
                    println!("gain={set:#x},{}", read.dwData);
                }
                "autocenter" => {
                    let value = DIPROPDWORD {
                        diph: dword_header(),
                        dwData: number(1) as u32,
                    };
                    println!("autocenter={:#x}", set_property(device, 9, &value.diph));
                }
                "wait" => std::thread::sleep(std::time::Duration::from_millis(number(1) as u64)),
                other => println!("unknown step {other}"),
            }
        }
    }

    /// CMR's effects: autocentre off, a spring and a damper on both
    /// axes, a sine and a square wave and a constant force along X, each
    /// started; then the constant force changed in place.
    ///
    /// # Safety
    /// As [`run_step`].
    unsafe fn make_effects(
        device: This,
        set_property: unsafe extern "system" fn(This, usize, *const DIPROPHEADER) -> HRESULT,
        effects: &mut Vec<This>,
    ) {
        let condition = |coefficient| DICONDITION {
            lOffset: 0,
            lPositiveCoefficient: coefficient,
            lNegativeCoefficient: coefficient,
            dwPositiveSaturation: 10_000,
            dwNegativeSaturation: 10_000,
            lDeadBand: 0,
        };
        let conditions = [condition(8_000), condition(2_000)];
        let sine = DIPERIODIC {
            dwMagnitude: 6_000,
            lOffset: 0,
            dwPhase: 0,
            dwPeriod: 100_000,
        };
        let square = DIPERIODIC {
            dwMagnitude: 10_000,
            ..sine
        };
        let constant = DICONSTANTFORCE { lMagnitude: 6_000 };
        let mut axes = [0u32, 4];
        let mut direction = [1i32, 0];
        let (axes_ptr, direction_ptr) = (axes.as_mut_ptr(), direction.as_mut_ptr());
        let params = |data: *const c_void, size: usize| DIEFFECT {
            dwSize: size_of::<DIEFFECT>() as u32,
            dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
            dwDuration: u32::MAX,
            dwSamplePeriod: 0,
            dwGain: 10_000,
            dwTriggerButton: DIEB_NOTRIGGER,
            dwTriggerRepeatInterval: 0,
            cAxes: 2,
            rgdwAxes: axes_ptr,
            rglDirection: direction_ptr,
            lpEnvelope: null_mut(),
            cbTypeSpecificParams: size as u32,
            lpvTypeSpecificParams: data.cast_mut(),
            dwStartDelay: 0,
        };
        // SAFETY: the caller's contract; every structure lives across the
        // calls.
        unsafe {
            let off = DIPROPDWORD {
                diph: dword_header(),
                dwData: 0,
            };
            println!("autocenter={:#x}", set_property(device, 9, &off.diph));
            let made = [
                (
                    "spring",
                    &GUID_Spring,
                    params(conditions.as_ptr().cast(), size_of_val(&conditions)),
                ),
                (
                    "damper",
                    &GUID_Damper,
                    params(conditions.as_ptr().cast(), size_of_val(&conditions)),
                ),
                (
                    "sine",
                    &GUID_Sine,
                    params((&raw const sine).cast(), size_of::<DIPERIODIC>()),
                ),
                (
                    "square",
                    &GUID_Square,
                    params((&raw const square).cast(), size_of::<DIPERIODIC>()),
                ),
                (
                    "constant",
                    &GUID_ConstantForce,
                    params((&raw const constant).cast(), size_of::<DICONSTANTFORCE>()),
                ),
            ];
            for (name, guid, p) in &made {
                let mut effect: This = null_mut();
                let code =
                    method::<CreateEffect>(device, 18)(device, *guid, p, &mut effect, null_mut());
                let started = if effect.is_null() {
                    -1
                } else {
                    method::<Start>(effect, 7)(effect, 1, 0)
                };
                println!("effect={name},{code:#x},{started:#x}");
                if !effect.is_null() {
                    effects.push(effect);
                }
            }
            update(effects, -8_000);
        }
    }

    /// X and Y, cartesian along X, forever: CMR's effect parameters.
    struct Params {
        axes: [u32; 2],
        direction: [i32; 2],
        constant: DICONSTANTFORCE,
    }

    impl Params {
        fn constant(magnitude: i32) -> Box<Params> {
            Box::new(Params {
                axes: [0, 4],
                direction: [1, 0],
                constant: DICONSTANTFORCE {
                    lMagnitude: magnitude,
                },
            })
        }

        /// Valid while `self` lives.
        fn effect(&mut self) -> DIEFFECT {
            DIEFFECT {
                dwSize: size_of::<DIEFFECT>() as u32,
                dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
                dwDuration: u32::MAX,
                dwSamplePeriod: 0,
                dwGain: 10_000,
                dwTriggerButton: DIEB_NOTRIGGER,
                dwTriggerRepeatInterval: 0,
                cAxes: 2,
                rgdwAxes: self.axes.as_mut_ptr(),
                rglDirection: self.direction.as_mut_ptr(),
                lpEnvelope: null_mut(),
                cbTypeSpecificParams: size_of::<DICONSTANTFORCE>() as u32,
                lpvTypeSpecificParams: (&raw mut self.constant).cast(),
                dwStartDelay: 0,
            }
        }
    }

    /// The last effect (the constant force) changed in place.
    ///
    /// # Safety
    /// `effects` are live effects of ours.
    unsafe fn update(effects: &[This], magnitude: i32) {
        let mut params = Params::constant(magnitude);
        if let Some(&last) = effects.last() {
            // SAFETY: the caller's contract; `params` lives across the call.
            let code = unsafe {
                method::<SetParameters>(last, 6)(
                    last,
                    &params.effect(),
                    DIEP_TYPESPECIFICPARAMS | DIEP_NORESTART,
                )
            };
            println!("update={code:#x}");
        }
    }

    /// A constant force made without parameters, started, then given
    /// them.
    ///
    /// # Safety
    /// `device` is a live virtual wheel.
    unsafe fn incomplete(device: This, effects: &mut Vec<This>) {
        let mut effect: This = null_mut();
        let mut params = Params::constant(7_000);
        // SAFETY: the caller's contract; `params` lives across the calls.
        unsafe {
            let made = method::<CreateEffect>(device, 18)(
                device,
                &GUID_ConstantForce,
                null(),
                &mut effect,
                null_mut(),
            );
            if effect.is_null() {
                println!("incomplete={made:#x}");
                return;
            }
            let started = method::<Start>(effect, 7)(effect, 1, 0);
            let set = method::<SetParameters>(effect, 6)(
                effect,
                &params.effect(),
                DIEP_AXES | DIEP_DIRECTION | DIEP_DURATION | DIEP_TYPESPECIFICPARAMS,
            );
            println!("incomplete={made:#x},{started:#x},{set:#x}");
        }
        effects.push(effect);
    }
}
