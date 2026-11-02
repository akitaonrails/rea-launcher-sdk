# rea-launcher-ui

The settings window kit of the rea launchers (eframe 0.37, glow): the shared
look with each game's colours, art and tabs left to the launcher.

| Module | What |
|---|---|
| `theme` | `Palette` (colour roles: base, panel, line, ink, muted, action, select, error, disabled, focus, selection; `tint` for a const translucent colour), `Metrics` (sizes and style choices, below), `install`/`install_with`, `palette`/`metrics`, `contrast`, `contrast_problems(&Palette, &Metrics)` / `Palette::pairs`, Barlow fonts (`body`, `semibold`, `condensed`, `decal`) |
| `widgets` | `chips`/`chips_with`/`chips_sized` (disabled reasons, `ChipSize::Compact`, one focus stop stepped with ←/→), `toggle`, `look_card` + `fit_title`, `game_card`, `outline_button`, `primary_button`, `progress_bar`, `folder_field`, `heading`/`field`/`note`/`error_note`, `heading_focused`/`field_focused`/`focus_bar` (SCR's row focus), `tabs_bar`, `play_plate`/`footer`, `tab_body`, `gradient`, `stripes`, `decode_jpeg`/`load_jpeg`/`hero_image` |
| `window` | `WindowFit` (final size at creation under Wine with Wine's DPI divided out; opt-in `with_native_fit` DPI fit + recentre off Wine; `screen()` in pixels), `window_zoom`, `creation_zoom`, `SIZE`, `Layout` |
| `spec` | `spec_row(ui, &OptionSpec, &mut String)` / `spec_path_row(…, pick)`: one row per core option (toggle, chips, slider, text/size field, folder), label + description as accessible name/description, refused values say what they need; `rendered_keys(ctx)` / `mark_rendered` / `clear_rendered` for `rea_launcher::cfg::spec::assert_parity` in a GUI test |
| `download` | `download_row(ui, &Download { label, size, state })` → `Option<DownloadAction>` (Start / Cancel / Remove); `DownloadState` (Missing{partial}, Connecting, Downloading{done,total,rate}, Verifying, Unpacking, Installed, Failed(msg)), `DownloadState::from_phase(Option<&Phase>, installed, partial)` + `with_rate`; `describe`, `size_text`, `time_left` |
| `testing` (feature) | `harness(&Palette, &Metrics, state, ui)` on `egui_kittest`, `checked`/`disabled`/`description`/`value`, re-exported `Queryable` |
| `pad` | `inject` and `key_for`, plus `Nav`/`PadNav` re-exported from `rea_launcher::pad` (edge detection on XInput button masks; the bits are `rea_launcher::pad::DPAD_UP` …). `inject`: d-pad → Tab/Shift+Tab/arrows, A → Space; Start, LB/RB and B come back to the launcher |

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

Text sizes: body text (labels, notes, chips, buttons) is at least 13 pt
and nothing is under 11 pt (`theme::size`, `Metrics::text_sizes`); keep
`assert!(theme::text_size_problems(&METRICS).is_empty())` next to the
contrast test.

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

Every widget reports its role, name, state and disabled reason to
AccessKit: chips are radio buttons in a radio group (the group's value is
the chosen label), toggles are switches, look and game cards radio buttons
with the note as description, tabs are tabs, the Play plate a button
disabled until ready, `folder_field`'s text field is named by its hint.

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
mask, so SCR's native Linux build can feed it from `/dev/input/js*`.
