//! `rea_launcher::pe::edit::add_import` checked by the real loader (Windows,
//! or Wine under `scripts/wine-test`): a copy of `examples/dll_probe.exe`
//! gets the stand-in `d3dx9_35.dll` in its import table, so the loader maps
//! it before `main` runs, with no injection from outside.

#![cfg(windows)]

use std::path::PathBuf;

use rea_launcher::pe::edit;
use rea_launcher_win32::process::Launch;

/// What the probe exits with when the native stand-in is loaded.
const NATIVE: u32 = 10;
/// `STATUS_DLL_NOT_FOUND`: the loader refused to start the image.
const DLL_NOT_FOUND: u32 = 0xC000_0135;

/// A private folder with the stand-in DLL and the probe, the probe's bytes
/// passed through `edit`.
fn install(name: &str, edit: impl FnOnce(&[u8]) -> Vec<u8>) -> PathBuf {
    let build = std::env::current_exe().unwrap();
    let examples = build.parent().unwrap().parent().unwrap().join("examples");
    let dir = examples.join(format!("pe-import-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(examples.join("d3dx9_35.dll"), dir.join("d3dx9_35.dll")).unwrap();
    let probe = std::fs::read(examples.join("dll_probe.exe")).unwrap();
    let exe = dir.join(format!("{name}.exe"));
    std::fs::write(&exe, edit(&probe)).unwrap();
    exe
}

fn run(exe: &PathBuf) -> u32 {
    let code = Launch::new(exe).cwd(exe.parent().unwrap()).run();
    let _ = std::fs::remove_dir_all(exe.parent().unwrap());
    code.unwrap()
}

#[test]
fn an_added_import_is_loaded_before_the_program_runs() {
    let exe = install("imported", |bytes| {
        let added = edit::add_import(
            bytes,
            ".reaimp",
            "d3dx9_35.dll",
            &["rea_native_marker"],
            None,
        )
        .unwrap();
        let found = edit::imports(&added).unwrap();
        assert!(found.iter().any(|i| i.dll == "d3dx9_35.dll"));
        added
    });
    assert_eq!(run(&exe), NATIVE);
}

#[test]
fn a_missing_imported_dll_stops_the_program_from_starting() {
    // The control: the loader really reads the new import.
    let exe = install("missing", |bytes| {
        edit::add_import(bytes, ".reaimp", "rea_missing.dll", &["nothing"], None).unwrap()
    });
    assert_eq!(run(&exe), DLL_NOT_FOUND);
}

#[test]
fn an_address_table_in_a_fix_section_is_filled_by_the_loader() {
    // CMR1's layout: a writable code+data section first, the import's
    // address table inside it.
    let exe = install("table", |bytes| {
        let (with_fix, fix) =
            edit::add_section(bytes, ".rea", &[0u8; 0x100], edit::SCN_CODE_RWX).unwrap();
        edit::add_import(
            &with_fix,
            ".reaimp",
            "d3dx9_35.dll",
            &["rea_native_marker"],
            Some(fix.rva + 0x40),
        )
        .unwrap()
    });
    assert_eq!(run(&exe), NATIVE);
}
