//! Checks for a launcher's patch tests: golden bytes on a synthetic file,
//! and every set on the player's real build when an environment variable
//! names its folder (layers 1 and 2 of the binary-patching playbook).
//!
//! The `check_*` functions return the problem as text; the `assert_*`
//! wrappers panic with it. Under Wine a panic aborts the whole test
//! binary, so test the `check_*` form where a failure is expected.

use std::path::Path;

use super::{PatchSet, apply_sets, prepare, unpatch};
use crate::identify::{hex, sha256};
use crate::runtime;

/// Checks `set` on `synthetic`, a small file built in the test with each
/// site's original bytes in place (its build hash and `results` are not
/// checked; [`real_game`] does that):
///
/// - every site reads its original bytes and becomes its patched bytes,
///   and nothing else changes;
/// - the patched file is accepted as it is (applying again changes
///   nothing) and turns back into `synthetic`;
/// - a site holding other bytes refuses the whole set.
pub fn check_golden(set: &PatchSet, synthetic: &[u8]) -> Result<(), String> {
    let err = |e: super::PatchError| format!("{}: {e}", set.name);
    let prepared = prepare(synthetic, &[set], false).map_err(err)?;
    if prepared.pristine != synthetic {
        return Err(format!(
            "{}: the synthetic file already carries some patched sites",
            set.name
        ));
    }
    let patched = prepared.with(|_| true);
    let rows = super::describe(set, crate::pe::Pe::parse(synthetic).ok().as_ref()).map_err(err)?;
    let mut expected = synthetic.to_vec();
    for row in &rows {
        expected[row.offset..row.offset + row.patched.len()].copy_from_slice(row.patched);
    }
    if patched != expected {
        return Err(format!("{}: bytes outside the sites changed", set.name));
    }
    let again = prepare(&patched, &[set], false)
        .map_err(err)?
        .with(|_| true);
    if again != patched {
        return Err(format!("{}: applying twice changed the file", set.name));
    }
    let back = prepare(&patched, &[set], false).map_err(err)?.pristine;
    if back != synthetic {
        return Err(format!(
            "{}: unpatching did not give the original back",
            set.name
        ));
    }
    for row in &rows {
        let mut wrong = synthetic.to_vec();
        let byte = &mut wrong[row.offset];
        // Neither the original nor the patched first byte.
        *byte = (0..=255u8)
            .find(|b| *b != row.original[0] && *b != row.patched[0])
            .unwrap_or(0);
        if prepare(&wrong, &[set], false).is_ok() {
            return Err(format!(
                "{}: unexpected bytes at {:#x} were accepted",
                set.name, row.offset
            ));
        }
    }
    Ok(())
}

/// [`check_golden`], panicking with the problem.
#[track_caller]
pub fn assert_golden(set: &PatchSet, synthetic: &[u8]) {
    if let Err(problem) = check_golden(set, synthetic) {
        panic!("{problem}");
    }
}

/// File names worth hashing in a game folder.
fn executable(rel: &str) -> bool {
    let lower = rel.to_ascii_lowercase();
    [".exe", ".dll", ".com", ".ovl", ".asi"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// When `env_var` names a folder (`<GAME>_GAME_DIR`), finds every
/// executable there whose SHA-256 is a build some of `sets` are for, and
/// checks on it: all those sets together through [`apply_sets`] (each
/// set's result hash verified), each set alone, applying the result again,
/// and unpatching back to the original. Returns one line per build
/// checked; nothing (and no error) when the variable is unset. An error
/// when the folder holds no known build or a check fails. Nothing is
/// written; never commit the executables.
pub fn real_game(env_var: &str, sets: &[&PatchSet]) -> Result<Vec<String>, String> {
    let Some(dir) = std::env::var_os(env_var) else {
        return Ok(Vec::new());
    };
    real_game_in(Path::new(&dir), sets)
}

/// [`real_game`] on a folder.
pub fn real_game_in(dir: &Path, sets: &[&PatchSet]) -> Result<Vec<String>, String> {
    let listing = runtime::list(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut checked = Vec::new();
    for rel in listing.into_keys().filter(|r| executable(r)) {
        let path = crate::paths::native(dir, &rel);
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let digest = sha256(&bytes);
        let mine: Vec<&PatchSet> = sets
            .iter()
            .copied()
            .filter(|s| s.editions.contains(&digest))
            .collect();
        if mine.is_empty() {
            continue;
        }
        let fail = |what: &str, e: &dyn std::fmt::Display| format!("{rel}: {what}: {e}");
        let all = apply_sets(&bytes, &mine).map_err(|e| fail("all sets", &e))?;
        for set in &mine {
            apply_sets(&bytes, &[set]).map_err(|e| fail(set.name, &e))?;
        }
        let again = apply_sets(&all, &mine).map_err(|e| fail("applied twice", &e))?;
        if again != all {
            return Err(format!("{rel}: applying twice changed the file"));
        }
        if unpatch(&all, &mine).map_err(|e| fail("unpatch", &e))? != bytes {
            return Err(format!("{rel}: unpatching did not give the original back"));
        }
        let names: Vec<&str> = mine.iter().map(|s| s.name).collect();
        checked.push(format!("{rel} ({}): {}", hex(&digest), names.join(", ")));
    }
    if checked.is_empty() {
        return Err(format!(
            "no build the patch sets know is in {}",
            dir.display()
        ));
    }
    Ok(checked)
}

/// [`real_game`], panicking with the problem; prints what it checked.
#[track_caller]
pub fn assert_real_game(env_var: &str, sets: &[&PatchSet]) {
    match real_game(env_var, sets) {
        Ok(lines) => lines.iter().for_each(|l| println!("checked {l}")),
        Err(problem) => panic!("{problem}"),
    }
}
