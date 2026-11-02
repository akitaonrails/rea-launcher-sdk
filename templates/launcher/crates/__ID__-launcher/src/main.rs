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
    use rea_launcher_win32::console::{self, Output};
    use rea_launcher_win32::dialog::{self, Icon};
    use rea_launcher_win32::process::Launch;
    use rea_launcher_win32::{display, gpu, wine};

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

    /// Where messages go: the starting terminal, a dialog when a person
    /// started it without one, or only `launcher.log`.
    struct Out {
        log: Log,
        attached: bool,
        interactive: bool,
    }

    impl Out {
        fn say(&self, text: &str, icon: Icon) {
            match console::output(self.attached, console::stdout_usable(), self.interactive) {
                Output::Terminal if icon == Icon::Info => println!("{text}"),
                Output::Terminal => eprintln!("{text}"),
                Output::Dialog => dialog::message(TITLE, text, icon),
                Output::Nowhere => {}
            }
        }

        fn warn(&self, message: &str) {
            self.log.line(&format!("warning: {message}"));
        }
    }

    pub fn run() -> i32 {
        let log = Log::new(home().join("launcher.log"));
        log.line(concat!("@LAUNCHER_NAME@ ", env!("CARGO_PKG_VERSION")));
        let parsed = cli::parse(std::env::args().skip(1));
        let (attached, interactive) = match &parsed {
            Ok(invocation) => (
                invocation.wants_console() && console::attach_parent(),
                invocation.interactive(),
            ),
            Err(_) => (console::attach_parent(), true),
        };
        let out = Out {
            log,
            attached,
            interactive,
        };
        let invocation = match parsed {
            Ok(invocation) => invocation,
            Err(error) => {
                out.log.line(&format!("error: {error}"));
                out.say(&error, Icon::Error);
                return cli::USAGE_ERROR;
            }
        };
        match run_mode(&out, invocation) {
            Ok(code) => code,
            Err(error) => {
                out.log.line(&format!("error: {error}"));
                out.say(&error, Icon::Error);
                cli::REFUSED
            }
        }
    }

    fn run_mode(out: &Out, invocation: Invocation) -> Result<i32, String> {
        if invocation.mode == Mode::Help {
            out.say(&config::usage(), Icon::Info);
            return Ok(cli::OK);
        }
        let path = settings_path();
        if let Mode::Window { settings_only } = invocation.mode {
            if wine::is_wine() {
                match gpu::prefer_glx() {
                    Ok(true) => out.log.line("settings window: OpenGL through GLX (UseEGL=N)"),
                    Ok(false) => {}
                    Err(error) => out.warn(&error.to_string()),
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
            out.say(&cli::check_report(&game, &found), Icon::Info);
            return Ok(cli::OK);
        }

        let runtime_dir = settings.runtime.clone().unwrap_or_else(|| home().join("runtime"));
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
        let exe_name = found
            .exe
            .file_name()
            .ok_or("the game's exe has no file name")?;
        let exe = runtime_dir.join(exe_name);
        if !wine::is_wine() {
            match gpu::prefer_fast_gpu(&exe) {
                Ok(true) => out.log.line("Windows: high-performance GPU preferred"),
                Ok(false) => {}
                Err(error) => out.warn(&error.to_string()),
            }
        }
        let code = Launch::new(&exe).cwd(&runtime_dir).run()?;
        out.log.line(&format!("game exited with {code}"));
        Ok(code as i32)
    }
}
