//! The registry: an RAII `Key` with DWORD/string reads and writes.
//!
//! From CMR4 `win32.rs`, the only copy that closes keys on every path
//! (RAII), opens HKLM in its 32-bit view, and reports read errors instead
//! of treating them as "missing".
//!
//! Never count on the HKCU VirtualStore to give a game its values: the
//! game sees it only while UAC registry virtualization is on for the game
//! process, which Windows doesn't promise (rea-colin-mcrae#6). Write the
//! game's values to HKCU with [`write_verified`] and give the game that key
//! with `rea_close_hook`'s registry redirect (`REA_REGISTRY`), or write HKLM
//! with [`Key::machine`] and stop when it fails. [`Key::read_machine`] and
//! [`virtual_store_path`] stay for carrying the game's own old values over.
//!
//! The rule from `DESIGN.md` holds here: a value the player set is never
//! rewritten by the `*_if_missing` / [`seed_missing`] helpers.

use std::fmt;

/// A failed registry call: what was attempted and the Win32 error code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub what: String,
    pub code: u32,
}

impl Error {
    /// `ERROR_ACCESS_DENIED`.
    pub fn is_access_denied(&self) -> bool {
        self.code == 5
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (error {})", self.what, self.code)
    }
}

impl std::error::Error for Error {}

impl From<Error> for String {
    fn from(error: Error) -> String {
        error.to_string()
    }
}

/// Writes each default whose value is absent and leaves present ones
/// alone; true when anything was written.
pub fn seed_missing<V: Copy, E>(
    defaults: &[(&str, V)],
    mut has: impl FnMut(&str) -> Result<bool, E>,
    mut write: impl FnMut(&str, V) -> Result<(), E>,
) -> Result<bool, E> {
    let mut wrote = false;
    for &(name, value) in defaults {
        if !has(name)? {
            write(name, value)?;
            wrote = true;
        }
    }
    Ok(wrote)
}

/// A value's data, for [`write_verified`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Data {
    Dword(u32),
    /// `REG_SZ`.
    String(String),
}

impl fmt::Display for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Data::Dword(value) => write!(f, "{value}"),
            Data::String(text) => write!(f, "\"{text}\""),
        }
    }
}

/// When [`write_verified`] writes a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// The launcher's value (a folder path), written on every launch and
    /// read back exactly.
    Always,
    /// The game's own setting: seeded when absent, then the game's; read
    /// back as present with the right type.
    IfMissing,
}

/// One value for [`write_verified`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    pub name: String,
    pub data: Data,
    pub when: When,
}

impl Value {
    pub fn new(name: impl Into<String>, data: Data, when: When) -> Value {
        Value {
            name: name.into(),
            data,
            when,
        }
    }
}

/// What [`write_verified`] found wrong with one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub name: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The key or the value couldn't be written.
    NotWritten(Error),
    /// Read back, it isn't there.
    Missing,
    /// Read back, it holds something else.
    Differs { wanted: Data, found: Data },
    /// Reading it back failed (a wrong type included).
    Unreadable(Error),
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.name;
        match &self.problem {
            Problem::NotWritten(error) => write!(f, "{name} can't be written: {error}"),
            Problem::Missing => write!(f, "{name} is missing"),
            Problem::Differs { wanted, found } => write!(f, "{name} is {found}, not {wanted}"),
            Problem::Unreadable(error) => write!(f, "{name} can't be read: {error}"),
        }
    }
}

/// Compares `values` with what `read` gives back for each (by name, of the
/// value's type): [`When::Always`] values exactly, [`When::IfMissing`] ones
/// present. Empty when everything matches.
pub fn verify(
    values: &[Value],
    mut read: impl FnMut(&Value) -> Result<Option<Data>, Error>,
) -> Vec<Mismatch> {
    values
        .iter()
        .filter_map(|value| {
            let problem = match read(value) {
                Ok(None) => Problem::Missing,
                Ok(Some(_)) if value.when == When::IfMissing => return None,
                Ok(Some(found)) if found == value.data => return None,
                Ok(Some(found)) => Problem::Differs {
                    wanted: value.data.clone(),
                    found,
                },
                Err(error) => Problem::Unreadable(error),
            };
            Some(Mismatch {
                name: value.name.clone(),
                problem,
            })
        })
        .collect()
}

/// Where a 32-bit game reads `HKLM\<machine_path>` from when its HKLM
/// write was virtualised: `HKCU\Software\Classes\VirtualStore\MACHINE\…`,
/// with `WOW6432Node` after `SOFTWARE` on a 64-bit Windows.
pub fn virtual_store_path(machine_path: &str, wow64: bool) -> String {
    let path = machine_path.trim_matches('\\');
    let rest = path
        .get(..9)
        .filter(|head| head.eq_ignore_ascii_case("SOFTWARE\\"))
        .map(|_| &path[9..]);
    match rest {
        Some(rest)
            if wow64
                && !rest
                    .get(..12)
                    .is_some_and(|node| node.eq_ignore_ascii_case("WOW6432Node\\")) =>
        {
            format!(r"Software\Classes\VirtualStore\MACHINE\SOFTWARE\WOW6432Node\{rest}")
        }
        _ => format!(r"Software\Classes\VirtualStore\MACHINE\{path}"),
    }
}

/// Whether an `HKLM\<path>` key is opened in the 32-bit registry view
/// (`KEY_WOW64_32KEY`): only under `SOFTWARE`, the redirected subtree the
/// 32-bit games read. Elsewhere (e.g. Wine's winebus key under `System`)
/// no view flag is passed, as CMR4/SRR/OR open it.
pub fn uses_32_bit_view(machine: bool, path: &str) -> bool {
    let path = path.trim_start_matches('\\');
    machine
        && path
            .get(..8)
            .is_some_and(|head| head.eq_ignore_ascii_case("SOFTWARE"))
        && matches!(path.as_bytes().get(8), None | Some(b'\\'))
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_CONFIG, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE,
        KEY_READ, KEY_SET_VALUE, KEY_WOW64_32KEY, REG_DWORD, REG_EXPAND_SZ,
        REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };

    use super::{Data, Error, Mismatch, Problem, Value, When};
    use crate::wide::{from_wide, wide};

    /// A registry root.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Hive {
        CurrentUser,
        /// `SOFTWARE\…` opens in its 32-bit view (`KEY_WOW64_32KEY`), where
        /// the 32-bit games read, whatever the launcher's own bitness;
        /// other paths open without a view flag
        /// ([`uses_32_bit_view`](super::uses_32_bit_view)).
        LocalMachine,
        CurrentConfig,
    }

    /// Where the deprecated [`Key::machine_or_virtual_store`] opened the
    /// key.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Place {
        /// `HKLM`, in its 32-bit view for `SOFTWARE` paths (the game keys
        /// [`Key::machine_or_virtual_store`] is for).
        Machine,
        /// `HKCU\Software\Classes\VirtualStore\MACHINE\…` (no elevation on
        /// Windows; the 32-bit game reads it transparently).
        VirtualStore,
    }

    impl Place {
        /// The label the launchers log (CMR4's): where the game's values
        /// went.
        pub fn describe(self) -> &'static str {
            match self {
                Place::Machine => "HKLM 32-bit view",
                Place::VirtualStore => "HKCU VirtualStore",
            }
        }
    }

    /// An open registry key, closed on drop.
    #[derive(Debug)]
    pub struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns one open key and closes it once.
            unsafe {
                RegCloseKey(self.0);
            }
        }
    }

    impl Key {
        /// Opens `path` under `hive` for reading and writing, creating it
        /// when absent. To only read, use [`Key::open_read`] or
        /// [`Key::read_machine`], which never create a key.
        pub fn create(hive: Hive, path: &str) -> Result<Key, Error> {
            let (root, view) = root(hive, path);
            let mut key: HKEY = null_mut();
            // SAFETY: the output points to local storage and the path is
            // NUL-terminated.
            let code = unsafe {
                RegCreateKeyExW(
                    root,
                    wide(path).as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_QUERY_VALUE | KEY_SET_VALUE | view,
                    null(),
                    &mut key,
                    null_mut(),
                )
            };
            if code == ERROR_SUCCESS {
                Ok(Key(key))
            } else {
                Err(Error {
                    what: format!("cannot open {hive:?}\\{path}"),
                    code,
                })
            }
        }

        /// `HKLM\<path>`, or its HKCU VirtualStore twin when HKLM is
        /// access-denied (native Windows without elevation). Any other
        /// error is returned as it is.
        ///
        /// Unsafe for a game's values: the game reads the VirtualStore copy
        /// only while UAC registry virtualization is on for its process,
        /// which Windows doesn't promise (off by policy, per key, and always
        /// off for an exe with a `requestedExecutionLevel` manifest;
        /// rea-colin-mcrae#6).
        #[deprecated(
            since = "0.6.0",
            note = "the game may never see the VirtualStore: write HKCU with reg::write_verified \
                    and give the game that key with rea_close_hook's REA_REGISTRY redirect, \
                    or use Key::machine (no fallback)"
        )]
        pub fn machine_or_virtual_store(path: &str) -> Result<(Key, Place), Error> {
            match Key::create(Hive::LocalMachine, path) {
                Ok(key) => Ok((key, Place::Machine)),
                Err(error) if error.is_access_denied() => {
                    let store = super::virtual_store_path(path, windows_is_64_bit());
                    Ok((Key::create(Hive::CurrentUser, &store)?, Place::VirtualStore))
                }
                Err(error) => Err(error),
            }
        }

        /// `HKLM\<path>` for reading and writing (32-bit view under
        /// `SOFTWARE`), created when absent. Fails without administrator
        /// rights on Windows ([`Error::is_access_denied`]); never falls
        /// back to the VirtualStore. Under Wine it always works.
        pub fn machine(path: &str) -> Result<Key, Error> {
            Key::create(Hive::LocalMachine, path)
        }

        /// Opens an existing key read-only (`KEY_READ`); `None` when it does
        /// not exist. Creates nothing: reading through [`Key::create`] on
        /// non-admin Windows would leave empty keys in the VirtualStore.
        pub fn open_read(hive: Hive, path: &str) -> Result<Option<Key>, Error> {
            let (root, view) = root(hive, path);
            let mut key: HKEY = null_mut();
            // SAFETY: the output points to local storage and the path is
            // NUL-terminated.
            let code =
                unsafe { RegOpenKeyExW(root, wide(path).as_ptr(), 0, KEY_READ | view, &mut key) };
            match code {
                ERROR_SUCCESS => Ok(Some(Key(key))),
                ERROR_FILE_NOT_FOUND => Ok(None),
                code => Err(Error {
                    what: format!("cannot read {hive:?}\\{path}"),
                    code,
                }),
            }
        }

        /// What a 32-bit game reads at `HKLM\<path>`: each value from the
        /// HKCU VirtualStore when it is there, else from HKLM. Both are
        /// opened read-only; nothing is created.
        pub fn read_machine(path: &str) -> Result<MachineValues, Error> {
            let store = super::virtual_store_path(path, windows_is_64_bit());
            Ok(MachineValues {
                store: Key::open_read(Hive::CurrentUser, &store)?,
                machine: Key::open_read(Hive::LocalMachine, path)?,
            })
        }

        /// Size and type of a value; `None` when it is absent.
        fn query(&self, name: &str) -> Result<Option<(u32, u32)>, Error> {
            let name_wide = wide(name);
            let (mut kind, mut size) = (0u32, 0u32);
            // SAFETY: an open key; only the type and size are asked for.
            let code = unsafe {
                RegQueryValueExW(
                    self.0,
                    name_wide.as_ptr(),
                    null(),
                    &mut kind,
                    null_mut(),
                    &mut size,
                )
            };
            match code {
                ERROR_SUCCESS => Ok(Some((kind, size))),
                ERROR_FILE_NOT_FOUND => Ok(None),
                code => Err(Error {
                    what: format!("cannot read registry value {name}"),
                    code,
                }),
            }
        }

        /// Whether the value `name` exists.
        pub fn has(&self, name: &str) -> Result<bool, Error> {
            Ok(self.query(name)?.is_some())
        }

        /// A DWORD value; `None` when absent, an error when it has another type.
        pub fn dword(&self, name: &str) -> Result<Option<u32>, Error> {
            let name_wide = wide(name);
            let (mut kind, mut value, mut size) = (0u32, 0u32, 4u32);
            // SAFETY: an open key; the output is a local u32 of `size` bytes.
            let code = unsafe {
                RegQueryValueExW(
                    self.0,
                    name_wide.as_ptr(),
                    null(),
                    &mut kind,
                    (&raw mut value).cast(),
                    &mut size,
                )
            };
            match code {
                ERROR_SUCCESS if kind == REG_DWORD && size == 4 => Ok(Some(value)),
                ERROR_FILE_NOT_FOUND => Ok(None),
                // A wrong type with a larger size reports ERROR_MORE_DATA.
                ERROR_SUCCESS | 234 => Err(Error {
                    what: format!("registry value {name} is not a DWORD"),
                    code: 13, // ERROR_INVALID_DATA
                }),
                code => Err(Error {
                    what: format!("cannot read registry value {name}"),
                    code,
                }),
            }
        }

        /// A string (`REG_SZ`, or `REG_EXPAND_SZ` unexpanded); `None` when absent.
        pub fn string(&self, name: &str) -> Result<Option<String>, Error> {
            let Some((kind, size)) = self.query(name)? else {
                return Ok(None);
            };
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return Err(Error {
                    what: format!("registry value {name} is not a string"),
                    code: 13,
                });
            }
            let name_wide = wide(name);
            let mut buffer = vec![0u16; size as usize / 2 + 1];
            let mut bytes = (buffer.len() * 2) as u32;
            // SAFETY: an open key; the buffer holds `bytes` bytes.
            let code = unsafe {
                RegQueryValueExW(
                    self.0,
                    name_wide.as_ptr(),
                    null(),
                    null_mut(),
                    buffer.as_mut_ptr().cast(),
                    &mut bytes,
                )
            };
            match code {
                ERROR_SUCCESS => Ok(Some(
                    from_wide(&buffer[..(bytes as usize / 2).min(buffer.len())])
                        .to_string_lossy()
                        .into_owned(),
                )),
                ERROR_FILE_NOT_FOUND => Ok(None),
                code => Err(Error {
                    what: format!("cannot read registry value {name}"),
                    code,
                }),
            }
        }

        pub fn set_dword(&self, name: &str, value: u32) -> Result<(), Error> {
            let name_wide = wide(name);
            let data = value.to_le_bytes();
            // SAFETY: an open key; `data` is four readable bytes.
            let code = unsafe {
                RegSetValueExW(self.0, name_wide.as_ptr(), 0, REG_DWORD, data.as_ptr(), 4)
            };
            check(code, name)
        }

        /// Deletes the value `name`; false when it was not there.
        pub fn delete_value(&self, name: &str) -> Result<bool, Error> {
            let name_wide = wide(name);
            // SAFETY: an open key and a NUL-terminated name.
            match unsafe { RegDeleteValueW(self.0, name_wide.as_ptr()) } {
                ERROR_SUCCESS => Ok(true),
                ERROR_FILE_NOT_FOUND => Ok(false),
                code => Err(Error {
                    what: format!("cannot delete registry value {name}"),
                    code,
                }),
            }
        }

        pub fn set_string(&self, name: &str, value: &str) -> Result<(), Error> {
            let name_wide = wide(name);
            let data = wide(value);
            // SAFETY: an open key; `data` is NUL-terminated and its byte
            // length includes the terminator.
            let code = unsafe {
                RegSetValueExW(
                    self.0,
                    name_wide.as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            check(code, name)
        }

        /// Writes `value` only when `name` is absent; true when it wrote.
        pub fn set_dword_if_missing(&self, name: &str, value: u32) -> Result<bool, Error> {
            self.seed_dwords(&[(name, value)])
        }

        /// Writes `value` only when `name` is absent; true when it wrote.
        pub fn set_string_if_missing(&self, name: &str, value: &str) -> Result<bool, Error> {
            super::seed_missing(
                &[(name, value)],
                |name| self.has(name),
                |name, value| self.set_string(name, value),
            )
        }

        /// [`super::seed_missing`] for DWORDs on this key.
        pub fn seed_dwords(&self, defaults: &[(&str, u32)]) -> Result<bool, Error> {
            super::seed_missing(
                defaults,
                |name| self.has(name),
                |name, value| self.set_dword(name, value),
            )
        }
    }

    /// Writes `values` to `hive\<path>` (creating the key; [`When::IfMissing`]
    /// values only when absent), then reopens the key read-only and reads
    /// each one back ([`super::verify`]). `Err` lists every value that is
    /// wrong, so `--check` and Play can stop with the reason instead of
    /// starting a game that won't find its folders.
    pub fn write_verified(hive: Hive, path: &str, values: &[Value]) -> Result<(), Vec<Mismatch>> {
        // The same problem for every value (the key itself failed).
        let every = |problem: Problem| {
            values
                .iter()
                .map(|value| Mismatch {
                    name: value.name.clone(),
                    problem: problem.clone(),
                })
                .collect::<Vec<_>>()
        };
        {
            let key = Key::create(hive, path).map_err(|error| every(Problem::NotWritten(error)))?;
            let mut failed = Vec::new();
            for value in values {
                let written = match (&value.data, value.when) {
                    (Data::Dword(data), When::Always) => key.set_dword(&value.name, *data),
                    (Data::String(data), When::Always) => key.set_string(&value.name, data),
                    (Data::Dword(data), When::IfMissing) => {
                        key.set_dword_if_missing(&value.name, *data).map(drop)
                    }
                    (Data::String(data), When::IfMissing) => {
                        key.set_string_if_missing(&value.name, data).map(drop)
                    }
                };
                if let Err(error) = written {
                    failed.push(Mismatch {
                        name: value.name.clone(),
                        problem: Problem::NotWritten(error),
                    });
                }
            }
            if !failed.is_empty() {
                return Err(failed);
            }
        }
        let key = match Key::open_read(hive, path) {
            Ok(Some(key)) => key,
            Ok(None) => return Err(every(Problem::Missing)),
            Err(error) => return Err(every(Problem::Unreadable(error))),
        };
        let wrong = super::verify(values, |value| match value.data {
            Data::Dword(_) => key.dword(&value.name).map(|v| v.map(Data::Dword)),
            Data::String(_) => key.string(&value.name).map(|v| v.map(Data::String)),
        });
        if wrong.is_empty() { Ok(()) } else { Err(wrong) }
    }

    /// The values a 32-bit game reads under one HKLM key; from
    /// [`Key::read_machine`].
    #[derive(Debug)]
    pub struct MachineValues {
        store: Option<Key>,
        machine: Option<Key>,
    }

    impl MachineValues {
        /// Whether the key exists in either place.
        pub fn exists(&self) -> bool {
            self.store.is_some() || self.machine.is_some()
        }

        pub fn dword(&self, name: &str) -> Result<Option<u32>, Error> {
            self.first(|key| key.dword(name))
        }

        pub fn string(&self, name: &str) -> Result<Option<String>, Error> {
            self.first(|key| key.string(name))
        }

        fn first<T>(
            &self,
            read: impl Fn(&Key) -> Result<Option<T>, Error>,
        ) -> Result<Option<T>, Error> {
            for key in [&self.store, &self.machine].into_iter().flatten() {
                if let Some(value) = read(key)? {
                    return Ok(Some(value));
                }
            }
            Ok(None)
        }
    }

    fn root(hive: Hive, path: &str) -> (HKEY, u32) {
        match hive {
            Hive::CurrentUser => (HKEY_CURRENT_USER, 0),
            Hive::LocalMachine if super::uses_32_bit_view(true, path) => {
                (HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY)
            }
            Hive::LocalMachine => (HKEY_LOCAL_MACHINE, 0),
            Hive::CurrentConfig => (HKEY_CURRENT_CONFIG, 0),
        }
    }

    fn check(code: u32, name: &str) -> Result<(), Error> {
        if code == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(Error {
                what: format!("cannot write registry value {name}"),
                code,
            })
        }
    }

    /// Whether Windows itself is 64-bit (a 32-bit launcher then runs under
    /// WOW64 and HKLM\SOFTWARE writes land in `WOW6432Node`).
    pub fn windows_is_64_bit() -> bool {
        if cfg!(target_pointer_width = "64") {
            return true;
        }
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process};
        let mut wow64 = 0;
        // SAFETY: the pseudo-handle needs no closing; the output is local.
        unsafe { IsWow64Process(GetCurrentProcess(), &mut wow64) != 0 && wow64 != 0 }
    }
}

/// Run on Windows or under Wine (`scripts/wine-test`).
#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn places_are_labelled_as_the_launchers_log_them() {
        assert_eq!(Place::Machine.describe(), "HKLM 32-bit view");
        assert_eq!(Place::VirtualStore.describe(), "HKCU VirtualStore");
    }

    fn fresh(name: &str) -> String {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!(
            r"Software\rea-launcher-sdk-test\{name}-{}-{stamp}",
            std::process::id()
        )
    }

    #[test]
    fn write_verified_writes_reads_back_and_keeps_the_games_values() {
        let path = fresh("verified");
        // The game already chose its language.
        Key::create(Hive::CurrentUser, &path)
            .unwrap()
            .set_string("Language", "GERMAN")
            .unwrap();
        let values = [
            Value::new(
                "HDPath",
                Data::String(r"C:\Jogos d'Ação\CMR".into()),
                When::Always,
            ),
            Value::new("Width", Data::Dword(1920), When::Always),
            Value::new("Language", Data::String("ENGLISH".into()), When::IfMissing),
            Value::new("Hardware", Data::String("5".into()), When::IfMissing),
        ];
        assert_eq!(write_verified(Hive::CurrentUser, &path, &values), Ok(()));
        let key = Key::open_read(Hive::CurrentUser, &path).unwrap().unwrap();
        assert_eq!(key.string("Language").unwrap().as_deref(), Some("GERMAN"));
        assert_eq!(key.string("Hardware").unwrap().as_deref(), Some("5"));
        assert_eq!(
            key.string("HDPath").unwrap().as_deref(),
            Some(r"C:\Jogos d'Ação\CMR")
        );
        // A second launch: the same, nothing to report.
        assert_eq!(write_verified(Hive::CurrentUser, &path, &values), Ok(()));
    }

    #[test]
    fn write_verified_reports_a_value_it_cant_read_back() {
        let path = fresh("wrong-type");
        // The game left a string where the launcher writes only if missing
        // a DWORD: present, but the read-back fails on the type.
        Key::create(Hive::CurrentUser, &path)
            .unwrap()
            .set_string("Width", "wide")
            .unwrap();
        let values = [Value::new("Width", Data::Dword(1920), When::IfMissing)];
        let wrong = write_verified(Hive::CurrentUser, &path, &values).unwrap_err();
        assert_eq!(wrong.len(), 1);
        assert!(
            matches!(wrong[0].problem, Problem::Unreadable(_)),
            "{:?}",
            wrong
        );
    }

    #[test]
    fn machine_opens_hklm_without_a_fallback() {
        // Wine runs as an administrator: HKLM is writable, and the value
        // lands in HKLM (32-bit view), not in a VirtualStore.
        let path = fresh("machine").replacen("Software", "SOFTWARE", 1);
        Key::machine(&path).unwrap().set_dword("X", 7).unwrap();
        let machine = Key::open_read(Hive::LocalMachine, &path).unwrap().unwrap();
        assert_eq!(machine.dword("X").unwrap(), Some(7));
        let store = virtual_store_path(&path, windows_is_64_bit());
        assert!(Key::open_read(Hive::CurrentUser, &store).unwrap().is_none());
    }

    #[test]
    fn reading_never_creates_keys() {
        let path = r"Software\rea-launcher-sdk-test\never-created";
        assert!(Key::open_read(Hive::CurrentUser, path).unwrap().is_none());
        let values = Key::read_machine(r"SOFTWARE\rea-launcher-sdk-test\missing").unwrap();
        assert!(!values.exists());
        assert_eq!(values.dword("Width").unwrap(), None);
        // Still absent after the reads.
        assert!(Key::open_read(Hive::CurrentUser, path).unwrap().is_none());
    }

    #[test]
    fn written_values_read_back_and_seed_only_when_missing() {
        // A fresh key each run: the test prefix keeps what earlier runs wrote.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = format!(
            r"Software\rea-launcher-sdk-test\values-{}-{stamp}",
            std::process::id()
        );
        let path = path.as_str();
        let key = Key::create(Hive::CurrentUser, path).unwrap();
        key.set_dword("Width", 1920).unwrap();
        key.set_string("Path", r"C:\Games\Rally").unwrap();
        assert!(!key.set_dword_if_missing("Width", 640).unwrap());
        assert!(key.set_string_if_missing("Lang", "E").unwrap());
        let read = Key::open_read(Hive::CurrentUser, path).unwrap().unwrap();
        assert_eq!(read.dword("Width").unwrap(), Some(1920));
        assert_eq!(
            read.string("Path").unwrap().as_deref(),
            Some(r"C:\Games\Rally")
        );
        assert_eq!(read.string("Lang").unwrap().as_deref(), Some("E"));
        assert!(read.dword("Path").is_err(), "a string is not a DWORD");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_missing_keeps_present_values() {
        let present = ["Map Controllers"];
        let mut written = Vec::new();
        let wrote = seed_missing::<u32, ()>(
            &[("Map Controllers", 1), ("DisableHidraw", 1)],
            |name| Ok(present.contains(&name)),
            |name, value| {
                written.push((name.to_owned(), value));
                Ok(())
            },
        )
        .unwrap();
        assert!(wrote);
        assert_eq!(written, [("DisableHidraw".to_owned(), 1)]);
        let nothing = seed_missing::<u32, ()>(&[("a", 1)], |_| Ok(true), |_, _| panic!("wrote"));
        assert_eq!(nothing, Ok(false));
    }

    #[test]
    fn seed_missing_stops_at_the_first_error() {
        let result = seed_missing(&[("a", 1u32)], |_| Ok(false), |_, _| Err("denied"));
        assert_eq!(result, Err("denied"));
    }

    #[test]
    fn virtual_store_paths() {
        let cmr4 = r"SOFTWARE\Codemasters\Colin McRae Rally 04";
        assert_eq!(
            virtual_store_path(cmr4, true),
            r"Software\Classes\VirtualStore\MACHINE\SOFTWARE\WOW6432Node\Codemasters\Colin McRae Rally 04"
        );
        assert_eq!(
            virtual_store_path(r"SOFTWARE\Sega\SEGA Rally", false),
            r"Software\Classes\VirtualStore\MACHINE\SOFTWARE\Sega\SEGA Rally"
        );
        // Already in the 32-bit node, or outside SOFTWARE: unchanged.
        assert_eq!(
            virtual_store_path(r"Software\WOW6432Node\X", true),
            r"Software\Classes\VirtualStore\MACHINE\Software\WOW6432Node\X"
        );
        assert_eq!(
            virtual_store_path(r"\System\X\", true),
            r"Software\Classes\VirtualStore\MACHINE\System\X"
        );
    }

    #[test]
    fn only_hklm_software_uses_the_32_bit_view() {
        assert!(uses_32_bit_view(
            true,
            r"SOFTWARE\Codemasters\Colin McRae Rally 04"
        ));
        assert!(uses_32_bit_view(true, r"\software\Sega"));
        assert!(uses_32_bit_view(true, "Software"));
        // Wine's bus key: no view flag, as CMR4/SRR/OR open it.
        assert!(!uses_32_bit_view(
            true,
            r"System\CurrentControlSet\Services\winebus"
        ));
        assert!(!uses_32_bit_view(true, r"SOFTWAREX\a"));
        assert!(!uses_32_bit_view(false, r"Software\Wine\X11 Driver"));
    }

    #[test]
    fn verify_compares_ours_exactly_and_the_games_by_presence() {
        let values = [
            Value::new("Path", Data::String(r"C:\Games\X".into()), When::Always),
            Value::new("Width", Data::Dword(1920), When::Always),
            Value::new("Language", Data::String("ENGLISH".into()), When::IfMissing),
            Value::new("Hardware", Data::Dword(5), When::IfMissing),
            Value::new("Broken", Data::Dword(1), When::Always),
        ];
        let read = |value: &Value| match value.name.as_str() {
            "Path" => Ok(Some(Data::String(r"C:\Old".into()))),
            "Width" => Ok(Some(Data::Dword(1920))),
            "Language" => Ok(Some(Data::String("GERMAN".into()))),
            "Hardware" => Ok(None),
            _ => Err(Error {
                what: "cannot read registry value Broken".into(),
                code: 13,
            }),
        };
        let wrong = verify(&values, read);
        let text: Vec<String> = wrong.iter().map(ToString::to_string).collect();
        assert_eq!(
            text,
            [
                r#"Path is "C:\Old", not "C:\Games\X""#,
                "Hardware is missing",
                "Broken can't be read: cannot read registry value Broken (error 13)",
            ]
        );
        assert!(verify(&values[1..2], read).is_empty());
        let denied = Mismatch {
            name: "Path".into(),
            problem: Problem::NotWritten(Error {
                what: "cannot open LocalMachine\\SOFTWARE\\X".into(),
                code: 5,
            }),
        };
        assert_eq!(
            denied.to_string(),
            "Path can't be written: cannot open LocalMachine\\SOFTWARE\\X (error 5)"
        );
    }

    #[test]
    fn errors_read_well_and_convert_to_string() {
        let error = Error {
            what: "cannot open X".into(),
            code: 5,
        };
        assert!(error.is_access_denied());
        assert_eq!(String::from(error), "cannot open X (error 5)");
    }
}
