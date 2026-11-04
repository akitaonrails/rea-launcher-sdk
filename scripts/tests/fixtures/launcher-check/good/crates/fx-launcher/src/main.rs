#![cfg_attr(windows, windows_subsystem = "windows")]
use rea_launcher_win32::instance::{self, Window};
use rea_launcher_win32::report::{self, Run};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--bogus") {
        let mode = report::audience(Run::bad_command_line(&args)).mode();
        report::fatal("FX Launcher", "unknown flag", mode);
        std::process::exit(report::BAD_COMMAND_LINE);
    }
    let audience = report::audience(Run::from_args(&args, &[]));
    let _guard = match instance::single("rea-fx") {
        Ok(guard) => guard,
        Err(running) => {
            let action = instance::is_action(&args, &[]);
            std::process::exit(running.second_start(action, Window::Own("FX Launcher")).exit_code())
        }
    };
    let _ = audience;
}
