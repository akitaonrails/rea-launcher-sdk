//! @LAUNCHER_EXE@: a Windows GUI program (it also runs under Proton and
//! Wine). Everything testable lives in the library; this file wires it to
//! Windows: the console for `--help`/`--check`/`--no-window`, the settings
//! window, `launcher.log`, the runtime copy and the game's process.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(app::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "@LAUNCHER_EXE@ is a Windows program; on Linux run it with Proton, Wine or linux/@ID@-play."
    );
    std::process::exit(@CRATE_SNAKE@::cli::USAGE_ERROR);
}

#[cfg(windows)]
mod app {
    use std::path::PathBuf;

    use @CRATE_SNAKE@::cli::{self, Invocation, Mode};
    use @CRATE_SNAKE@::config;
    use @CRATE_SNAKE@::{runtime, ui};
    use rea_launcher::log::Log;
    use rea_launcher::{cfg, fsx};
    use rea_launcher_win32::instance::{self, SecondStart, Window};
    use rea_launcher_win32::process::{self, Launch};
    use rea_launcher_win32::report::{self, Mode as Says, Run};
    use rea_launcher_win32::gpu::{self, Glx};
    use rea_launcher_win32::{dialog, display, wine};

    const TITLE: &str = "@GAME@";

    /// `%LOCALAPPDATA%\@LAUNCHER_NAME@`: launcher.cfg, launcher.log and the
    /// runtime copy.
    fn home() -> PathBuf {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("@LAUNCHER_NAME@")
    }

    /// `launcher.cfg` beside the exe (a portable install), else in [`home`].
    fn settings_path() -> PathBuf {
        let portable = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(config::FILE_NAME)));
        match portable {
            Some(path) if path.is_file() => path,
            _ => home().join(config::FILE_NAME),
        }
    }

    /// Where messages go: the SDK's `report` decides from the run's
    /// audience (the terminal, a dialog, or only `launcher.log`).
    struct Out {
        log: Log,
        says: Says,
    }

    impl Out {
        /// A command's result (`--help`, `--check`): the terminal, or a
        /// dialog when a person started it without one.
        fn say(&self, text: &str) {
            let route = report::route_here(self.says);
            if route.print {
                println!("{text}");
            }
            if route.dialog {
                dialog::message(TITLE, text, dialog::Icon::Info);
            }
        }

        /// An error the launcher stops on: always in the log, and a dialog
        /// whenever a person may be watching.
        fn fail(&self, text: &str) {
            self.log.line(&format!("error: {text}"));
            report::fatal(TITLE, text, self.says);
        }

        fn warn(&self, message: &str) {
            self.log.line(&format!("warning: {message}"));
        }
    }

    pub fn run() -> i32 {
        let log = Log::new(home().join("launcher.log"));
        log.line(concat!("@LAUNCHER_NAME@ ", env!("CARGO_PKG_VERSION")));
        let args: Vec<String> = std::env::args().skip(1).collect();
        let parsed = cli::parse(args.iter().cloned());
        let audience = match &parsed {
            Ok(invocation) => report::audience(invocation.run()),
            // A bad command line answers like a command: the terminal, or a
            // dialog when a person started it and sees no terminal.
            Err(_) => report::audience(Run::bad_command_line(&args)),
        };
        let out = Out {
            log,
            says: audience.mode(),
        };
        let invocation = match parsed {
            Ok(invocation) => invocation,
            Err(error) => {
                out.fail(&error);
                return cli::USAGE_ERROR;
            }
        };
        match run_mode(&out, invocation) {
            Ok(code) => code,
            Err(error) => {
                out.fail(&error);
                cli::REFUSED
            }
        }
    }

    /// One launcher at a time (STANDARDS "One instance"): a second start
    /// brings the open window forward, or says the game is running and
    /// exits non-zero. Keep the guard for the launcher's lifetime.
    fn single_instance(out: &Out, invocation: &Invocation) -> Result<instance::Guard, i32> {
        match instance::single(@CRATE_SNAKE@::INSTANCE) {
            Ok(guard) => Ok(guard),
            Err(running) => {
                let second = running.second_start(
                    invocation.is_action(),
                    Window::Own(@CRATE_SNAKE@::WINDOW_TITLE),
                );
                out.log.line(second.log_line());
                if let Some(text) = second.message(TITLE) {
                    out.fail(&text);
                }
                Err(second.exit_code())
            }
        }
    }

    fn run_mode(out: &Out, invocation: Invocation) -> Result<i32, String> {
        if invocation.mode == Mode::Help {
            out.say(&config::usage());
            return Ok(cli::OK);
        }
        let mut instance = match single_instance(out, &invocation) {
            Ok(guard) => Some(guard),
            Err(code) => return Ok(code),
        };
        let path = settings_path();
        if let Mode::Window { settings_only } = invocation.mode {
            // A fresh Wine prefix draws eframe windows through EGL, which
            // fails on many drivers: switch it to GLX, and run the launcher
            // once more so this first start already gets the window.
            match gpu::prefer_glx_or_restart(instance.take()) {
                Glx::Restarted { exit_code } => {
                    out.log.line("settings window: GLX set on a fresh prefix, started again");
                    return Ok(exit_code as i32);
                }
                Glx::Ready {
                    instance: guard,
                    warning,
                } => {
                    instance = guard;
                    if let Some(warning) = warning {
                        out.warn(&warning);
                    }
                }
            }
            let saved = cfg::read_file(&path).ok().flatten();
            match ui::show(&path, saved.as_deref(), invocation.linux) {
                Ok(ui::Outcome::Play) if settings_only => return Ok(cli::OK),
                Ok(ui::Outcome::Play) => {}
                Ok(ui::Outcome::Closed) if settings_only => return Ok(cli::WINDOW_CLOSED),
                Ok(ui::Outcome::Closed) => return Ok(cli::OK),
                Err(error) => {
                    out.warn(&format!("settings window unavailable: {error}"));
                    if settings_only {
                        return Ok(cli::WINDOW_UNAVAILABLE);
                    }
                }
            }
        }

        let screen = display::screen_size();
        let existing = cfg::read_file(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
        let first_run = existing.is_none();
        let text = existing.unwrap_or_else(|| config::first_run_text(screen));
        let (saved, warnings) = config::parse_file(&text);
        for warning in &warnings {
            out.warn(warning);
        }
        let settings = saved.clone().merged(invocation.flags);
        let cwd = std::env::current_dir().unwrap_or_default();
        let (game, found) = cli::check(&settings, &cwd)?;
        out.log.line(&format!(
            "game: {} ({}, {})",
            game.display(),
            found.edition.name,
            found.exe.display()
        ));
        if invocation.mode == Mode::Check {
            out.say(&cli::check_report(&game, &found));
            return Ok(cli::OK);
        }

        let runtime_dir = settings.runtime.clone().unwrap_or_else(|| home().join("runtime"));
        let exe_name = found
            .exe
            .file_name()
            .ok_or("the game's exe has no file name")?;
        let exe = runtime_dir.join(exe_name);
        // A launcher that was killed released the instance, but its game
        // may still run from this runtime copy: never sync under it.
        if !process::find_running(&exe).is_empty() {
            let busy = SecondStart::Busy;
            out.log.line(busy.log_line());
            out.fail(&busy.message(TITLE).unwrap_or_default());
            return Ok(busy.exit_code());
        }
        let report = runtime::prepare(&game, &runtime_dir)?;
        for warning in &report.warnings {
            out.warn(warning);
        }
        out.log.line(&format!(
            "runtime: {} ({} brought, {} linked)",
            runtime_dir.display(),
            report.brought,
            report.linked
        ));
        // Phase 4 of the playbook: components, the game's own configuration
        // (generated from the picture below), patches. Log every decision.
        out.log.line(&format!("picture: {}", settings.picture(screen).describe()));

        if first_run || saved.game.as_ref() != Some(&game) {
            let text = config::set(&text, "game", &game.display().to_string());
            fsx::write_atomic(&path, text)
                .map_err(|error| format!("Cannot save {}: {error}", path.display()))?;
        }
        if !wine::is_wine() {
            match gpu::prefer_fast_gpu(&exe) {
                Ok(true) => out.log.line("Windows: high-performance GPU preferred"),
                Ok(false) => {}
                Err(error) => out.warn(&error.to_string()),
            }
        }
        let code = Launch::new(&exe).cwd(&runtime_dir).run()?;
        out.log.line(&format!("game exited with {code}"));
        drop(instance);
        Ok(code as i32)
    }
}
