//! Telling the player something from a native (Linux) launcher: the
//! terminal, a small message window, or only the log.
//!
//! `rea_launcher_win32::report` shows a Windows message box; a native
//! Linux launcher has none, so before v0.4.0 DiRT Rally's errors went to
//! stderr and `launcher.log` only, and an AppImage double-clicked with a
//! missing or unknown game just exited. The same rule, natively:
//!
//! - **Script-facing commands** (`--help`, `--check`, [`Mode::Command`]):
//!   the terminal; a window only when no terminal is attached and a person
//!   started the launcher on a desktop.
//! - **Play-time messages** (game not found, unknown edition, a failed
//!   copy, [`Mode::Play`]): a window whenever a person may be watching on a
//!   desktop, also from a terminal.
//! - Either way the text is printed to stderr, which the play helper and
//!   the AppImage send to the log; with no desktop and no terminal that is
//!   all ([`Audience::LogOnly`]).
//!
//! "A terminal is attached": stdin or stderr is a terminal. "A desktop":
//! `DISPLAY` or `WAYLAND_DISPLAY` is set. [`audience`] is the pure table;
//! [`fatal`] applies it in this process.
//!
//! ```no_run
//! use rea_launcher_ui::report::{self, Link, Message, Mode};
//! # use rea_launcher_ui::theme::{Metrics, Palette};
//! # let palette = Palette::default(); // the launcher's own PALETTE
//! let message = Message {
//!     title: "DiRT Rally",
//!     text: "This copy of DiRT Rally is an edition the launcher does not know yet.",
//!     link: Some(Link {
//!         label: "Report the edition",
//!         url: "https://github.com/akitaonrails/rea-dirt-rally/issues/new?template=unknown-edition.yml",
//!     }),
//!     details: Some("DiRTRallyLauncher 0.1.0\nexe: …/DirtRally (sha256 …)"),
//! };
//! report::fatal(&palette, &Metrics::DEFAULT, &message, Mode::Play { interactive: true });
//! ```

use std::ffi::OsStr;

use eframe::egui::{self, Key, Rect, RichText, Role, Ui, vec2};

use crate::pad::{self, Nav, PadNav};
use crate::theme::{self, Metrics, Palette, body, condensed, size};
use crate::widgets;

/// What kind of message it is, and whether a person started the launcher
/// (`interactive`: false for `--no-window` or other runs from scripts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Output of a script-facing command: `--help`, `--check`.
    Command { interactive: bool },
    /// A message while playing or preparing to play.
    Play { interactive: bool },
}

/// What a native process can tell about who is watching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Native {
    /// stdin is a terminal.
    pub stdin_terminal: bool,
    /// stderr is a terminal (not a pipe or the helper's log).
    pub stderr_terminal: bool,
    /// A desktop to open a window on (see [`Native::desktop`]).
    pub display: bool,
}

impl Native {
    /// This process: `std::io::IsTerminal` on stdin and stderr, and
    /// `DISPLAY`/`WAYLAND_DISPLAY` (always a desktop on Windows).
    pub fn here() -> Native {
        use std::io::IsTerminal;
        Native {
            stdin_terminal: std::io::stdin().is_terminal(),
            stderr_terminal: std::io::stderr().is_terminal(),
            display: !cfg!(unix)
                || Native::desktop(
                    std::env::var_os("DISPLAY").as_deref(),
                    std::env::var_os("WAYLAND_DISPLAY").as_deref(),
                ),
        }
    }

    /// Whether either display variable names a display (set and not empty).
    pub fn desktop(display: Option<&OsStr>, wayland: Option<&OsStr>) -> bool {
        [display, wayland]
            .into_iter()
            .any(|value| value.is_some_and(|value| !value.is_empty()))
    }

    /// Whether a person sees what is printed.
    pub fn terminal(&self) -> bool {
        self.stdin_terminal || self.stderr_terminal
    }
}

/// Who gets one message, beyond stderr and the log (which always do).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience {
    /// The person at the terminal: printing is enough.
    Terminal,
    /// A message window ([`show`]), as well as the printed text.
    Window,
    /// Nobody is watching: only stderr and the log.
    LogOnly,
}

/// The decision table of the module docs.
pub fn audience(mode: Mode, native: Native) -> Audience {
    let (interactive, command) = match mode {
        Mode::Command { interactive } => (interactive, true),
        Mode::Play { interactive } => (interactive, false),
    };
    if command && native.terminal() {
        Audience::Terminal
    } else if interactive && native.display {
        Audience::Window
    } else if native.terminal() {
        Audience::Terminal
    } else {
        Audience::LogOnly
    }
}

/// A web page the message points to (the unknown-edition issue form).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link<'a> {
    /// The button's text: what the page is for ("Report the edition").
    pub label: &'a str,
    pub url: &'a str,
}

/// One message: a title (the window's), the text, an optional page, and
/// details for a bug report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Message<'a> {
    pub title: &'a str,
    /// Plain language: what happened and what the player can do.
    pub text: &'a str,
    pub link: Option<Link<'a>>,
    /// Extra lines Copy details adds (version, paths, hashes); not shown.
    pub details: Option<&'a str>,
}

impl Message<'_> {
    /// What Copy details puts on the clipboard: title, text, details and
    /// the page, separated by blank lines.
    pub fn details_text(&self) -> String {
        let mut parts = vec![self.title, self.text];
        parts.extend(self.details);
        parts.extend(self.link.map(|link| link.url));
        parts
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// The printed form: `title: text`, then the page.
    pub fn printed(&self) -> String {
        let mut text = format!("{}: {}", self.title, self.text);
        if let Some(link) = self.link {
            text.push_str(&format!("\n{}: {}", link.label, link.url));
        }
        text
    }
}

/// What the player did in [`message_ui`] this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// OK, Enter, Escape (or the pad's A on OK, B, Start): close.
    Close,
    /// The details went to the clipboard.
    Copied,
    /// The link was opened in the browser.
    OpenedLink,
}

/// What [`message_ui`] remembers between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MessageState {
    /// Copy details was pressed: "Copied." is shown.
    pub copied: bool,
    focused: bool,
}

/// Draws `message` filling `ui`: the title, the text (scrolling when
/// long), the page's address, then Copy details, the link's button and OK.
/// OK has the focus at first, so Enter, Space or the pad's A close it, as
/// do Escape and the pad's B or Start ([`pad_closes`]). Copying and opening
/// the page happen here (`ctx.copy_text`, `ctx.open_url`).
///
/// Accessibility: the title is a heading-styled label, the text a label,
/// every button named by its text; the address is visible so it can be
/// typed elsewhere.
pub fn message_ui(ui: &mut Ui, message: &Message<'_>, state: &mut MessageState) -> Option<Action> {
    let p = theme::palette(ui.ctx());
    let mut action = None;
    if ui.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, Key::Escape)
            || (!state.focused && input.consume_key(egui::Modifiers::NONE, Key::Enter))
    }) {
        action = Some(Action::Close);
    }
    let full = ui.max_rect().shrink(24.0);
    let footer_height = 34.0;
    let body_rect = Rect::from_min_max(
        full.min,
        egui::pos2(full.right(), full.bottom() - footer_height - 16.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(body_rect), |ui| {
        let title = ui.label(
            RichText::new(message.title)
                .font(condensed(26.0))
                .color(p.ink),
        );
        title.widget_info(|| egui::WidgetInfo::labeled(Role::Heading, true, message.title));
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.label(
                    RichText::new(message.text)
                        .font(body(size::LABEL))
                        .color(p.ink),
                );
                if let Some(link) = message.link {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!("{}: {}", link.label, link.url))
                            .font(body(size::NOTE))
                            .color(p.muted),
                    );
                }
                if state.copied {
                    let copied =
                        widgets::note(ui, "Copied. Paste it into a bug report or a message.");
                    // Announced when it appears.
                    ui.ctx().accesskit_node_builder(copied.id, |node| {
                        node.set_live(egui::accesskit::Live::Polite);
                    });
                }
            });
    });
    let footer = Rect::from_min_max(
        egui::pos2(full.left(), full.bottom() - footer_height),
        full.max,
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(footer), |ui| {
        ui.horizontal(|ui| {
            if widgets::outline_button(ui, "Copy details").clicked() {
                ui.ctx().copy_text(message.details_text());
                state.copied = true;
                action = Some(Action::Copied);
            }
            if let Some(link) = message.link {
                let open = widgets::outline_button(ui, link.label);
                // It opens a page: a link to screen readers.
                ui.ctx()
                    .accesskit_node_builder(open.id, |node| node.set_role(Role::Link));
                if open.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(link.url));
                    action = Some(Action::OpenedLink);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = widgets::primary_button(ui, "OK");
                if !state.focused {
                    ok.request_focus();
                    state.focused = true;
                }
                if ok.clicked() {
                    action = Some(Action::Close);
                }
            });
        });
    });
    action
}

/// Whether a pad step closes the message window (B/Back or Start; A
/// presses the focused button through [`pad::inject`]).
pub fn pad_closes(nav: Nav) -> bool {
    matches!(nav, Nav::Back | Nav::Play)
}

/// The message window's size, in points.
pub const WINDOW_SIZE: egui::Vec2 = vec2(560.0, 300.0);

/// Prints `message` to stderr and, when [`audience`] says so for this
/// process ([`Native::here`]), shows it in a window ([`show`]) and waits
/// for the player to close it. Returns who got it.
pub fn fatal(palette: &Palette, metrics: &Metrics, message: &Message<'_>, mode: Mode) -> Audience {
    eprintln!("{}", message.printed());
    let audience = audience(mode, Native::here());
    if audience == Audience::Window
        && let Err(error) = show(palette, metrics, message)
    {
        eprintln!("(the message window could not open: {error})");
    }
    audience
}

/// Shows `message` in its own window in `palette`'s colours, with pad
/// navigation (XInput bits from `buttons`, e.g.
/// `pad::joystick::Joysticks::buttons` on Linux), until the player closes
/// it. See [`show`] for the usual pads.
pub fn show_with(
    palette: &Palette,
    metrics: &Metrics,
    message: &Message<'_>,
    buttons: impl FnMut() -> u16 + 'static,
) -> Result<(), String> {
    let owned = Owned::from(message);
    let (palette, metrics) = (*palette, *metrics);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(owned.title.clone())
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(vec2(420.0, 240.0)),
        ..eframe::NativeOptions::default()
    };
    let title = owned.title.clone();
    eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            theme::install_with(&cc.egui_ctx, &palette, &metrics);
            Ok(Box::new(MessageApp {
                message: owned,
                state: MessageState::default(),
                pad: PadNav::default(),
                buttons: Box::new(buttons),
                pending: Vec::new(),
            }))
        }),
    )
    .map_err(|error| error.to_string())
}

/// [`show_with`] reading every pad this platform's SDK reads natively:
/// `/dev/input/js*` on Linux, none elsewhere (a Windows launcher uses
/// `rea_launcher_win32::report`).
pub fn show(palette: &Palette, metrics: &Metrics, message: &Message<'_>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let mut joysticks = pad::joystick::Joysticks::new();
        show_with(palette, metrics, message, move || joysticks.buttons())
    }
    #[cfg(not(target_os = "linux"))]
    {
        show_with(palette, metrics, message, || 0)
    }
}

/// A [`Message`] the window keeps for its lifetime.
struct Owned {
    title: String,
    text: String,
    link: Option<(String, String)>,
    details: Option<String>,
}

impl From<&Message<'_>> for Owned {
    fn from(message: &Message<'_>) -> Owned {
        Owned {
            title: message.title.to_owned(),
            text: message.text.to_owned(),
            link: message
                .link
                .map(|link| (link.label.to_owned(), link.url.to_owned())),
            details: message.details.map(str::to_owned),
        }
    }
}

impl Owned {
    fn message(&self) -> Message<'_> {
        Message {
            title: &self.title,
            text: &self.text,
            link: self.link.as_ref().map(|(label, url)| Link { label, url }),
            details: self.details.as_deref(),
        }
    }
}

struct MessageApp {
    message: Owned,
    state: MessageState,
    pad: PadNav,
    buttons: Box<dyn FnMut() -> u16>,
    pending: Vec<Nav>,
}

impl eframe::App for MessageApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let navs = self.pad.update((self.buttons)());
        pad::inject(ctx, raw, &navs);
        self.pending.extend(navs);
    }

    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        let mut close = std::mem::take(&mut self.pending)
            .into_iter()
            .any(pad_closes);
        close |= message_ui(ui, &self.message.message(), &mut self.state) == Some(Action::Close);
        if close {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // A pad sends no window events: look again soon.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, NodeT, Queryable};

    const fn native(stdin: bool, stderr: bool, display: bool) -> Native {
        Native {
            stdin_terminal: stdin,
            stderr_terminal: stderr,
            display,
        }
    }

    #[test]
    fn who_sees_a_native_message() {
        let play = Mode::Play { interactive: true };
        let help = Mode::Command { interactive: true };
        // The DiRT Rally case: an AppImage double-clicked on a desktop.
        assert_eq!(audience(play, native(false, false, true)), Audience::Window);
        assert_eq!(audience(help, native(false, false, true)), Audience::Window);
        // From a terminal: play-time errors still get the window...
        assert_eq!(audience(play, native(true, true, true)), Audience::Window);
        // ...commands only print.
        assert_eq!(audience(help, native(true, true, true)), Audience::Terminal);
        assert_eq!(
            audience(help, native(true, false, true)),
            Audience::Terminal
        );
        // Over ssh (no desktop): the terminal.
        assert_eq!(
            audience(play, native(true, true, false)),
            Audience::Terminal
        );
        // The play helper's log, no desktop, or a script: the log only.
        assert_eq!(
            audience(play, native(false, false, false)),
            Audience::LogOnly
        );
        let script = Mode::Play { interactive: false };
        assert_eq!(
            audience(script, native(false, false, true)),
            Audience::LogOnly
        );
        assert_eq!(
            audience(script, native(false, true, true)),
            Audience::Terminal
        );
        let check = Mode::Command { interactive: false };
        assert_eq!(
            audience(check, native(false, false, true)),
            Audience::LogOnly
        );
    }

    #[test]
    fn a_desktop_is_a_display_variable_with_a_value() {
        let os = |s: &'static str| Some(OsStr::new(s));
        assert!(Native::desktop(os(":0"), None));
        assert!(Native::desktop(None, os("wayland-1")));
        assert!(!Native::desktop(os(""), None));
        assert!(!Native::desktop(None, None));
    }

    const URL: &str = "https://example.org/issues/new?template=unknown-edition.yml";

    const MESSAGE: Message<'static> = Message {
        title: "DiRT Rally",
        text: "This copy is an edition the launcher does not know yet.",
        link: Some(Link {
            label: "Report the edition",
            url: URL,
        }),
        details: Some("DiRTRallyLauncher 0.1.0"),
    };

    #[test]
    fn details_and_the_printed_text_carry_everything() {
        assert_eq!(
            MESSAGE.details_text(),
            format!(
                "DiRT Rally\n\nThis copy is an edition the launcher does not know yet.\n\nDiRTRallyLauncher 0.1.0\n\n{URL}"
            )
        );
        assert_eq!(
            MESSAGE.printed(),
            format!(
                "DiRT Rally: This copy is an edition the launcher does not know yet.\nReport the edition: {URL}"
            )
        );
        let plain = Message {
            title: "T",
            text: "x",
            ..Message::default()
        };
        assert_eq!(plain.details_text(), "T\n\nx");
    }

    #[derive(Default)]
    struct Seen {
        state: MessageState,
        actions: Vec<Action>,
    }

    fn open(message: Message<'static>) -> testing::Harness<'static, Seen> {
        testing::harness_sized(
            &crate::theme::tests::CMR2005,
            &Metrics::DEFAULT,
            WINDOW_SIZE,
            Seen::default(),
            move |ui, seen: &mut Seen| {
                if let Some(action) = message_ui(ui, &message, &mut seen.state) {
                    seen.actions.push(action);
                }
            },
        )
    }

    #[test]
    fn the_window_shows_the_text_the_page_and_its_buttons() {
        let mut ui = open(MESSAGE);
        assert!(
            ui.query_by_role_and_label(Role::Heading, "DiRT Rally")
                .is_some()
        );
        assert!(ui.query_by_label(MESSAGE.text).is_some());
        assert!(
            ui.query_by_label(&format!("Report the edition: {URL}"))
                .is_some(),
            "the address is visible"
        );
        ui.get_by_role_and_label(Role::Button, "Copy details")
            .click();
        ui.step();
        let copied = ui.output().platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if *text == MESSAGE.details_text())
        });
        assert!(copied);
        ui.run();
        let copied = ui.get_by_label_contains("Copied.");
        assert_eq!(
            copied.accesskit_node().live(),
            egui::accesskit::Live::Polite
        );
        ui.get_by_role_and_label(Role::Link, "Report the edition")
            .click();
        ui.step();
        let opened = ui.output().platform_output.commands.iter().any(
            |command| matches!(command, egui::OutputCommand::OpenUrl(open) if open.url == URL),
        );
        assert!(opened);
        ui.run();
        ui.get_by_role_and_label(Role::Button, "OK").click();
        ui.run();
        assert_eq!(
            ui.state().actions,
            [Action::Copied, Action::OpenedLink, Action::Close]
        );
    }

    #[test]
    fn ok_has_the_focus_so_enter_or_a_pad_closes_it() {
        let mut ui = open(Message {
            link: None,
            ..MESSAGE
        });
        assert!(
            ui.query_by_role_and_label(Role::Button, "Report the edition")
                .is_none()
        );
        // The pad's A is Space on the focused button (pad::inject).
        ui.key_press(Key::Space);
        ui.run();
        assert_eq!(ui.state().actions, [Action::Close]);
        let mut ui = open(MESSAGE);
        ui.key_press(Key::Escape);
        ui.run();
        assert_eq!(ui.state().actions, [Action::Close]);
        assert!(pad_closes(Nav::Back) && pad_closes(Nav::Play));
        assert!(!pad_closes(Nav::Down) && !pad_closes(Nav::Press));
    }
}
