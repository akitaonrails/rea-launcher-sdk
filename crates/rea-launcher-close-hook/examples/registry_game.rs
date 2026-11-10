//! A stand-in for an old game's registry reads, for the redirect's tests
//! (`tests/registry.rs`): it opens keys under `HKLM\SOFTWARE\<base>` the
//! ways the games do (CMR4/CMR2005: `RegCreateKeyExA` of the full path,
//! then reads and writes; SRR: `RegOpenKeyExA`; CMR1: `SOFTWARE` from HKLM,
//! then one level at a time) and prints `name=value` lines of what it read
//! (`-` for nothing). `<base>` is the first argument.

#[cfg(windows)]
fn main() {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS, KEY_READ, REG_SZ, RegCloseKey, RegCreateKeyA,
        RegCreateKeyExA, RegOpenKeyA, RegOpenKeyExA, RegOpenKeyExW, RegQueryValueExA,
        RegSetValueExA,
    };

    let base = std::env::args().nth(1).expect("the base key");
    let c = |text: &str| format!("{text}\0");

    // SAFETY: registry calls on NUL-terminated names, local outputs and
    // handles this function opened.
    unsafe {
        let read = |key: HKEY| -> String {
            let mut data = [0u8; 260];
            let mut size = data.len() as u32;
            let code = RegQueryValueExA(
                key,
                c"Path".as_ptr().cast(),
                null(),
                null_mut(),
                data.as_mut_ptr(),
                &mut size,
            );
            if code != 0 {
                return "-".into();
            }
            let text = &data[..size as usize];
            String::from_utf8_lossy(text.strip_suffix(&[0]).unwrap_or(text)).into_owned()
        };
        let open_ex = |root: HKEY, path: &str| -> Option<HKEY> {
            let mut key = null_mut();
            (RegOpenKeyExA(root, c(path).as_ptr(), 0, KEY_READ, &mut key) == 0).then_some(key)
        };
        let show = |name: &str, key: Option<HKEY>| {
            println!("{name}={}", key.map_or("-".into(), read));
            if let Some(key) = key {
                RegCloseKey(key);
            }
        };

        // SRR: one open of the full path.
        show(
            "full",
            open_ex(HKEY_LOCAL_MACHINE, &format!(r"SOFTWARE\{base}\Game")),
        );
        // A key no rule names.
        show(
            "other",
            open_ex(HKEY_LOCAL_MACHINE, &format!(r"SOFTWARE\{base}\Other")),
        );
        // The wide call.
        let wide: Vec<u16> = format!(r"SOFTWARE\{base}\Game")
            .encode_utf16()
            .chain([0])
            .collect();
        let mut key = null_mut();
        let opened = RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide.as_ptr(), 0, KEY_READ, &mut key) == 0;
        show("wide", opened.then_some(key));

        // CMR4/CMR2005: create with full access, then write a value.
        let mut key = null_mut();
        let created = RegCreateKeyExA(
            HKEY_LOCAL_MACHINE,
            c(&format!(r"SOFTWARE\{base}\Game")).as_ptr(),
            0,
            null(),
            0,
            KEY_ALL_ACCESS,
            null(),
            &mut key,
            null_mut(),
        ) == 0;
        let wrote = created
            && RegSetValueExA(
                key,
                c"Written".as_ptr().cast(),
                0,
                REG_SZ,
                c"yes".as_ptr().cast(),
                4,
            ) == 0;
        println!("create={}", if wrote { "ok" } else { "-" });
        if created {
            RegCloseKey(key);
        }
        // RegCreateKeyA of a key below it.
        let mut key = null_mut();
        let sub = RegCreateKeyA(
            HKEY_LOCAL_MACHINE,
            c(&format!(r"SOFTWARE\{base}\Game\Sub")).as_ptr(),
            &mut key,
        ) == 0;
        println!("create_sub={}", if sub { "ok" } else { "-" });
        if sub {
            RegCloseKey(key);
        }

        // CMR1: SOFTWARE, then one level at a time; RegOpenKeyA last.
        let software = open_ex(HKEY_LOCAL_MACHINE, "SOFTWARE");
        let level = software.and_then(|software| open_ex(software, &base));
        let walk = level.and_then(|level| {
            let mut key = null_mut();
            (RegOpenKeyA(level, c"Walk".as_ptr().cast(), &mut key) == 0).then_some(key)
        });
        show("walk", walk);
        // Off the walked path, from the same SOFTWARE handle.
        show(
            "walk_off",
            software.and_then(|software| open_ex(software, &format!(r"{base}\Other"))),
        );
        for key in [level, software].into_iter().flatten() {
            RegCloseKey(key);
        }
    }
}

#[cfg(not(windows))]
fn main() {}
