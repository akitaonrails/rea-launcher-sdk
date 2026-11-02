//! A download row: what is offered, its size, progress while it runs, and
//! Download / Cancel / Remove (OR's HD-pack tab, made generic).
//!
//! The row draws a [`DownloadState`] and returns what the player asked
//! for; the launcher owns the job (a background thread running
//! `rea_launcher::download::install`) and maps its phase onto the state
//! each frame with [`DownloadState::from_phase`]:
//!
//! ```
//! use rea_launcher_ui::download::{self, Download, DownloadAction, DownloadState};
//! # fn ui(ui: &mut rea_launcher_ui::egui::Ui) {
//! let row = Download {
//!     label: "HD texture pack",
//!     size: 326_000_000,
//!     state: DownloadState::Downloading { done: 120_000_000, total: 326_000_000, rate: Some(8e6) },
//! };
//! match download::download_row(ui, &row) {
//!     Some(DownloadAction::Start) => { /* spawn the job */ }
//!     Some(DownloadAction::Cancel) => { /* job.cancel() */ }
//!     Some(DownloadAction::Remove) => { /* delete the installed copy */ }
//!     None => {}
//! }
//! # }
//! ```
//!
//! Accessibility: the status line is a label, the bar a progress
//! indicator with the fraction as its value, the buttons are buttons
//! named by their text, and a failure is an error line.

use eframe::egui::{self, Align, Ui};
use rea_launcher::download::Phase;

use crate::widgets;

/// Where a download is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DownloadState<'a> {
    /// Not there; `partial` bytes from an earlier attempt can resume.
    Missing {
        partial: u64,
    },
    /// Started, nothing received yet.
    Connecting,
    /// `rate` in bytes per second, when known.
    Downloading {
        done: u64,
        total: u64,
        rate: Option<f64>,
    },
    /// Checking the downloaded file.
    Verifying,
    Unpacking {
        done: u64,
        total: u64,
    },
    /// Installed and in use.
    Installed,
    /// The last attempt failed with this message.
    Failed(&'a str),
}

impl<'a> DownloadState<'a> {
    /// The state for the core download job's `phase` (`None`: no job this
    /// session), with what is on disk: `installed`
    /// (`download::installed(&pin, root).is_some()`) and `partial`
    /// (`download::partial(&pin, root)`). A finished job is `Installed`; a
    /// cancelled one, or none, falls back to the disk. Core reports no
    /// rate; add one with [`DownloadState::with_rate`].
    pub fn from_phase(
        phase: Option<&'a Phase>,
        installed: bool,
        partial: u64,
    ) -> DownloadState<'a> {
        let on_disk = if installed {
            DownloadState::Installed
        } else {
            DownloadState::Missing { partial }
        };
        match phase {
            None | Some(Phase::Cancelled) => on_disk,
            Some(Phase::Starting) => DownloadState::Connecting,
            Some(&Phase::Downloading { done, total }) => DownloadState::Downloading {
                done,
                total,
                rate: None,
            },
            Some(Phase::Verifying) => DownloadState::Verifying,
            Some(&Phase::Unpacking { done, total }) => DownloadState::Unpacking { done, total },
            Some(Phase::Done(_)) => DownloadState::Installed,
            Some(Phase::Failed(message)) => DownloadState::Failed(message),
        }
    }

    /// Adds a download rate (bytes per second) to a `Downloading` state.
    pub fn with_rate(self, rate: Option<f64>) -> DownloadState<'a> {
        match self {
            DownloadState::Downloading { done, total, .. } => {
                DownloadState::Downloading { done, total, rate }
            }
            other => other,
        }
    }

    /// Whether a job is running (Cancel is offered).
    pub fn running(&self) -> bool {
        matches!(
            self,
            DownloadState::Connecting
                | DownloadState::Downloading { .. }
                | DownloadState::Verifying
                | DownloadState::Unpacking { .. }
        )
    }
}

/// One downloadable thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Download<'a> {
    pub label: &'a str,
    /// Download size in bytes.
    pub size: u64,
    pub state: DownloadState<'a>,
}

/// What the player pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadAction {
    /// Download (or resume, or retry).
    Start,
    Cancel,
    Remove,
}

/// `bytes` as `326 MB` or `1.4 GB`.
pub fn size_text(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 1000.0 {
        format!("{:.1} GB", mb / 1000.0)
    } else {
        format!("{mb:.0} MB")
    }
}

/// "under a minute left", "about 3 min left".
pub fn time_left(seconds: f64) -> String {
    match seconds as u64 {
        0..=59 => "under a minute left".to_owned(),
        s => format!("about {} min left", s.div_ceil(60)),
    }
}

/// The status line, the bar's fraction (`None`: no bar) and the button
/// offered, for a state.
pub fn describe(download: &Download<'_>) -> (String, Option<f32>, Option<DownloadAction>) {
    let fraction = |done: u64, total: u64| Some((done as f64 / total.max(1) as f64) as f32);
    match download.state {
        DownloadState::Missing { partial: 0 } => (
            format!("{}, {}", download.label, size_text(download.size)),
            None,
            Some(DownloadAction::Start),
        ),
        DownloadState::Missing { partial } => (
            format!(
                "{}: {} of {} downloaded; Download resumes",
                download.label,
                size_text(partial),
                size_text(download.size)
            ),
            fraction(partial, download.size),
            Some(DownloadAction::Start),
        ),
        DownloadState::Connecting => (
            "Connecting…".to_owned(),
            Some(0.0),
            Some(DownloadAction::Cancel),
        ),
        DownloadState::Downloading { done, total, rate } => {
            let speed = match rate {
                Some(rate) if rate > 0.0 => format!(
                    " · {:.1} MB/s · {}",
                    rate / 1_000_000.0,
                    time_left(total.saturating_sub(done) as f64 / rate)
                ),
                _ => String::new(),
            };
            (
                format!(
                    "Downloading {} of {}{speed}",
                    size_text(done),
                    size_text(total)
                ),
                fraction(done, total),
                Some(DownloadAction::Cancel),
            )
        }
        DownloadState::Verifying => (
            "Checking the download…".to_owned(),
            Some(1.0),
            Some(DownloadAction::Cancel),
        ),
        DownloadState::Unpacking { done, total } => (
            format!("Unpacking {} of {}", size_text(done), size_text(total)),
            fraction(done, total),
            Some(DownloadAction::Cancel),
        ),
        DownloadState::Installed => (
            format!("{} is installed", download.label),
            None,
            Some(DownloadAction::Remove),
        ),
        DownloadState::Failed(_) => (
            format!("{}, {}", download.label, size_text(download.size)),
            None,
            Some(DownloadAction::Start),
        ),
    }
}

/// The button text for an action.
pub fn action_text(action: DownloadAction) -> &'static str {
    match action {
        DownloadAction::Start => "Download",
        DownloadAction::Cancel => "Cancel",
        DownloadAction::Remove => "Remove",
    }
}

/// Draws the row; the action pressed this frame, if any. Download is the
/// primary button; Cancel and Remove are outline buttons.
pub fn download_row(ui: &mut Ui, download: &Download<'_>) -> Option<DownloadAction> {
    let (status, fraction, offered) = describe(download);
    let mut pressed = None;
    ui.vertical(|ui| {
        if let Some(fraction) = fraction {
            widgets::progress_bar(ui, fraction);
            ui.add_space(6.0);
        }
        ui.horizontal(|ui| {
            widgets::note(ui, &status);
            if let Some(action) = offered {
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let response = match action {
                        DownloadAction::Start => widgets::primary_button(ui, action_text(action)),
                        _ => widgets::outline_button(ui, action_text(action)),
                    };
                    if response.clicked() {
                        pressed = Some(action);
                    }
                });
            }
        });
        if let DownloadState::Failed(message) = download.state {
            widgets::error_note(ui, message);
        }
    });
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{self, NodeT, Queryable};
    use crate::theme::{Metrics, Palette};
    use egui::Role;

    const MB: u64 = 1_000_000;

    fn row(state: DownloadState<'_>) -> Download<'_> {
        Download {
            label: "HD pack",
            size: 326 * MB,
            state,
        }
    }

    #[test]
    fn core_phases_map_onto_states() {
        use std::path::PathBuf;
        fn from(phase: Option<&Phase>, installed: bool, partial: u64) -> DownloadState<'_> {
            DownloadState::from_phase(phase, installed, partial)
        }
        assert_eq!(from(None, false, 0), DownloadState::Missing { partial: 0 });
        assert_eq!(from(None, false, 5), DownloadState::Missing { partial: 5 });
        assert_eq!(from(None, true, 0), DownloadState::Installed);
        assert_eq!(
            from(Some(&Phase::Starting), false, 0),
            DownloadState::Connecting
        );
        let downloading = Phase::Downloading { done: 1, total: 2 };
        assert_eq!(
            from(Some(&downloading), false, 0).with_rate(Some(3.0)),
            DownloadState::Downloading {
                done: 1,
                total: 2,
                rate: Some(3.0)
            }
        );
        assert_eq!(
            from(Some(&Phase::Verifying), false, 0),
            DownloadState::Verifying
        );
        assert_eq!(
            from(Some(&Phase::Unpacking { done: 1, total: 4 }), false, 0),
            DownloadState::Unpacking { done: 1, total: 4 }
        );
        let done = Phase::Done(PathBuf::from("pack"));
        assert_eq!(from(Some(&done), false, 0), DownloadState::Installed);
        let failed = Phase::Failed("checksum mismatch".into());
        assert_eq!(
            from(Some(&failed), false, 9),
            DownloadState::Failed("checksum mismatch")
        );
        // Cancelled: what is on disk (the partial file resumes).
        assert_eq!(
            from(Some(&Phase::Cancelled), false, 9),
            DownloadState::Missing { partial: 9 }
        );
        assert_eq!(
            DownloadState::Installed.with_rate(Some(1.0)),
            DownloadState::Installed
        );
        for phase in [Phase::Starting, downloading.clone(), Phase::Verifying] {
            assert_eq!(from(Some(&phase), false, 0).running(), phase.running());
        }
    }

    #[test]
    fn each_state_says_where_it_is_and_offers_one_action() {
        let missing = describe(&row(DownloadState::Missing { partial: 0 }));
        assert_eq!(
            missing,
            ("HD pack, 326 MB".into(), None, Some(DownloadAction::Start))
        );
        let (text, fraction, action) = describe(&row(DownloadState::Missing { partial: 163 * MB }));
        assert!(text.contains("163 MB of 326 MB"), "{text}");
        assert_eq!(fraction, Some(0.5));
        assert_eq!(action, Some(DownloadAction::Start));
        let (text, fraction, action) = describe(&row(DownloadState::Downloading {
            done: 100 * MB,
            total: 400 * MB,
            rate: Some(2.0 * MB as f64),
        }));
        assert_eq!(
            text,
            "Downloading 100 MB of 400 MB · 2.0 MB/s · about 3 min left"
        );
        assert_eq!(fraction, Some(0.25));
        assert_eq!(action, Some(DownloadAction::Cancel));
        assert_eq!(
            describe(&row(DownloadState::Unpacking {
                done: 1_400 * MB,
                total: 2_800 * MB
            }))
            .0,
            "Unpacking 1.4 GB of 2.8 GB"
        );
        assert_eq!(
            describe(&row(DownloadState::Installed)).2,
            Some(DownloadAction::Remove)
        );
        assert_eq!(
            describe(&row(DownloadState::Failed("no network"))).2,
            Some(DownloadAction::Start)
        );
        assert!(DownloadState::Verifying.running());
        assert!(!DownloadState::Installed.running());
        assert_eq!(time_left(30.0), "under a minute left");
        assert_eq!(time_left(121.0), "about 3 min left");
        // A zero total never divides by zero.
        assert_eq!(
            describe(&row(DownloadState::Unpacking { done: 0, total: 0 })).1,
            Some(0.0)
        );
    }

    struct Job {
        state: &'static str,
        pressed: Vec<DownloadAction>,
    }

    fn window(ui: &mut Ui, job: &mut Job) {
        let state = match job.state {
            "missing" => DownloadState::Missing { partial: 0 },
            "running" => DownloadState::Downloading {
                done: 163 * MB,
                total: 326 * MB,
                rate: None,
            },
            "installed" => DownloadState::Installed,
            _ => DownloadState::Failed("The server answered 404"),
        };
        if let Some(action) = download_row(ui, &row(state)) {
            job.pressed.push(action);
        }
    }

    fn open(state: &'static str) -> testing::Harness<'static, Job> {
        testing::harness(
            &Palette::default(),
            &Metrics::DEFAULT,
            Job {
                state,
                pressed: Vec::new(),
            },
            window,
        )
    }

    #[test]
    fn the_buttons_report_the_action_and_progress_is_accessible() {
        let mut ui = open("missing");
        assert!(ui.query_by_role(Role::ProgressIndicator).is_none());
        ui.get_by_role_and_label(Role::Button, "Download").click();
        ui.run();
        assert_eq!(ui.state().pressed, [DownloadAction::Start]);

        ui.state_mut().state = "running";
        ui.run();
        let bar = ui.get_by_role(Role::ProgressIndicator);
        assert_eq!(bar.accesskit_node().numeric_value(), Some(0.5));
        assert!(ui.query_by_label_contains("Downloading 163 MB").is_some());
        ui.get_by_role_and_label(Role::Button, "Cancel").click();
        ui.run();
        assert_eq!(ui.state().pressed.last(), Some(&DownloadAction::Cancel));

        ui.state_mut().state = "installed";
        ui.run();
        ui.get_by_role_and_label(Role::Button, "Remove").click();
        ui.run();
        assert_eq!(ui.state().pressed.last(), Some(&DownloadAction::Remove));
    }

    #[test]
    fn a_failure_shows_its_message_and_offers_a_retry() {
        let ui = open("failed");
        assert!(ui.query_by_label("The server answered 404").is_some());
        assert!(
            ui.query_by_role_and_label(Role::Button, "Download")
                .is_some()
        );
    }
}
