//! Reviewed non-destructive ReplayGain scans and durable job history.

mod grouping;
mod service;
#[cfg(test)]
mod tests;

use crate::ui::player::Player;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, Render, Role, SharedString, Task, TestSupportExt as _, Window,
    div, px, uniform_list,
};
use parabolica_domain::{
    gain::PeakMethod,
    ids::{JobId, MediaFileId, ReleaseId},
};
use parabolica_storage::replaygain::{ScanMode, ScanState};
use service::{Command, ScannerService, Status};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

pub struct Scanner {
    service: Option<Arc<ScannerService>>,
    player: Entity<Player>,
    status: Status,
    selected: HashSet<MediaFileId>,
    requested_album: Option<ReleaseId>,
    mode: ScanMode,
    method: PeakMethod,
    accepted: bool,
    showing: Option<JobId>,
    revision: u64,
    library_revision: u64,
    _poll: Task<()>,
}
impl Scanner {
    pub fn new(player: Entity<Player>, cx: &mut Context<Self>) -> Self {
        let service = crate::ui::library::storage(cx)
            .map(ScannerService::start)
            .map(Arc::new);
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        let Some(service) = &this.service else {
                            return;
                        };
                        service.set_playing(this.player.read(cx).is_playing());
                        let status = service.status();
                        let library_revision = crate::ui::library::revision(cx);
                        if library_revision != this.library_revision && !status.busy {
                            this.library_revision = library_revision;
                            if let Err(error) = service.send(Command::Refresh) {
                                this.status.error = Some(error);
                            }
                        }
                        if this.revision != status.revision {
                            this.revision = status.revision;
                            if this.status.busy && !status.busy && status.completed_scan.is_some() {
                                this.showing = status.completed_scan;
                            }
                            this.status = status;
                            if !this.status.busy
                                && let Some(album) = this.requested_album.take()
                            {
                                this.selected = this
                                    .status
                                    .candidates
                                    .iter()
                                    .filter(|c| c.album == album)
                                    .map(|c| c.file)
                                    .collect();
                                this.changed(cx);
                            }
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            service,
            player,
            status: Status::default(),
            selected: HashSet::new(),
            requested_album: None,
            mode: ScanMode::Tags,
            method: PeakMethod::Sample,
            accepted: false,
            showing: None,
            revision: 0,
            library_revision: 0,
            _poll: poll,
        }
    }
    pub(crate) fn select_album(&mut self, album: ReleaseId, cx: &mut Context<Self>) {
        self.requested_album = Some(album);
        self.changed(cx);
        if !self.status.busy {
            self.send(Command::Refresh, cx);
        }
    }
    pub(crate) fn select_files(&mut self, files: Vec<MediaFileId>, cx: &mut Context<Self>) {
        self.requested_album = None;
        self.selected = files.into_iter().collect();
        self.changed(cx);
        if !self.status.busy {
            self.send(Command::Refresh, cx);
        }
    }
    fn send(&mut self, command: Command, cx: &mut Context<Self>) {
        if let Some(service) = &self.service {
            match service.send(command) {
                Ok(()) => self.status = service.status(),
                Err(error) => self.status.error = Some(error),
            }
        }
        cx.notify();
    }
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.accepted = false;
        self.status.preview = Arc::default();
        if let Some(service) = &self.service {
            service.clear_review();
        }
        self.showing = None;
        cx.notify();
    }
    fn settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        v_flex()
            .w(px(248.))
            .flex_shrink_0()
            .gap_3()
            .child(label("analysis-settings", "Analysis settings".into()).font_semibold())
            .child(
                RadioGroup::vertical("scan-grouping-mode")
                    .selected_index(Some(match self.mode {
                        ScanMode::Tracks => 0,
                        ScanMode::Album => 1,
                        ScanMode::Tags => 2,
                        ScanMode::Disc => 3,
                    }))
                    .disabled(busy)
                    .children(
                        [
                            ScanMode::Tracks,
                            ScanMode::Album,
                            ScanMode::Tags,
                            ScanMode::Disc,
                        ]
                        .into_iter()
                        .map(|mode| Radio::new(mode.label()).label(mode.label())),
                    )
                    .on_change(cx.listener(|this, index, _, cx| {
                        this.mode = [
                            ScanMode::Tracks,
                            ScanMode::Album,
                            ScanMode::Tags,
                            ScanMode::Disc,
                        ][*index];
                        this.changed(cx);
                    })),
            )
            .child(
                label("peak-setting-label", "Peak measurement".into())
                    .mt_3()
                    .font_semibold(),
            )
            .child(
                RadioGroup::vertical("scan-peak-mode")
                    .selected_index(Some(if self.method == PeakMethod::Sample {
                        0
                    } else {
                        1
                    }))
                    .disabled(busy)
                    .children(
                        [PeakMethod::Sample, PeakMethod::TruePeak]
                            .into_iter()
                            .map(|method| Radio::new(peak_label(method)).label(peak_label(method))),
                    )
                    .on_change(cx.listener(|this, index, _, cx| {
                        this.method = if *index == 0 {
                            PeakMethod::Sample
                        } else {
                            PeakMethod::TruePeak
                        };
                        this.accepted = false;
                        cx.notify();
                    })),
            )
            .child(label("reference", "Reference: -18 LUFS".into()).mt_3())
            .child(label("destination", "Save destination: App library".into()))
            .child(
                label("non-destructive", "Files remain unchanged".into())
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                Button::new("scanner-review")
                    .label("Review groups")
                    .disabled(busy || self.selected.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.changed(cx);
                        this.send(Command::Review(this.selected.clone(), this.mode), cx);
                    })),
            )
            .when(
                !self.status.preview.is_empty() && self.showing.is_none(),
                |panel| {
                    panel
                        .child(
                            Button::new("scanner-accept")
                                .label(if self.accepted {
                                    "Groups accepted"
                                } else {
                                    "Accept selected groups"
                                })
                                .selected(self.accepted)
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.accepted = !this.accepted;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("scanner-start")
                                .primary()
                                .label("Scan accepted groups")
                                .disabled(busy || !self.accepted)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.send(
                                        Command::Start(
                                            this.status.preview.as_ref().clone(),
                                            this.mode,
                                            this.method,
                                        ),
                                        cx,
                                    );
                                })),
                        )
                },
            )
            .into_any_element()
    }
    fn selection(&self, cx: &mut Context<Self>) -> AnyElement {
        let busy = self.status.busy;
        v_flex()
            .id("scan-content")
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(label(
                        "selection-count",
                        format!("{} tracks selected", self.selected.len()),
                    ))
                    .child(
                        Button::new("scanner-all")
                            .ghost()
                            .label("Select all")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected =
                                    this.status.candidates.iter().map(|c| c.file).collect();
                                this.changed(cx);
                            })),
                    )
                    .child(
                        Button::new("scanner-clear")
                            .ghost()
                            .label("Clear selection")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected.clear();
                                this.changed(cx);
                            })),
                    )
                    .child(
                        Button::new("scanner-refresh")
                            .ghost()
                            .label("Refresh library")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.changed(cx);
                                this.send(Command::Refresh, cx);
                            })),
                    ),
            )
            .when(self.status.candidates.is_empty(), |panel| {
                panel.child(label(
                    "empty-selection",
                    "No local tracks. Add a folder in Sources & output.".into(),
                ))
            })
            .child(
                uniform_list(
                    "scanner-tracks",
                    self.status.candidates.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _: &mut Window, cx| {
                        range
                            .map(|i| {
                                let item = this.status.candidates[i].clone();
                                let file = item.file;
                                let album = item.album;
                                h_flex()
                                    .id(SharedString::from(format!("scan-track-row-{file}")))
                                    .w_full()
                                    .gap_2()
                                    .py_2()
                                    .items_center()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(
                                                label(
                                                    "track-title",
                                                    format!(
                                                        "{}.{} {}{}",
                                                        item.disc,
                                                        item.track,
                                                        item.title,
                                                        if item.available {
                                                            ""
                                                        } else {
                                                            " · Missing"
                                                        }
                                                    ),
                                                )
                                                .truncate(),
                                            )
                                            .child(
                                                label(
                                                    "track-album",
                                                    format!(
                                                        "{} · {} · {}",
                                                        item.album_artist,
                                                        item.album_title,
                                                        item.edition
                                                    ),
                                                )
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .truncate(),
                                            ),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("scan-select-{i}")))
                                            .label(if this.selected.contains(&file) {
                                                "Selected"
                                            } else {
                                                "Select track"
                                            })
                                            .accessibility_label(format!(
                                                "Select track {}",
                                                item.title
                                            ))
                                            .selected(this.selected.contains(&file))
                                            .disabled(this.status.busy)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if !this.selected.remove(&file) {
                                                    this.selected.insert(file);
                                                }
                                                this.changed(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("scan-album-{i}")))
                                            .label("Select album")
                                            .accessibility_label(format!(
                                                "Select album {}",
                                                item.album_title
                                            ))
                                            .disabled(this.status.busy)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.selected.extend(
                                                    this.status
                                                        .candidates
                                                        .iter()
                                                        .filter(|c| c.album == album)
                                                        .map(|c| c.file),
                                                );
                                                this.changed(cx);
                                            })),
                                    )
                                    .into_any_element()
                            })
                            .collect()
                    }),
                )
                .h(px(320.)),
            )
            .children(
                self.status
                    .preview
                    .iter()
                    .enumerate()
                    .map(|(index, group)| {
                        v_flex()
                            .id(SharedString::from(format!("scan-preview-group-{index}")))
                            .gap_2()
                            .py_3()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                label(
                                    "review-title",
                                    format!("{} · {} tracks", group.name, group.members.len()),
                                )
                                .font_semibold(),
                            )
                            .when(!group.warning.is_empty(), |row| {
                                row.child(
                                    label("review-warning", group.warning.clone())
                                        .text_color(cx.theme().warning),
                                )
                            })
                            .children(group.members.iter().map(|m| {
                                label(
                                    SharedString::from(format!("review-member-{}", m.file)),
                                    m.title.clone(),
                                )
                                .into_any_element()
                            }))
                    }),
            )
            .into_any_element()
    }
    fn results(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(record) = self
            .showing
            .and_then(|id| self.status.history.iter().find(|r| r.id == id))
        else {
            return self.selection(cx);
        };
        let id = record.id;
        v_flex()
            .id("scan-content")
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        label(
                            "result-state",
                            format!(
                                "{}{} · {}",
                                record.state.label(),
                                if record.saved {
                                    " · Saved in app library"
                                } else {
                                    " · Preview"
                                },
                                peak_label(record.method)
                            ),
                        )
                        .font_semibold(),
                    )
                    .child(
                        Button::new("scanner-new")
                            .label("New selection")
                            .disabled(self.status.busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.changed(cx);
                            })),
                    )
                    .child(
                        Button::new("scanner-resume")
                            .label("Review and resume")
                            .disabled(self.status.busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(record) =
                                    this.status.history.iter().find(|r| r.id == id)
                                {
                                    this.selected = record
                                        .groups
                                        .iter()
                                        .flat_map(|g| g.members.iter().map(|m| m.file))
                                        .collect();
                                    this.mode = record.mode;
                                    this.method = record.method;
                                }
                                this.changed(cx);
                                this.send(Command::Review(this.selected.clone(), this.mode), cx);
                            })),
                    )
                    .child(
                        Button::new("scanner-save")
                            .primary()
                            .label("Save to app library")
                            .disabled(
                                self.status.busy
                                    || record.state != ScanState::Ready
                                    || record.saved,
                            )
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.send(Command::Save(id), cx)),
                            ),
                    ),
            )
            .child(
                label(
                    "result-provenance",
                    format!(
                        "{} · target {:.1} LUFS",
                        record.analyzer, record.target_lufs
                    ),
                )
                .text_xs()
                .text_color(cx.theme().muted_foreground),
            )
            .when_some(record.error.clone(), |panel, error| {
                panel.child(label("result-error", error).text_color(cx.theme().warning))
            })
            .children(record.groups.iter().enumerate().map(|(index, group)| {
                let complete = group.members.iter().all(|m| m.done);
                v_flex()
                    .id(SharedString::from(format!("scan-result-group-{index}")))
                    .gap_2()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        label(
                            "group-state",
                            format!(
                                "{} · {}",
                                group.reviewed.name,
                                if group.stale {
                                    "Stale"
                                } else if !complete {
                                    "Incomplete"
                                } else {
                                    "Ready"
                                }
                            ),
                        )
                        .font_semibold(),
                    )
                    .when(!group.reviewed.warning.is_empty(), |panel| {
                        panel.child(
                            label(
                                "accepted-warning",
                                format!("Accepted group: {}", group.reviewed.warning),
                            )
                            .text_color(cx.theme().warning),
                        )
                    })
                    .when(record.mode != ScanMode::Tracks, |panel| {
                        panel.child(label(
                            "album-result",
                            if group.stale || !complete {
                                "Album result unavailable".into()
                            } else {
                                format!(
                                    "Album gain {} · Peak {}",
                                    db(group.gain_db),
                                    peak(group.peak)
                                )
                            },
                        ))
                    })
                    .children(group.members.iter().map(|member| {
                        let text = if let Some(m) = &member.measurement {
                            format!(
                                "{} · Gain {} · Peak {}{}{}",
                                member.title,
                                db(m.gain_db()),
                                peak(Some(m.peak)),
                                if member.reused { " · Reused" } else { "" },
                                if m.loudness_lufs.is_none() {
                                    " · No gated loudness"
                                } else {
                                    ""
                                }
                            )
                        } else {
                            format!(
                                "{} · {}",
                                member.title,
                                member.detail.as_deref().unwrap_or("Pending")
                            )
                        };
                        label(
                            SharedString::from(format!("member-result-{}", member.file)),
                            text,
                        )
                        .into_any_element()
                    }))
            }))
            .into_any_element()
    }
}
impl Render for Scanner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < px(1100.);
        v_flex()
            .id("replaygain-scanner")
            .test_support()
            .gap_5()
            .w_full()
            .when(self.service.is_none(), |panel| {
                panel.child(label(
                    "no-storage",
                    "App library unavailable. Resolve the database error to scan.".into(),
                ))
            })
            .when_some(self.status.error.clone(), |panel, error| {
                panel.child(
                    div()
                        .id("scanner-error")
                        .role(Role::Alert)
                        .child(label("error-message", error))
                        .text_color(cx.theme().warning),
                )
            })
            .when(self.status.busy, |panel| {
                panel.child(
                    h_flex()
                        .items_center()
                        .gap_3()
                        .child(
                            label(
                                "progress",
                                format!(
                                    "{} · {}/{} tracks · {}/{} groups · {} reused · {} · {}{}",
                                    if self.status.waiting_for_playback {
                                        "Waiting for playback"
                                    } else {
                                        &self.status.operation
                                    },
                                    self.status.done,
                                    self.status.total,
                                    self.status.groups_done,
                                    self.status.groups_total,
                                    self.status.reused,
                                    self.status.current,
                                    self.status.frames,
                                    self.status
                                        .total_frames
                                        .map(|n| format!("/{n} frames"))
                                        .unwrap_or_default()
                                ),
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .when(self.status.cancellable, |row| {
                            row.child(Button::new("scanner-cancel").label("Cancel scan").on_click(
                                cx.listener(|this, _, _, cx| {
                                    if let Some(s) = &this.service {
                                        s.cancel();
                                    }
                                    cx.notify();
                                }),
                            ))
                        }),
                )
            })
            .child(
                h_flex()
                    .when(narrow, |row| row.flex_col())
                    .items_start()
                    .gap_6()
                    .child(self.results(cx))
                    .child(self.settings(cx)),
            )
            .child(
                label("history-heading", "Scan history".into())
                    .text_size(px(22.))
                    .font_semibold(),
            )
            .when(self.status.history.is_empty(), |panel| {
                panel.child(
                    label("history-empty", "No scans yet".into())
                        .text_color(cx.theme().muted_foreground),
                )
            })
            .children(self.status.history.iter().map(|record| {
                let id = record.id;
                Button::new(SharedString::from(format!("scan-history-{id}")))
                    .ghost()
                    .label(format!(
                        "{} · {} · {} · {} groups{}",
                        scan_time(record.created_at_ms),
                        record.state.label(),
                        peak_label(record.method),
                        record.groups.len(),
                        if record.saved { " · Saved" } else { "" }
                    ))
                    .disabled(self.status.busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.showing = Some(id);
                        cx.notify();
                    }))
                    .into_any_element()
            }))
    }
}
fn label(id: impl Into<gpui_kit::ElementId>, text: String) -> gpui_kit::Stateful<gpui_kit::Div> {
    div()
        .id(id)
        .role(Role::Label)
        .aria_value(text.clone())
        .child(text)
}
fn peak_label(method: PeakMethod) -> &'static str {
    match method {
        PeakMethod::Sample => "Sample peak",
        PeakMethod::TruePeak => "True peak",
    }
}
fn db(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:+.2} dB"))
        .unwrap_or_else(|| "Unavailable".into())
}
fn peak(value: Option<f64>) -> String {
    value
        .map(|v| format!("{v:.6}"))
        .unwrap_or_else(|| "Unavailable".into())
}

fn scan_time(timestamp_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(timestamp_ms)
        .map(|date| date.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unknown date".into())
}
