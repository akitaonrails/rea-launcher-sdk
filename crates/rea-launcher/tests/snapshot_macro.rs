//! `rea_launcher::snapshot!` from another crate's point of view: the
//! caller's tests/snapshots/ and CARGO_PKG_VERSION.

#[test]
fn the_snapshot_macro_reads_the_callers_folder_and_version() {
    let help = format!(
        "rea-launcher {}\r\nsecond line\r\n",
        env!("CARGO_PKG_VERSION")
    );
    rea_launcher::snapshot!("macro.txt", help);
    rea_launcher::snapshot!("macro.txt", &help);
    let snapshots = rea_launcher::snapshots!().update_var("REA_LAUNCHER_UPDATE_SNAPSHOTS");
    assert_eq!(
        snapshots.check("macro.txt", &help).unwrap(),
        rea_launcher::testing::Outcome::Matched
    );
}
