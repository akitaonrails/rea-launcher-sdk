# rea-launcher-ui

The settings window kit of the rea launchers (eframe 0.37, glow): the shared
look with each game's colours, art and tabs left to the launcher.

| Module | What |
|---|---|
| `theme` | `Palette` (colour roles: base, panel, line, ink, muted, action, select, error, disabled, focus, selection; `tint` for a const translucent colour), `Metrics` (sizes and style choices, below), `install`/`install_with`, `visuals` (the egui style: dark, or light with palette-filled widgets when `Palette::is_light`), `palette`/`metrics`, `contrast`, `contrast_problems(&Palette, &Metrics)` / `Palette::pairs`, Barlow fonts (`body`, `semibold`, `condensed`, `decal`) |
| `widgets` | `chips`/`chips_with`/`chips_sized` (disabled reasons written under the row, `disabled_reasons`, `ChipSize::Compact`, one focus stop stepped with ←/→), `toggle` (disabled: keeps the saved value, reason under the label), `join_sentences`, `look_card` + `fit_title`, `game_card`, `outline_button`, `primary_button`, `progress_bar`, `folder_field`, `heading`/`field`/`note`/`error_note`, `heading_focused`/`field_focused`/`focus_bar` (SCR's row focus), `tabs_bar`, `play_plate`/`footer`, `tab_body`, `gradient`, `stripes`, `decode_jpeg`/`load_jpeg`/`hero_image`/`photo_caption`, `name_control` |
| `window` | `WindowFit` (final size at creation under Wine with Wine's DPI divided out; opt-in `with_native_fit` DPI fit + recentre off Wine; `screen()` in pixels), `window_zoom`, `creation_zoom`, `SIZE`, `Layout` |
| `spec` | `spec_row(ui, &OptionSpec, &mut String)` / `spec_row_with(…, RowOptions { disabled, steps })` / `spec_path_row(…, pick)` (an `unset_label` option adds an "As the game" chip): one row per core option (toggle, chips, slider, text/size field, folder), label + description as accessible name/description, refused values say what they need; `rendered_keys(ctx)` / `mark_rendered` / `clear_rendered` for `rea_launcher::cfg::spec::assert_parity` in a GUI test |
| `download` | `download_row(ui, &Download { label, size, state })` → `Option<DownloadAction>` (Start / Cancel / Remove); `DownloadState` (Missing{partial}, Connecting, Downloading{done,total,rate}, Verifying, Unpacking, Installed, Failed(msg)), `DownloadState::from_phase(Option<&Phase>, installed, partial)` + `with_rate`; `describe`, `size_text`, `time_left` |
| `testing` (feature) | `harness(&Palette, &Metrics, state, ui)`, `harness_sized(…, size, …)` (scroll with `node.scroll_to_me()` below the fold) on `egui_kittest`, `checked`/`disabled`/`description`/`value`, re-exported `Queryable` |
| `report` | A native (Linux) launcher's messages: `audience(Mode, Native) -> Audience` (`Terminal` / `Window` / `LogOnly`; `Native::here()` reads stdin/stderr `IsTerminal` and `DISPLAY`/`WAYLAND_DISPLAY`), `Message { title, text, link: Option<Link { label, url }>, details }`, `fatal(&Palette, &Metrics, &Message, Mode) -> Audience` (prints to stderr, then the window when the audience is `Window`), `show`/`show_with` (the window alone, pad navigation included), `message_ui` + `MessageState` + `Action` for drawing it inside another window or a test |
| `pad` | `inject` and `key_for`, plus `Nav`/`PadNav` re-exported from `rea_launcher::pad` (edge detection on XInput button masks; the bits are `rea_launcher::pad::DPAD_UP` …). `inject`: d-pad → Tab/Shift+Tab/arrows, A → Space; Start, LB/RB and B come back to the launcher ; Linux: `pad::joystick::Joysticks` (`new()`, `buttons()` → the XInput mask from every `/dev/input/js*` pad, read without blocking, rescanned every 2 s; `with_scan` for fake devices in tests) |

`eframe` and `egui` are re-exported (`rea_launcher_ui::egui`), so a
launcher needs no eframe dependency of its own.

## Metrics: each launcher's look

`theme::install_with(ctx, &PALETTE, &METRICS)`; unset fields keep the
SDK default (`Metrics::DEFAULT`, CMR4's card). The values each launcher's
own widgets drew:

| Field | Default | CMR3 | CMR4 | SCR | SRR | OR |
|---|---|---|---|---|---|---|
| `chip_pad` / `chip_gap` | 24 / 6 | – | – | 22 / 6 | – | – |
| `compact_chip_pad` / `_gap` | 14 / 5 | – | – | 14 / 5 | – | – |
| `button_text` | `Action` | – | – | – | – | `Ink` |
| `field_width` | 118 | – | – | 128 | – | – |
| `toggle_wrap` | true | – | false | false | false | false |
| `card.pad` | 12 | 14 | – | 12 | 14 | 14 |
| `card.title` | 22 | – | – | 20 | – | – (OR fits titles: `fit_title`) |
| `card.badge` / `badge_pad` / `badge_rise` | 12 / 6 / 9 | – | – | 11.5 / 5 / 8 | – | – |
| `card.note` / `note_top` | 13 / 30 | 13.5 / 32 | – | 12.5 / 28 | 13.5 / 32 | 13.5 / 32 |
| `Palette::selection` | – | `tint(GOLD, 128)` | `tint(ACCENT, 128)` | `tint(GOLD, 128)` | `tint(ORANGE, 128)` | `tint(RED, 153)` |

A hovered outline button always shows `ink` text on the `line` fill (CMR4's
orange on its hover fill was 3.4:1).

## Contrast and text size

`theme::contrast_problems(&PALETTE, &METRICS)` checks every pair the
widgets draw; since 0.2.0 that includes control borders: `line` needs 3:1
against `base` and `panel` (WCAG non-text contrast), and `line` is never a
fill under text (hover fills and the not-ready Play plate use `panel`).
Every launcher palette fails the new check as of v0.1.0; the least
lightening of `line` (towards white) that passes:

| Launcher | `line` now | vs base / panel | Passing `line` |
|---|---|---|---|
| CMR3 | `#8a2a42` | 1.70 / 1.41 | `#af6d7d` |
| CMR4 | `#3a3f48` | 1.70 / 1.45 | `#6b6e75` |
| SCR | `#283e6b` | 1.70 / 1.46 | `#5d6e90` |
| SRR | `#1f4a84` | 1.95 / 1.62 | `#5475a1` |
| OR | `#3d3430` | 1.61 / 1.40 | `#6d6663` |

### Light palettes

A palette whose `base` is light (`Palette::is_light`: black contrasts with
it more than white, as CMR2005's white menu band) gets egui's light style,
and egui's own widgets — sliders, number boxes, egui buttons, combo
boxes, tooltips — are filled from the palette: `panel` at rest, `panel`
tinted 6 % towards `ink` when hovered or open and 12 % when pressed, `ink`
strokes and links (`theme::visuals`). `contrast_problems` checks `ink` on
every one of those fills, in every state, and the link colour on `base`.
Before v0.4.0 every palette started from egui's dark style, so a light
palette's dark text sat on dark-grey widgets (1.5:1) and the check could
not see it. Dark palettes keep egui's dark fills.

Text sizes: body text (labels, notes, chips, buttons) is at least 13 pt
and nothing is under 11 pt (`theme::size`, `Metrics::text_sizes`); keep
`assert!(theme::text_size_problems(&METRICS).is_empty())` next to the
contrast test.

## Native messages (no Windows message box)

`rea_launcher_win32::report` covers Windows and Wine. A native Linux
launcher calls `rea_launcher_ui::report::fatal` for every error before
or instead of the settings window (game not found, unknown edition,
refused home), so an AppImage double-click never exits silently:

| Mode | Terminal attached | Desktop | Audience |
|---|---|---|---|
| `Play { interactive: true }` | any | yes | `Window` |
| `Command { interactive }` | yes | any | `Terminal` |
| `Command { interactive: true }` | no | yes | `Window` |
| any | yes | no | `Terminal` |
| otherwise (scripts, the helper's log, no desktop) | | | `LogOnly` |

The text is always printed to stderr too (the helper and the AppImage log
it). The window: the title, the text (scrolls when long), the link's
address in full, then **Copy details** (title, text, `details`, address),
the link's button (opens the browser) and **OK**, which has the focus so
Enter, Space or the pad's A close it; Escape, B and Start close it too.

## A download (core job → row → actions)

```rust
use std::sync::Arc;
use rea_launcher::download::{self, Job, Pin};
use rea_launcher_ui::download::{download_row, Download, DownloadAction, DownloadState};

// in the window: self.job: Option<Arc<Job>>, self.pin: Pin, self.root: PathBuf
let phase = self.job.as_ref().map(|job| job.phase());
let state = DownloadState::from_phase(
    phase.as_ref(),
    download::installed(&self.pin, &self.root).is_some(),
    download::partial(&self.pin, &self.root),
);
let row = Download { label: "HD texture pack", size: self.pin.size, state };
match download_row(ui, &row) {
    Some(DownloadAction::Start) => {
        let job = Arc::new(Job::default());
        self.job = Some(job.clone());
        let (pin, root) = (self.pin.clone(), self.root.clone());
        std::thread::spawn(move || {
            // Windows: rea_launcher_win32::http::WinHttp; free space from win32::disk.
            let fetch = rea_launcher_win32::http::WinHttp::new("MyLauncher/1.0");
            let free = |dir: &std::path::Path| rea_launcher_win32::disk::free_space(dir);
            download::install(&pin, &root, &free, &fetch, &job);
        });
    }
    Some(DownloadAction::Cancel) => if let Some(job) = &self.job { job.cancel() },
    Some(DownloadAction::Remove) => { let _ = download::remove(&self.pin, &self.root); self.job = None; }
    None => {}
}
if state.running() { ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)); }
```

## Accessibility and GUI tests

**A disabled control says why, on the window.** A pad player never
hovers, so a tooltip or the screen-reader description is not enough
(OR's lesson). Since v0.4.0 every widget writes the reason where the eye
already is, in `muted` (AA on the base):

| Control | Where the reason goes |
|---|---|
| `chips*` | a note under the row; the reason alone when every chip shares it, else `Label, Label: reason` per reason (`widgets::disabled_reasons`) |
| `toggle` | a second line under its label |
| `look_card` / `game_card` | in place of the note / subtitle |
| `spec_row_with(… RowOptions { disabled })` | in place of the row's description (the row keeps its height) |

Disabled chips and switches keep showing the saved value (a dimmed
`select` outline, the knob where it was). Accessible descriptions join
the note and the reason with `widgets::join_sentences`, so a note that
ends in "." no longer gets a second one.

Every widget reports its role, name, state and disabled reason to
AccessKit: chips are radio buttons in a radio group (the group's value is
the chosen label), toggles are switches, look and game cards radio buttons
with the note as description, tabs are tabs, the Play plate a button
disabled until ready (the footer hint says why), `folder_field`'s text
field is named by its label with the hint as description, the photo
strip is an image named by its alt text and its caption a label. Tabs
switch with Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PageDown / PageUp.

Launcher GUI tests use the `testing` feature (dev-dependency only):

```rust
use rea_launcher_ui::testing::{self, Queryable};

let mut ui = testing::harness(&PALETTE, &METRICS, Form::default(), |ui, form| draw(ui, form));
ui.get_by_label("120 fps").click();
ui.run();
assert_eq!(ui.state().fps, 120);
assert_eq!(testing::description(&ui.get_by_label("Glide")).as_deref(), Some("Needs the 3dfx build"));
```

They are headless (no window, no GPU) and run on the host and under
`scripts/wine-test`. Keep the contrast test too:
`assert!(theme::contrast_problems(&PALETTE, &METRICS).is_empty())`.

The bundled Barlow fonts are under the SIL Open Font License
(`assets/fonts/OFL.txt`, also `theme::OFL`); ship that text with the
launcher.

## Use

```toml
[dependencies]
rea-launcher-ui = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
```

```rust
use rea_launcher_ui::egui::{self, Color32};
use rea_launcher_ui::{eframe, pad, theme::{self, Metrics, Palette}, widgets, window::{Layout, WindowFit, SIZE}};

const PALETTE: Palette = Palette {
    base: Color32::from_rgb(0x15, 0x17, 0x1b),
    // ...and every other role, with the game's own values
};
const METRICS: Metrics = Metrics { toggle_wrap: false, ..Metrics::DEFAULT };

struct App { fit: WindowFit, pad: pad::PadNav, pending: Vec<pad::Nav>, fps: u32, vsync: bool }

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let navs = self.pad.update(xinput_buttons());
        pad::inject(ctx, raw, &navs);
        self.pending.extend(navs);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        self.fit.first_frame(ui.ctx());
        // Taken, so a second egui pass in one frame can't play twice.
        for nav in std::mem::take(&mut self.pending) {
            if nav == pad::Nav::Play { /* save, then close */ }
        }
        let layout = Layout::new(ui.max_rect(), 300.0);
        ui.scope_builder(egui::UiBuilder::new().max_rect(layout.body), |ui| {
            widgets::chips(ui, &[(60, "60"), (0, "Unlimited")], &mut self.fps, None);
            widgets::toggle(ui, &mut self.vsync, "VSync", None);
        });
        if widgets::footer(ui, layout.footer, "Settings are saved when you press Play.", true).clicked() {
            // save, then close
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
    }
}

fn show(wine: bool, screen: Option<(u32, u32)>, wine_scale: f32) -> eframe::Result {
    // wine_scale: rea_launcher_win32::display::wine_dpi_scale() under Wine, else 1.0
    let fit = WindowFit::new(SIZE, wine, screen, wine_scale).with_native_fit(true);
    let options = fit.native_options("My Game");
    eframe::run_native("My Game", options, Box::new(move |cc| {
        theme::install_with(&cc.egui_ctx, &PALETTE, &METRICS);
        Ok(Box::new(App { fit, pad: Default::default(), pending: Vec::new(), fps: 60, vsync: true }))
    }))
}
```

It depends on the portable `rea-launcher` crate (for `pad`), not on
`rea-launcher-win32`: the folder picker is a
closure passed to `folder_field`, and the pad reads a plain XInput button
mask: on Windows from `rea_launcher_win32::xinput::buttons()`, in a
native Linux window from `pad::joystick::Joysticks::buttons()`.

## Migration

What changes for a launcher when it moves to a new version of this crate.
Behaviour not listed is the same.

### v0.4.0

- **Light palettes** (CMR2005): `theme::install`/`set_palette` now give
  egui's own widgets light, palette-filled backgrounds and set egui's
  light theme. Delete a local fix such as CMR2005's
  `palette::light_widgets` (and its call after `install_with`); keep the
  palette test, which now also checks those fills. Dark palettes: no
  change.
- **Disabled reasons are visible.** A disabled chip row, switch, look or
  game card, or spec row now writes its reason on the window (table under
  "Accessibility and GUI tests"); rows grow by one note line where a
  launcher used a disabled chip row or switch outside spec rows. Remove a
  local copy that already shows it (OR's `chip_row`/`bool_row` write
  `disabled.unwrap_or(row.description)`: pass `row.description` only, or
  move to `spec_row_with`; Screamer's `pad_off_reason` note under a
  disabled row), or the reason shows twice. Keep reasons full sentences.
- **A disabled switch shows its saved value** (knob on the right, dimmed
  outline) instead of looking off. Tests that read `checked` on a
  disabled switch now see the saved value.
- **Accessible descriptions**: a disabled spec row's description is
  `description. reason` (it was `description reason`); a look card's
  `note. reason` no longer doubles the full stop.
- **Native Linux pads** (DiRT Rally): replace the local `jspad` module
  with `rea_launcher_ui::pad::joystick::Joysticks` (same events, mapping
  and 2-second rescan); `Pads::buttons()` becomes `Joysticks::buttons()`
  and the `libc` dependency can go if nothing else uses it.
- **Native Linux errors** (DiRT Rally): call
  `report::fatal(&PALETTE, &METRICS, &Message { … }, Mode::Play {
  interactive: !no_window })` where `linux::run` now only `eprintln!`s an
  error (bad home, `cli::check` failing, unknown edition — give that one
  the issue form as `link`), keeping the log line. `--help`/`--check`
  output uses `Mode::Command`.
