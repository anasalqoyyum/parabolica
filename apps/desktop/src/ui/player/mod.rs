//! Persistent player bar and transport controls.

use std::collections::VecDeque;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, PathPromptOptions, Render, Role, SharedString, Subscription, Task,
    TestSupportExt as _, Window, div, px,
};
use parabolica_audio::decode::SourceInfo;
use parabolica_audio::engine::{EngineEvent, EngineSnapshot};
use parabolica_domain::control::SnapshotGate;
use parabolica_domain::format::{Codec, SourceSampleFormat};
use parabolica_domain::gain::Volume;
use parabolica_domain::ids::{MediaFileId, QueueEntryId};
use parabolica_domain::output::OutputBackend;
use parabolica_domain::playback::{PositionStamp, TransportState};

use crate::commands::playback::PlaybackCommands;

pub const PLAYER_HEIGHT: f32 = 104.;
/// How often the bar drains engine events and redraws the position while playing.
const FRAME: Duration = Duration::from_millis(33);

pub struct Player {
    commands: PlaybackCommands,
    events: Receiver<EngineEvent>,
    gate: SnapshotGate,
    snapshot: Option<EngineSnapshot>,
    position: Option<PositionStamp>,
    seek: Entity<SliderState>,
    volume: Entity<SliderState>,
    /// Where the user is dragging the seek slider, as a fraction of the duration.
    dragging: Option<f32>,
    album_remaining: VecDeque<(MediaFileId, PathBuf)>,
    album_entry: Option<QueueEntryId>,
    _subscriptions: [Subscription; 2],
    _poll: Task<()>,
}

impl Player {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (commands, events) = PlaybackCommands::start();
        Self::with_commands(commands, events, cx)
    }

    pub fn with_commands(
        commands: PlaybackCommands,
        events: Receiver<EngineEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let seek = cx.new(|_| SliderState::new().min(0.).max(1.).step(0.001));
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(1.)
                .step(0.01)
                .default_value(1.)
        });
        let subscriptions = [
            cx.subscribe(&seek, |this, _, event: &SliderEvent, cx| match event {
                SliderEvent::Change(value) => {
                    this.dragging = Some(value.end());
                    cx.notify();
                }
                SliderEvent::Release(value) => {
                    this.dragging = None;
                    this.seek_to(value.end());
                    cx.notify();
                }
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, _| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                if let Some(volume) = Volume::new(value.end().clamp(0., 1.)) {
                    this.commands.set_volume(volume);
                }
            }),
        ];
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                if this.update(cx, |this, cx| this.drain(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            commands,
            events,
            gate: SnapshotGate::default(),
            snapshot: None,
            position: None,
            seek,
            volume,
            dragging: None,
            album_remaining: VecDeque::new(),
            album_entry: None,
            _subscriptions: subscriptions,
            _poll: poll,
        }
    }

    /// Apply every pending engine event in one update.
    fn drain(&mut self, cx: &mut Context<Self>) {
        let events: Vec<_> = self.events.try_iter().collect();
        let changed = !events.is_empty();
        for event in parabolica_domain::dispatch::coalesce(events) {
            match event {
                EngineEvent::Snapshot(snapshot) => {
                    if let Some(snapshot) = self.gate.accept(*snapshot) {
                        let ended = snapshot.playback.current == self.album_entry
                            && self.album_entry.is_some()
                            && snapshot.playback.transport == TransportState::Stopped
                            && snapshot.source.is_some()
                            && snapshot.failure.is_none();
                        self.position = snapshot.playback.position;
                        self.snapshot = Some(snapshot);
                        if ended {
                            self.play_next_indexed();
                        }
                    }
                }
                EngineEvent::Position(position) => self.position = Some(position),
                EngineEvent::Underruns { .. } | EngineEvent::OutputLost { .. } => {}
            }
        }
        if changed || self.transport() == TransportState::Playing {
            cx.notify();
        }
    }

    fn transport(&self) -> TransportState {
        self.snapshot
            .as_ref()
            .map_or(TransportState::Stopped, |snapshot| {
                snapshot.playback.transport
            })
    }

    #[cfg(test)]
    pub(crate) fn indexed_entry_for_test(&self) -> Option<QueueEntryId> {
        self.album_entry
    }

    #[cfg(test)]
    pub(crate) fn remaining_indexed_for_test(&self) -> usize {
        self.album_remaining.len()
    }

    pub fn is_playing(&self) -> bool {
        matches!(
            self.transport(),
            TransportState::Playing | TransportState::Loading
        )
    }

    pub fn play_indexed(&mut self, files: Vec<(MediaFileId, PathBuf)>, cx: &mut Context<Self>) {
        self.album_remaining = files.into();
        self.play_next_indexed();
        cx.notify();
    }

    fn play_next_indexed(&mut self) {
        self.album_entry = self
            .album_remaining
            .pop_front()
            .map(|(file, path)| self.commands.play_indexed(file, path));
    }

    fn duration(&self) -> Option<Duration> {
        self.snapshot.as_ref()?.playback.duration
    }

    fn elapsed(&self) -> Duration {
        let position = self
            .position
            .map_or(Duration::ZERO, |stamp| stamp.position_at(Instant::now()));
        match self.duration() {
            Some(duration) => position.min(duration),
            None => position,
        }
    }

    fn seek_to(&self, fraction: f32) {
        let (Some(snapshot), Some(duration)) = (&self.snapshot, self.duration()) else {
            return;
        };
        if let Some(entry) = snapshot.playback.current {
            self.commands
                .seek(entry, duration.mul_f64(f64::from(fraction.clamp(0., 1.))));
        }
    }

    fn toggle(&mut self, _: &gpui_kit::ClickEvent, _: &mut Window, _: &mut Context<Self>) {
        match self.transport() {
            TransportState::Playing | TransportState::Loading => self.commands.pause(),
            _ => self.commands.resume(),
        }
    }

    fn open_file(&mut self, _: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.album_remaining.clear();
                this.album_entry = None;
                this.commands.open_file(path);
                cx.notify();
            });
        })
        .detach();
    }

    fn render_now_playing(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let snapshot = self.snapshot.as_ref();
        let source = snapshot.and_then(|snapshot| snapshot.source.as_ref());
        let path = snapshot.and_then(|snapshot| snapshot.path.as_deref());
        let failure = snapshot.and_then(|snapshot| snapshot.failure.as_ref());

        let title: SharedString = match (source, path) {
            (Some(SourceInfo { tags, .. }), Some(path)) => {
                tags.title.clone().unwrap_or_else(|| file_name(path)).into()
            }
            (None, Some(path)) => file_name(path).into(),
            _ => "Nothing playing".into(),
        };
        let artist = source.and_then(|source| source.tags.artist.clone());
        let format = source.map(format_label);

        h_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(
                div()
                    .flex_shrink_0()
                    .size(px(56.))
                    .rounded(px(8.))
                    .bg(theme.border)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(IconName::Disc3)),
            )
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .id("now-playing")
                            .test_support()
                            .role(Role::Label)
                            .aria_value(title.clone())
                            .truncate()
                            .when(path.is_none(), |this| {
                                this.text_color(theme.muted_foreground)
                            })
                            .when(path.is_some(), |this| this.font_semibold())
                            .child(title),
                    )
                    .when_some(artist, |this, artist| {
                        this.child(
                            div()
                                .id("now-playing-artist")
                                .role(Role::Label)
                                .aria_value(artist.clone())
                                .truncate()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .child(artist),
                        )
                    })
                    .when_some(format, |this, format| {
                        this.child(
                            div()
                                .id("now-playing-format")
                                .role(Role::Label)
                                .aria_value(format.clone())
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format),
                        )
                    })
                    .when_some(failure, |this, failure| {
                        let file = failure.path.as_deref().map(file_name);
                        let text: SharedString = match file {
                            Some(file) => format!("Can't play {file}: {}", failure.error),
                            None => format!("Can't play: {}", failure.error),
                        }
                        .into();
                        this.child(
                            h_flex()
                                .id("player-error")
                                .test_support()
                                .role(Role::Alert)
                                .aria_label(text.clone())
                                .gap_1()
                                .text_xs()
                                .text_color(theme.danger)
                                .child(Icon::new(IconName::TriangleAlert).xsmall())
                                .child(div().truncate().child(text)),
                        )
                    }),
            )
            .child(
                Button::new("open-file")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .accessibility_label("Open file")
                    .tooltip("Open file")
                    .on_click(cx.listener(Self::open_file)),
            )
    }

    fn render_transport(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let transport = self.transport();
        let has_entry = self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.playback.current.is_some());
        let duration = self.duration();
        let elapsed = self.elapsed();
        let fraction = match (self.dragging, duration) {
            (Some(fraction), _) => fraction,
            (None, Some(duration)) if !duration.is_zero() => {
                (elapsed.as_secs_f64() / duration.as_secs_f64()) as f32
            }
            _ => 0.,
        };
        if self.dragging.is_none() && self.seek.read(cx).value().end() != fraction {
            self.seek
                .update(cx, |seek, cx| seek.set_value(fraction, window, cx));
        }
        let shown = match (self.dragging, duration) {
            (Some(fraction), Some(duration)) => duration.mul_f64(f64::from(fraction)),
            _ => elapsed,
        };
        let playing = matches!(transport, TransportState::Playing | TransportState::Loading);
        let (play_icon, play_label) = if playing {
            (IconName::Pause, "Pause")
        } else {
            (IconName::Play, "Play")
        };
        let theme = cx.theme();
        let queue_controls = |id: &'static str, label: &'static str, icon: IconName| {
            Button::new(id)
                .ghost()
                .icon(icon)
                .accessibility_label(label)
                .disabled(true)
        };

        // Shares the width equally with the side sections, up to 420 px, and keeps room for its
        // buttons at the minimum window width.
        v_flex()
            .id("player-transport")
            .test_support()
            .flex_1()
            .max_w(px(420.))
            .min_w(px(220.))
            .gap_1()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .child(queue_controls(
                        "player-shuffle",
                        "Shuffle",
                        IconName::Shuffle,
                    ))
                    .child(queue_controls(
                        "player-previous",
                        "Previous",
                        IconName::SkipBack,
                    ))
                    .child(
                        Button::new("player-play")
                            .primary()
                            .large()
                            .rounded_full()
                            .icon(play_icon)
                            .accessibility_label(play_label)
                            .tooltip(play_label)
                            .disabled(!has_entry)
                            .on_click(cx.listener(Self::toggle)),
                    )
                    .child(queue_controls("player-next", "Next", IconName::SkipForward))
                    .child(queue_controls("player-repeat", "Repeat", IconName::Repeat)),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(
                        div()
                            .id("player-elapsed")
                            .role(Role::Label)
                            .aria_value(clock(shown))
                            .w(px(44.))
                            .text_right()
                            .child(clock(shown)),
                    )
                    .child(
                        Slider::new(&self.seek)
                            .accessibility_label("Seek")
                            .disabled(duration.is_none())
                            .flex_1(),
                    )
                    .child(
                        div()
                            .id("player-duration")
                            .role(Role::Label)
                            .aria_value(duration.map(clock).unwrap_or_default())
                            .w(px(44.))
                            .child(duration.map(clock).unwrap_or_default()),
                    ),
            )
    }

    fn render_output(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let output = self.snapshot.as_ref().and_then(|snapshot| {
            let output = snapshot.output.as_ref()?;
            let backend = match output.device.backend {
                OutputBackend::WasapiShared => "Shared",
                OutputBackend::WasapiExclusive => "Exclusive",
                OutputBackend::Asio => "ASIO",
                _ => "Output",
            };
            let mut summary = format!("{backend} · {}", khz(output.format.rate.hz()));
            if snapshot
                .playback
                .signal_path
                .as_ref()
                .is_some_and(|path| path.app_changes_samples())
            {
                summary.push_str(" · Processing");
            }
            Some((summary, output.device_name.clone()))
        });

        h_flex()
            .flex_1()
            .min_w_0()
            .justify_end()
            .gap_3()
            .when_some(output, |this, (summary, device)| {
                this.child(
                    v_flex()
                        .id("output-summary")
                        .test_support()
                        .role(Role::Label)
                        .aria_value(format!("{summary}, {device}"))
                        .min_w_0()
                        .items_end()
                        .child(div().truncate().text_sm().child(summary))
                        .child(
                            div()
                                .truncate()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(device),
                        ),
                )
            })
            .child(
                Icon::new(IconName::Volume2)
                    .text_color(theme.muted_foreground)
                    .flex_shrink_0(),
            )
            .child(
                Slider::new(&self.volume)
                    .accessibility_label("Volume")
                    .w(px(96.))
                    .min_w(px(96.))
                    .flex_shrink_0(),
            )
    }
}

impl Render for Player {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let transport = self.render_transport(window, cx).into_any_element();
        let theme = cx.theme();
        let (border, background) = (theme.border, theme.sidebar);
        h_flex()
            .id("player")
            .role(Role::Region)
            .aria_label("Player")
            .flex_shrink_0()
            .h(px(PLAYER_HEIGHT))
            .px_6()
            .gap_6()
            .border_t_1()
            .border_color(border)
            .bg(background)
            .child(self.render_now_playing(cx))
            .child(transport)
            .child(self.render_output(cx))
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn khz(hz: u32) -> String {
    if hz.is_multiple_of(1000) {
        format!("{} kHz", hz / 1000)
    } else {
        format!("{:.1} kHz", f64::from(hz) / 1000.)
    }
}

/// "FLAC · 24-bit · 96 kHz", or without the bit depth for lossy codecs.
fn format_label(source: &SourceInfo) -> SharedString {
    let codec = match &source.format.codec {
        Codec::Flac => "FLAC",
        Codec::Alac => "ALAC",
        Codec::Wav => "WAV",
        Codec::Mp3 => "MP3",
        Codec::Aac => "AAC",
        Codec::Vorbis => "Vorbis",
        Codec::Opus => "Opus",
        Codec::Other(name) => name.as_str(),
    };
    let rate = khz(source.format.rate.hz());
    match source.format.sample {
        Some(SourceSampleFormat::Int { bits }) => format!("{codec} · {bits}-bit · {rate}"),
        Some(SourceSampleFormat::Float { bits }) => {
            format!("{codec} · {bits}-bit float · {rate}")
        }
        None => format!("{codec} · {rate}"),
    }
    .into()
}

fn clock(duration: Duration) -> SharedString {
    let seconds = duration.as_secs();
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}").into()
    } else {
        format!("{minutes}:{seconds:02}").into()
    }
}

#[cfg(test)]
mod tests;
