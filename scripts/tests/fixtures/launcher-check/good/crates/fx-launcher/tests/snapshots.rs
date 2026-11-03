#[test]
fn help() {
    let _ = "--frame-cap";
    rea_launcher::snapshot!("help.txt", String::new());
}
