//! Windows command-line quoting.
//!
//! From CMR3 `args.rs`, the only copy with tests for the C runtime's
//! backslash rules (bda85fb: xdelta got its own path as an argument, which
//! a unit test of the argument list now pins); the other launchers either
//! passed no arguments or let `std::process::Command` quote. Pure: the
//! win32 crate's process start builds its command line with it.

use std::ffi::OsStr;

/// Quotes one argument for the C runtime's command-line parser
/// (`CommandLineToArgvW` rules): always in quotes, backslashes doubled
/// before a quote and at the end.
pub fn quote_windows(arg: &OsStr) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for ch in arg.to_string_lossy().chars() {
        match ch {
            '\\' => {
                slashes += 1;
                out.push(ch);
            }
            '"' => {
                out.push_str(&"\\".repeat(slashes + 1));
                out.push(ch);
                slashes = 0;
            }
            _ => {
                out.push(ch);
                slashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(slashes));
    out.push('"');
    out
}

/// The full command line for `CreateProcessW`: the program in plain quotes
/// (`argv[0]` has no escape rules, and paths cannot hold `"`), then each
/// argument quoted with [`quote_windows`].
pub fn command_line<A: AsRef<OsStr>>(program: &OsStr, args: &[A]) -> String {
    let mut line = format!("\"{}\"", program.to_string_lossy());
    for arg in args {
        line.push(' ');
        line.push_str(&quote_windows(arg.as_ref()));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_spaces_empty_strings_quotes_and_trailing_backslashes() {
        assert_eq!(quote_windows(OsStr::new("plain")), "\"plain\"");
        assert_eq!(quote_windows(OsStr::new("two words")), "\"two words\"");
        assert_eq!(quote_windows(OsStr::new("")), "\"\"");
        assert_eq!(quote_windows(OsStr::new("a\"b")), "\"a\\\"b\"");
        assert_eq!(quote_windows(OsStr::new(r#"a\"b"#)), r#""a\\\"b""#);
        assert_eq!(
            quote_windows(OsStr::new(r"C:\Games\Colin McRae Rally 3\")),
            r#""C:\Games\Colin McRae Rally 3\\""#
        );
        // Backslashes not before a quote stay single.
        assert_eq!(quote_windows(OsStr::new(r"a\\b")), r#""a\\b""#);
    }

    #[test]
    fn command_line_quotes_program_and_each_argument() {
        let no_args: [&str; 0] = [];
        assert_eq!(
            command_line(OsStr::new(r"C:\Game\game.exe"), &no_args),
            r#""C:\Game\game.exe""#
        );
        assert_eq!(
            command_line(OsStr::new(r"C:\x\xdelta3.exe"), &["-d", r"C:\in dir\"]),
            r#""C:\x\xdelta3.exe" "-d" "C:\in dir\\""#
        );
    }
}
