use rea_launcher_win32::instance::{self, Window};
use rea_launcher_win32::report::{self, Run};

fn main() {
    let audience = report::audience(Run::from_args(&[] as &[&str], &[]));
    let _guard = match instance::single("rea-fx") {
        Ok(guard) => guard,
        Err(running) => std::process::exit(running.second_start(false, Window::Own("FX Launcher")).exit_code()),
    };
    let _ = audience;
}
