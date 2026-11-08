//! Listening device controls and the stream the native backend actually opened.

pub mod asio;
pub mod preferences;

use gpui_kit::component::button::Button;
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, Render, Role, Task, TestSupportExt as _, Window, div, px};
use parabolica_domain::format::{OutputSampleFormat, SampleRate};
use parabolica_domain::output::{OutputBackend, OutputDeviceId, OutputSelection, SampleRatePolicy};
use std::time::Duration;

use crate::controllers::queue::QueueService;
use crate::ui::player::Player;
use crate::ui::shell::NavItem;

pub struct ListeningDevice {
    queue: QueueService,
    player: Entity<Player>,
    _poll: Task<()>,
}

impl ListeningDevice {
    pub fn new(player: Entity<Player>, cx: &mut Context<Self>) -> Self {
        let queue = player.read(cx).queue();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        Self {
            queue,
            player,
            _poll: poll,
        }
    }

    fn select_backend(&self, backend: OutputBackend) {
        let mut selection = self.queue.snapshot().output_selection.clone();
        selection.backend = backend;
        if let Some(device) = &mut selection.device {
            if matches!(
                device.backend,
                OutputBackend::WasapiShared | OutputBackend::WasapiExclusive
            ) && matches!(
                backend,
                OutputBackend::WasapiShared | OutputBackend::WasapiExclusive
            ) {
                device.backend = backend;
            } else if device.backend != backend {
                selection.device = None;
            }
        }
        self.queue.select_output(selection);
    }
}

impl Render for ListeningDevice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.queue.snapshot();
        let selection = state.output_selection.clone();
        let stream = self.player.read(cx).output_snapshot().cloned();
        let output = stream
            .as_ref()
            .and_then(|snapshot| snapshot.output.as_ref());
        let failure = stream
            .as_ref()
            .and_then(|snapshot| snapshot.failure.as_ref());
        let wasapi = matches!(
            selection.backend,
            OutputBackend::WasapiShared | OutputBackend::WasapiExclusive
        );
        let selected = selection.device.as_ref().filter(|_| wasapi).and_then(|id| {
            state
                .endpoints
                .iter()
                .find(|endpoint| endpoint.stable_id == id.stable_id)
        });
        let missing = wasapi
            && selection.device.is_some()
            && selected.is_none_or(|endpoint| !endpoint.available);
        let exclusive = selection.backend == OutputBackend::WasapiExclusive;
        let queue = self.queue.clone();
        let mut page = v_flex().id("listening-device").gap_6().min_w_0()
            .when_some(failure, |view, failure| {
                let queue = self.queue.clone();
                view.child(warning("output-failure", failure.error.to_string(), cx)).child(
                    Button::new("retry-output")
                        .label("Retry selected output")
                        .on_click(move |_, _, _| {
                            queue.resume();
                        }),
                )
            })

            .child(v_flex().gap_2().child(heading("Output backend"))
                .child(v_flex().id("output-backends").role(Role::List).aria_label("Output backend")
                    .children([(OutputBackend::WasapiShared, "WASAPI Shared", "output-shared"), (OutputBackend::WasapiExclusive, "WASAPI Exclusive", "output-exclusive")].map(|(backend, label, id)|
                        NavItem::new(id, label, selection.backend == backend, cx.listener(move |this, _, _, _| this.select_backend(backend)))))
                    .child(NavItem::new("output-asio", if cfg!(all(windows, feature = "asio")) { "ASIO" } else { "ASIO unavailable in this build" }, selection.backend == OutputBackend::Asio, cx.listener(|this, _, _, _| this.select_backend(OutputBackend::Asio))))))
            .when(wasapi, |page| page.child(v_flex().gap_2()
                .child(h_flex().flex_wrap().gap_3().justify_between().child(heading("Endpoint"))
                    .child(Button::new("refresh-endpoints").label("Refresh devices").on_click(move |_, _, _| queue.refresh_outputs())))
                .child(v_flex().id("output-endpoints").role(Role::List).aria_label("Endpoints")
                    .children(state.endpoints.iter().filter(|endpoint| wasapi && endpoint.available).enumerate().map(|(index, endpoint)| {
                        let id = endpoint.stable_id.clone();
                        let chosen = selection.device.as_ref().is_some_and(|device| device.stable_id == id);
                        let label = format!("{}{}", endpoint.name, if endpoint.is_default { " · Windows default" } else { "" });
                        let queue = self.queue.clone();
                        let mut next = selection.clone();
                        next.device = Some(OutputDeviceId { backend: next.backend, stable_id: id });
                        NavItem::new(format!("endpoint-{index}"), label, chosen, move |_, _, _| queue.select_output(next.clone()))
                    })))
                .when(wasapi && !state.endpoints.iter().any(|endpoint| endpoint.available), |view| view.child(value("No active audio endpoints")))
                .when(missing, |view| view.child(warning("output-endpoint-missing", format!("Selected endpoint unavailable: {}. Reconnect it and refresh, or select another endpoint.", selected.map_or_else(|| selection.device.as_ref().expect("selected").stable_id.clone(), |endpoint| endpoint.name.clone())), cx)))))
            .when_some(state.output_settings_error.clone(), |view, error| view.child(warning("output-settings-error", error, cx)))
            .when_some(state.output_refresh_error.clone(), |view, error| view.child(warning("output-refresh-error", error, cx)))
            .when(!state.storage_available, |view| view.child(warning("output-storage-unavailable", "Output choices cannot be saved without the library database.".to_owned(), cx)));

        if selection.backend == OutputBackend::Asio {
            page = page.child(asio::controls(&state, &selection, &self.queue, cx));
        }
        if let Some(endpoint) = selected.filter(|endpoint| endpoint.available) {
            let mix = endpoint
                .mix_format
                .map(|format| {
                    format!(
                        "{} Hz · {} ch · {}",
                        format.rate.hz(),
                        format.channels.get(),
                        sample_label(format.sample)
                    )
                })
                .unwrap_or_else(|| "Unavailable".to_owned());
            page = page.child(v_flex().gap_2().child(heading("Sample rate"))
                .when(!exclusive, |view| view.child(value(format!("Windows mix format: {mix}"))))
                .when(exclusive, |view| view
                    .child(h_flex().flex_wrap().gap_2().id("output-rate-policy").role(Role::RadioGroup).aria_label("Sample rate policy")
                        .child(rate_choice("rate-source", "Follow source".to_owned(), SampleRatePolicy::FollowSource, &selection, &self.queue))
                        .children([44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000].map(|hz|
                            rate_choice(&format!("rate-{hz}"), format!("Fixed {hz} Hz"), SampleRatePolicy::Fixed(SampleRate::new(hz).expect("nonzero")), &selection, &self.queue))))
                    .child(value("Unsupported requests pause playback. Choose a fixed rate or WASAPI Shared.")))
                .child(value(if exclusive {
                    let rates: std::collections::BTreeSet<_> = endpoint.exclusive_formats.iter().map(|format| format.rate.hz()).collect();
                    format!("Queried stereo Exclusive rates: {}", if rates.is_empty() { "None reported".to_owned() } else { rates.iter().map(u32::to_string).collect::<Vec<_>>().join(", ") })
                } else { "Shared uses the endpoint's current mix rate. The app resamples when needed.".to_owned() }))
                .when_some(endpoint.capability_error.clone(), |view, error| view.child(warning("output-endpoint-capabilities", format!("Capabilities unavailable: {error}"), cx))));
        }
        let mut active = v_flex().gap_2().child(heading("Current stream"));
        if let Some(output) = output {
            active = active
                .child(value(format!(
                    "{} · {}",
                    backend_label(output.device.backend),
                    output.device_name
                )))
                .child(value(format!(
                    "{} Hz · {} ch · {}",
                    output.format.rate.hz(),
                    output.format.channels.get(),
                    sample_label(output.format.sample)
                )))
                .child(value(format!(
                    "Buffer: {} frames · {:.2} ms · {}",
                    output.buffer.frames,
                    output.buffer_duration().as_secs_f64() * 1000.,
                    if output.buffer.owner == parabolica_domain::output::BufferOwner::Driver {
                        "Driver-managed"
                    } else {
                        "App-managed"
                    }
                )))
                .child(value(format!(
                    "Reported output latency: {}",
                    output
                        .reported_latency
                        .map(|latency| format!("{:.2} ms", latency.as_secs_f64() * 1000.))
                        .unwrap_or_else(|| "Unavailable".to_owned())
                )))
                .child(value(format!(
                    "Output channels: {}",
                    output
                        .channel_map
                        .iter()
                        .map(|channel| (channel + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )))
                .child(value(format!(
                    "Windows mixer: {}",
                    if output.device.backend.uses_system_mixer() {
                        "Active; downstream processing unverified"
                    } else {
                        "Bypassed; device processing unverified"
                    }
                )))
                .child(value(format!(
                    "Windows session volume: {}",
                    if output.device.backend.uses_system_mixer() {
                        "Active; unverified"
                    } else {
                        "Not in this path"
                    }
                )))
                .child(value(format!(
                    "Resampling: {}",
                    stream
                        .as_ref()
                        .and_then(|snapshot| snapshot.source.as_ref())
                        .map_or("Unavailable", |source| {
                            if source.format.rate == output.format.rate {
                                "Off"
                            } else {
                                "Active"
                            }
                        })
                )))
                .child(value(format!(
                    "Underruns since stream open: {}",
                    stream.as_ref().map_or(0, |snapshot| snapshot.underruns)
                )));
        } else {
            active = active.child(value("No open stream"));
        }
        page.child(active).child(value(
            "Changing output pauses playback. Press Play to open the selected output.",
        ))
    }
}

pub(super) fn rate_choice(
    id: &str,
    label: String,
    rate: SampleRatePolicy,
    selection: &OutputSelection,
    queue: &QueueService,
) -> Radio {
    let mut next = selection.clone();
    next.rate = rate;
    let queue = queue.clone();
    Radio::new(gpui_kit::SharedString::from(id.to_owned()))
        .label(label)
        .checked(selection.rate == rate)
        .p_2()
        .on_change(move |_, _, _| queue.select_output(next.clone()))
}

pub(super) fn heading(text: &'static str) -> impl IntoElement {
    div()
        .id(text)
        .role(Role::Heading)
        .aria_level(2)
        .aria_label(text)
        .text_size(px(22.))
        .font_semibold()
        .child(text)
}
pub(super) fn value(text: impl Into<String>) -> impl IntoElement {
    let text = text.into();
    div()
        .id(gpui_kit::SharedString::from(text.clone()))
        .role(Role::Label)
        .aria_value(text.clone())
        .text_sm()
        .child(text)
}
pub(super) fn warning(id: &'static str, text: String, cx: &App) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .role(Role::Alert)
        .aria_label(text.clone())
        .text_color(cx.theme().warning)
        .child(text)
}
fn backend_label(backend: OutputBackend) -> &'static str {
    match backend {
        OutputBackend::WasapiShared => "WASAPI Shared",
        OutputBackend::WasapiExclusive => "WASAPI Exclusive",
        OutputBackend::Asio => "ASIO",
        _ => "Unavailable",
    }
}
pub(super) fn sample_label(sample: OutputSampleFormat) -> String {
    match sample {
        OutputSampleFormat::Float { bits } => format!("float{bits}"),
        OutputSampleFormat::Int {
            container_bits,
            valid_bits,
        } if container_bits != valid_bits => {
            format!("{valid_bits}-bit PCM in {container_bits}-bit container")
        }
        OutputSampleFormat::Int { valid_bits, .. } => format!("{valid_bits}-bit PCM"),
    }
}
