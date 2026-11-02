//! Window sizing and the page layout.
//!
//! From CMR4/OR `ui.rs`, with OR's DPI fix (commit ee8bf02):
//!
//! - **Under Wine** the window is created at its *final* size and never
//!   resized: on XWayland with fractional scaling a resize after creation
//!   leaves the surface out of step with winit (picture squashed into a
//!   corner, clicks landing on the wrong widgets; CMR3 `RUNTIME_NOTES.md`).
//!   egui-winit creates a *logical* size, which Wine multiplies by its own
//!   DPI scale, so that scale is divided out: pass
//!   `rea_launcher_win32::display::wine_dpi_scale()` as `wine_scale`.
//! - **On Windows** (and natively on Linux) the window starts at 1x and the
//!   first frame fits it to the DPI-aware scale, then recentres it.
//!
//! ```no_run
//! # use rea_launcher_ui::window::{WindowFit, SIZE};
//! # let (wine, screen, wine_scale) = (false, None, 1.0);
//! let mut fit = WindowFit::new(SIZE, wine, screen, wine_scale);
//! let options = fit.native_options("Colin McRae Rally 04");
//! // in App::ui, every frame (it acts once):
//! # let ctx = eframe::egui::Context::default();
//! fit.first_frame(&ctx);
//! ```

use eframe::egui::{self, Rect, Vec2, pos2, vec2};

/// The usual design size, in points (CMR3/CMR4). Pass your own when the
/// launcher's page is taller (OR: 880×760).
pub const SIZE: Vec2 = vec2(880.0, 640.0);

/// How much larger to draw a `design`-sized window on `screen` (pixels),
/// on top of the platform's `native` scale: 1x up to 1440 pixels on the
/// shorter side, then in step with it (1.5x at 4K), never more than 90 % of
/// the screen, in steps of 0.05. It never shrinks below the native scale,
/// unless the window would not fit on the screen at all at that scale.
pub fn window_zoom(design: Vec2, screen: Option<(u32, u32)>, native: f32) -> f32 {
    let native = if native > 0.0 { native } else { 1.0 };
    let Some((width, height)) = screen.filter(|&(w, h)| w > 0 && h > 0) else {
        return 1.0;
    };
    let (width, height) = (width as f32, height as f32);
    let full = (width / design.x).min(height / design.y);
    let target = (width.min(height) / 1440.0).max(1.0).min(0.9 * full);
    let zoom = (target / native).max((full / native).min(1.0));
    // Steps of 0.05, but never past the screen.
    let rounded = (zoom * 20.0).round() / 20.0;
    if rounded * native > full {
        (zoom * 20.0).floor() / 20.0
    } else {
        rounded
    }
}

/// The zoom the window is created with: under Wine the final one (with
/// Wine's DPI scale divided out), elsewhere 1.0.
pub fn creation_zoom(design: Vec2, wine: bool, screen: Option<(u32, u32)>, wine_scale: f32) -> f32 {
    if wine {
        window_zoom(design, screen, wine_scale)
    } else {
        1.0
    }
}

/// Sizes the settings window: [`WindowFit::native_options`] before
/// `eframe::run_native`, [`WindowFit::first_frame`] at the top of every
/// frame.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowFit {
    design: Vec2,
    wine: bool,
    screen: Option<(u32, u32)>,
    zoom: f32,
    fitted: bool,
}

impl WindowFit {
    /// `screen` is the physical size of the screen the window opens on
    /// (`rea_launcher_win32::display::screen_size()`; `None` natively on
    /// Linux, where egui's monitor size is used). `wine_scale` is ignored
    /// unless `wine`.
    pub fn new(design: Vec2, wine: bool, screen: Option<(u32, u32)>, wine_scale: f32) -> WindowFit {
        WindowFit {
            design,
            wine,
            screen,
            zoom: creation_zoom(design, wine, screen, wine_scale),
            fitted: false,
        }
    }

    /// The current zoom (final after the first frame).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The window size in points the window is created with.
    pub fn creation_size(&self) -> Vec2 {
        self.design * self.zoom
    }

    /// A fixed-size, non-resizable window. Under Wine it is not centred:
    /// the desktop's window manager places it on the monitor in use, while
    /// Wine's idea of the main screen can be another monitor.
    pub fn native_options(&self, title: &str) -> eframe::NativeOptions {
        let size = self.creation_size();
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title(title)
                .with_inner_size(size)
                .with_min_inner_size(size)
                .with_max_inner_size(size)
                .with_resizable(false)
                .with_maximize_button(false),
            centered: !self.wine,
            ..Default::default()
        }
    }

    /// Sets egui's zoom on the first frame; off Wine also resizes for the
    /// DPI-aware scale and recentres. Does nothing on later frames.
    pub fn first_frame(&mut self, ctx: &egui::Context) {
        if self.fitted {
            return;
        }
        self.fitted = true;
        if !self.wine {
            let native = ctx.native_pixels_per_point().unwrap_or(1.0);
            let screen = self.screen.or_else(|| {
                ctx.input(|input| input.viewport().monitor_size)
                    .map(|size| physical(size, native))
            });
            self.zoom = window_zoom(self.design, screen, native);
        }
        if self.zoom == 1.0 {
            return;
        }
        ctx.set_zoom_factor(self.zoom);
        if self.wine {
            return;
        }
        let size = self.design * self.zoom;
        for command in [
            egui::ViewportCommand::MinInnerSize(size),
            egui::ViewportCommand::MaxInnerSize(size),
            egui::ViewportCommand::InnerSize(size),
        ] {
            ctx.send_viewport_cmd(command);
        }
        if let Some(monitor) = ctx.input(|input| input.viewport().monitor_size) {
            let at = ((monitor - size) / 2.0).max(Vec2::ZERO);
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(at.to_pos2()));
        }
    }
}

/// egui's monitor size (points) in pixels.
fn physical(points: Vec2, native: f32) -> (u32, u32) {
    let size = points * native;
    (
        size.x.round().max(0.0) as u32,
        size.y.round().max(0.0) as u32,
    )
}

/// The page every launcher draws: a photo strip on the left, the body
/// (title, status, tabs, tab content) and a footer with the Play plate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub photo: Rect,
    pub body: Rect,
    pub footer: Rect,
}

impl Layout {
    /// Footer height in points.
    pub const FOOTER: f32 = 52.0;

    /// CMR4/OR's margins for a window `full` with a `photo_width` strip.
    pub fn new(full: Rect, photo_width: f32) -> Layout {
        let photo = Rect::from_min_size(full.min, vec2(photo_width, full.height()));
        let content = Rect::from_min_max(
            pos2(full.left() + photo_width + 12.0, full.top() + 24.0),
            pos2(full.right() - 32.0, full.bottom() - 24.0),
        );
        Layout {
            photo,
            body: Rect::from_min_max(
                content.min,
                pos2(content.right(), content.bottom() - Self::FOOTER - 16.0),
            ),
            footer: Rect::from_min_max(
                pos2(content.left(), content.bottom() - Self::FOOTER),
                content.max,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OR: Vec2 = vec2(880.0, 760.0);

    #[test]
    fn the_window_grows_with_the_screen() {
        assert_eq!(window_zoom(SIZE, Some((1920, 1080)), 1.0), 1.0);
        assert_eq!(window_zoom(SIZE, Some((3840, 2160)), 1.0), 1.5);
        assert_eq!(window_zoom(SIZE, Some((3840, 2160)), 1.5), 1.0);
        assert_eq!(window_zoom(SIZE, Some((1280, 800)), 1.0), 1.0);
        assert_eq!(window_zoom(SIZE, Some((2560, 1440)), 1.0), 1.0);
        assert_eq!(window_zoom(SIZE, None, 1.0), 1.0);
        assert_eq!(window_zoom(SIZE, Some((0, 0)), 1.0), 1.0);
        // A nonsense scale counts as 1.
        assert_eq!(window_zoom(SIZE, Some((3840, 2160)), 0.0), 1.5);
    }

    #[test]
    fn under_wine_the_dpi_scale_is_divided_out() {
        // Wine at 144 or 192 DPI already scales the window: never zoomed
        // on top of that (OR came out 2.25x and 3x, taller than 4K).
        assert_eq!(creation_zoom(OR, true, Some((3840, 2160)), 1.0), 1.5);
        assert_eq!(creation_zoom(OR, true, Some((3840, 2160)), 1.5), 1.0);
        assert_eq!(creation_zoom(OR, true, Some((3840, 2160)), 2.0), 1.0);
        assert_eq!(creation_zoom(OR, true, Some((1920, 1080)), 1.0), 1.0);
        for native in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for screen in [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 800)] {
                let zoom = creation_zoom(OR, true, Some(screen), native);
                let physical = OR * zoom * native;
                assert!(
                    physical.y <= screen.1 as f32 && physical.x <= screen.0 as f32,
                    "{screen:?} at {native}: {physical:?}"
                );
            }
        }
        // Windows starts at 1x; the first frame fits the DPI-aware scale.
        assert_eq!(creation_zoom(OR, false, Some((3840, 2160)), 1.5), 1.0);
    }

    #[test]
    fn a_high_scale_on_a_small_screen_shrinks_to_fit() {
        // 1080p at 200 %: 2x would be 1520 points tall.
        let zoom = window_zoom(SIZE, Some((1920, 1080)), 2.0);
        assert!(zoom < 1.0);
        assert!(SIZE.y * zoom * 2.0 <= 1080.0);
        // OR's taller page on a Steam Deck stays at 1x (95 % of the height).
        assert_eq!(window_zoom(OR, Some((1280, 800)), 1.0), 1.0);
    }

    #[test]
    fn the_fit_is_created_at_its_final_size_under_wine() {
        let fit = WindowFit::new(SIZE, true, Some((3840, 2160)), 1.0);
        assert_eq!(fit.zoom(), 1.5);
        assert_eq!(fit.creation_size(), vec2(1320.0, 960.0));
        assert!(!fit.native_options("t").centered);
        let windows = WindowFit::new(SIZE, false, Some((3840, 2160)), 2.0);
        assert_eq!(windows.creation_size(), SIZE);
        assert!(windows.native_options("t").centered);
    }

    #[test]
    fn first_frame_fits_once() {
        let ctx = egui::Context::default();
        let mut fit = WindowFit::new(SIZE, false, Some((3840, 2160)), 1.0);
        ctx.run_ui(egui::RawInput::default(), |ui| fit.first_frame(ui.ctx()))
            .textures_delta
            .clear();
        assert_eq!(fit.zoom(), 1.5);
        // egui applies the zoom from the next frame.
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        assert_eq!(ctx.zoom_factor(), 1.5);
        fit.zoom = 7.0;
        fit.first_frame(&ctx);
        assert_eq!(fit.zoom(), 7.0, "later frames change nothing");
    }

    #[test]
    fn layout_matches_the_launchers() {
        let full = Rect::from_min_size(pos2(0.0, 0.0), SIZE);
        let layout = Layout::new(full, 300.0);
        assert_eq!(
            layout.photo,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 640.0))
        );
        assert_eq!(
            layout.body,
            Rect::from_min_max(pos2(312.0, 24.0), pos2(848.0, 548.0))
        );
        assert_eq!(
            layout.footer,
            Rect::from_min_max(pos2(312.0, 564.0), pos2(848.0, 616.0))
        );
        assert_eq!(physical(vec2(2560.0, 1440.0), 1.5), (3840, 2160));
    }
}
