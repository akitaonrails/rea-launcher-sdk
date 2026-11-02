# rea-launcher-ui

The settings window kit of the rea launchers (eframe 0.37, glow): the shared
look with each game's colours, art and tabs left to the launcher.

| Module | What |
|---|---|
| `theme` | `Palette` (colour roles: base, panel, line, ink, muted, action, select, error, disabled, focus), `install`, `palette`, `contrast`/`Palette::contrast_problems`, Barlow fonts (`body`, `semibold`, `condensed`, `decal`) |
| `widgets` | `chips`/`chips_with` (disabled reasons, one focus stop stepped with ←/→), `toggle`, `look_card` + `fit_title`, `outline_button`, `primary_button`, `progress_bar`, `folder_field`, `heading`/`field`/`note`/`error_note`, `tabs_bar`, `play_plate`/`footer`, `tab_body`, `gradient`, `stripes`, `decode_jpeg`/`load_jpeg`/`hero_image` |
| `window` | `WindowFit` (final size at creation under Wine with Wine's DPI divided out; DPI-aware fit + recentre on Windows), `window_zoom`, `creation_zoom`, `SIZE`, `Layout` |
| `pad` | `inject` and `key_for`, plus `Nav`/`PadNav` re-exported from `rea_launcher::pad` (edge detection on XInput button masks; the bits are `rea_launcher::pad::DPAD_UP` …). `inject`: d-pad → Tab/Shift+Tab/arrows, A → Space; Start, LB/RB and B come back to the launcher |

The bundled Barlow fonts are under the SIL Open Font License
(`assets/fonts/OFL.txt`, also `theme::OFL`); ship that text with the
launcher.

## Use

```toml
[dependencies]
rea-launcher-ui = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
```

```rust
use eframe::egui::{self, Color32};
use rea_launcher_ui::{pad, theme::{self, Palette}, widgets, window::{Layout, WindowFit, SIZE}};

const PALETTE: Palette = Palette {
    base: Color32::from_rgb(0x15, 0x17, 0x1b),
    // ...and every other role, with the game's own values
};

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
    let fit = WindowFit::new(SIZE, wine, screen, wine_scale);
    let options = fit.native_options("My Game");
    eframe::run_native("My Game", options, Box::new(move |cc| {
        theme::install(&cc.egui_ctx, &PALETTE);
        Ok(Box::new(App { fit, pad: Default::default(), pending: Vec::new(), fps: 60, vsync: true }))
    }))
}
```

Keep a test in the launcher: `assert!(PALETTE.contrast_problems().is_empty())`.

It depends on the portable `rea-launcher` crate (for `pad`), not on
`rea-launcher-win32`: the folder picker is a
closure passed to `folder_field`, and the pad reads a plain XInput button
mask, so SCR's native Linux build can feed it from `/dev/input/js*`.
