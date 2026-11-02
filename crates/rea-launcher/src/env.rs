//! Child-process environments.
//!
//! From CMR4's `environment_block` (adopted from the win32 crate): setting
//! `WINEDLLOVERRIDES` in the game's own environment block works without the
//! Linux shell helper (plain Proton, a double-clicked exe). Pure; the
//! win32 crate passes the block to `CreateProcessW`.

use std::ffi::{OsStr, OsString};

/// The value of `name` in `vars`, compared without regard to case as
/// Windows does (Wine's environment may hold `WineDllOverrides`); the
/// last match wins, as with duplicates in a real environment block.
pub fn lookup<'a>(vars: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    vars.iter()
        .rev()
        .find(|(n, _)| n.to_string_lossy().eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_os_str())
}

/// `base` (normally `std::env::vars_os()`) with `changes` applied: a value
/// sets the variable, `None` removes it. Names compare case-insensitively,
/// as Windows does, and the result is sorted the way `CreateProcessW`
/// expects its block.
pub fn merge_env(
    base: impl IntoIterator<Item = (OsString, OsString)>,
    changes: &[(OsString, Option<OsString>)],
) -> Vec<(OsString, OsString)> {
    let same = |a: &OsStr, b: &OsStr| {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    };
    let mut vars: Vec<(OsString, OsString)> = base
        .into_iter()
        .filter(|(name, _)| !changes.iter().any(|(changed, _)| same(name, changed)))
        .collect();
    for (name, value) in changes {
        // A later change to the same name wins.
        vars.retain(|(existing, _)| !same(existing, name));
        if let Some(value) = value {
            vars.push((name.clone(), value.clone()));
        }
    }
    vars.sort_by_key(|(name, _)| name.to_string_lossy().to_uppercase());
    vars
}

/// A `CREATE_UNICODE_ENVIRONMENT` block: `NAME=value\0` each, then `\0`.
pub fn environment_block(vars: &[(OsString, OsString)]) -> Vec<u16> {
    let mut block = Vec::new();
    for (name, value) in vars {
        block.extend(encode(name));
        block.push(u16::from(b'='));
        block.extend(encode(value));
        block.push(0);
    }
    if vars.is_empty() {
        // An empty block still needs its own terminator after the first NUL.
        block.push(0);
    }
    block.push(0);
    block
}

#[cfg(windows)]
fn encode(text: &OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    text.encode_wide().collect()
}

#[cfg(not(windows))]
fn encode(text: &OsStr) -> Vec<u16> {
    text.to_string_lossy().encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        list.iter()
            .map(|(n, v)| ((*n).into(), (*v).into()))
            .collect()
    }

    #[test]
    fn lookup_ignores_case_and_takes_the_last() {
        let env = vars(&[
            ("WineDllOverrides", "a=b"),
            ("PATH", "x"),
            ("WINEDLLOVERRIDES", "c=d"),
        ]);
        assert_eq!(lookup(&env, "winedlloverrides"), Some(OsStr::new("c=d")));
        assert_eq!(lookup(&env, "HOME"), None);
    }

    #[test]
    fn env_changes_replace_case_insensitively_and_sort() {
        let base = vars(&[("Path", r"C:\w"), ("winedlloverrides", "x=b"), ("A", "1")]);
        let merged = merge_env(
            base,
            &[
                ("WINEDLLOVERRIDES".into(), Some("d3d9=n".into())),
                ("a".into(), None),
            ],
        );
        assert_eq!(
            merged,
            vars(&[("Path", r"C:\w"), ("WINEDLLOVERRIDES", "d3d9=n")])
        );
        // The last change to a name wins.
        let merged = merge_env(
            Vec::new(),
            &[
                ("X".into(), Some("1".into())),
                ("x".into(), Some("2".into())),
            ],
        );
        assert_eq!(merged, vars(&[("x", "2")]));
    }

    #[test]
    fn environment_block_is_double_nul_terminated() {
        let block = environment_block(&vars(&[("A", "1"), ("B", "")]));
        let expected: Vec<u16> = "A=1\0B=\0\0".encode_utf16().collect();
        assert_eq!(block, expected);
        assert_eq!(environment_block(&[]), [0, 0]);
    }
}
