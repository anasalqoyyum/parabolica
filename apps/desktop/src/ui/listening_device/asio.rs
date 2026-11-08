//! Driver identities, queried capabilities and explicit ASIO requests.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, Role, SharedString};
use parabolica_domain::output::{OutputBackend, OutputDeviceId, OutputSelection, SampleRatePolicy};

use super::{heading, rate_choice, sample_label, value, warning};
use crate::controllers::queue::{QueueService, QueueSnapshot};
use crate::ui::shell::NavItem;

pub(super) fn controls(
    state: &QueueSnapshot,
    selection: &OutputSelection,
    queue: &QueueService,
    cx: &App,
) -> impl IntoElement {
    let selected = selection.device.as_ref().and_then(|id| {
        state.asio_drivers.iter().find(|driver| {
            driver.compatible && driver.stable_id.eq_ignore_ascii_case(&id.stable_id)
        })
    });
    let refresh = queue.clone();
    let mut view = v_flex().gap_6()
        .child(v_flex().gap_2()
            .child(h_flex().flex_wrap().gap_3().justify_between().child(heading("ASIO driver"))
                .child(Button::new("refresh-asio").label("Refresh drivers").on_click(move |_, _, _| refresh.refresh_outputs())))
            .child(v_flex().id("asio-drivers").role(Role::List).aria_label("ASIO drivers")
                .children(state.asio_drivers.iter().enumerate().map(|(index, driver)| {
                    let mut next = selection.clone();
                    next.device = Some(OutputDeviceId { backend: OutputBackend::Asio, stable_id: driver.stable_id.clone() });
                    let queue = queue.clone();
                    let chosen = selected.is_some_and(|selected| selected.stable_id == driver.stable_id) && driver.compatible;
                    let compatible = driver.compatible;
                    NavItem::new(format!("asio-driver-{index}"), format!("{} · {}{}", driver.name, driver.architecture, if compatible { "" } else { " · Unavailable" }), chosen,
                        move |_, _, _| { if compatible { queue.select_output(next.clone()); } })
                })))
            .when(state.asio_drivers.is_empty(), |view| view.child(value("No installed ASIO drivers. This app requires a driver matching its architecture.")))
            .children(state.asio_drivers.iter().filter_map(|driver| driver.unavailable_reason.as_ref().map(|reason| value(format!("{}: {reason}", driver.name)))))
            .when(selection.device.is_some() && selected.is_none(), |view| view.child(warning("asio-driver-missing", format!("Selected ASIO driver unavailable: {}. Reconnect the device and refresh, or select a compatible driver.", selection.device.as_ref().expect("selected").stable_id), cx))))
        .when_some(state.asio_refresh_error.clone(), |view, error| view.child(warning("asio-enumeration-error", error, cx)))
        .when_some(state.asio_error.clone(), |view, error| view.child(warning("asio-capability-error", error, cx)));
    if let Some(caps) = &state.asio_capabilities {
        view = view.child(
            v_flex()
                .gap_2()
                .child(heading("Sample rate"))
                .child(value(format!(
                    "{} · Driver version {}",
                    caps.driver_name, caps.driver_version
                )))
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .id("asio-rate-policy")
                        .role(Role::RadioGroup)
                        .aria_label("ASIO sample rate policy")
                        .child(rate_choice(
                            "asio-rate-source",
                            "Follow source".to_owned(),
                            SampleRatePolicy::FollowSource,
                            selection,
                            queue,
                        ))
                        .children(caps.rates.iter().map(|rate| {
                            rate_choice(
                                &format!("asio-rate-{}", rate.hz()),
                                format!("Fixed {} Hz", rate.hz()),
                                SampleRatePolicy::Fixed(*rate),
                                selection,
                                queue,
                            )
                        })),
                )
                .child(value(
                    "Rates queried individually. Unsupported requests pause playback.",
                )),
        );
        let mut channels = v_flex().gap_2().child(heading("Output channels"));
        for side in 0..2 {
            channels = channels.child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .id(SharedString::from(format!("asio-output-side-{side}")))
                    .role(Role::RadioGroup)
                    .aria_label(if side == 0 {
                        "Left output"
                    } else {
                        "Right output"
                    })
                    .child(value(if side == 0 { "Left" } else { "Right" }))
                    .children(caps.outputs.iter().map(|channel| {
                        let mut next = selection.clone();
                        next.asio.channels.resize(2, 0);
                        if next.asio.channels[1 - side] == channel.index {
                            next.asio.channels[1 - side] = next.asio.channels[side];
                        }
                        next.asio.channels[side] = channel.index;
                        let queue = queue.clone();
                        Radio::new(SharedString::from(format!(
                            "asio-channel-{side}-{}",
                            channel.index
                        )))
                        .label(format!("{} · {}", channel.index + 1, channel.name))
                        .checked(selection.asio.channels.get(side) == Some(&channel.index))
                        .on_change(move |_, _, _| queue.select_output(next.clone()))
                    })),
            );
        }
        view = view.child(channels.children(caps.outputs.iter().map(|channel| {
            value(format!(
                "Output {}: {} · ASIO sample type {}",
                channel.index + 1,
                channel
                    .sample
                    .map(sample_label)
                    .unwrap_or_else(|| "Unsupported PCM representation".to_owned()),
                channel.sample_type
            ))
        })));
        let buffer = caps.buffer;
        let mut sizes = vec![
            32,
            64,
            128,
            256,
            512,
            1024,
            2048,
            4096,
            8192,
            16384,
            buffer.preferred,
        ];
        sizes.sort_unstable();
        sizes.dedup();
        let panel = queue.clone();
        view = view.child(v_flex().gap_2().child(heading("Driver buffer"))
            .child(value(format!("Preferred {} frames · Allowed {}–{} · Granularity {}", buffer.preferred, buffer.min, buffer.max, buffer.granularity)))
            .child(h_flex().flex_wrap().gap_2().id("asio-buffer-request").role(Role::RadioGroup).aria_label("ASIO buffer request")
                .child(buffer_choice("asio-buffer-driver", "Driver preferred".to_owned(), None, selection, queue))
                .when(buffer.min != buffer.max && buffer.granularity != 0, |view| view.children(sizes.into_iter().filter(|frames| buffer.supports(*frames)).map(|frames|
                    buffer_choice(&format!("asio-buffer-{frames}"), format!("{frames} frames"), Some(frames), selection, queue)))))
            .child(value("Driver-managed. Requests apply when opening the stream; the actual size appears in Current stream."))
            .child(Button::new("asio-panel").ghost().label("Open driver panel").disabled(!cfg!(all(windows, feature = "asio")))
                .on_click(move |_, _, _| panel.driver_panel())));
    }
    view
}

fn buffer_choice(
    id: &str,
    label: String,
    frames: Option<u32>,
    selection: &OutputSelection,
    queue: &QueueService,
) -> Radio {
    let mut next = selection.clone();
    next.asio.buffer_frames = frames;
    let queue = queue.clone();
    Radio::new(SharedString::from(id.to_owned()))
        .label(label)
        .checked(selection.asio.buffer_frames == frames)
        .on_change(move |_, _, _| queue.select_output(next.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::playback::PlaybackCommands;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        Bounds, Context, Point, Render, TestAppContext, Window, WindowBounds, WindowOptions, px,
        size,
    };
    use parabolica_audio::engine::{NoOutput, OutputError};

    struct MissingDriverPage {
        queue: QueueService,
        state: QueueSnapshot,
    }
    impl Render for MissingDriverPage {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .child(warning(
                    "output-failure",
                    OutputError::NoDevice.to_string(),
                    cx,
                ))
                .child(controls(
                    &self.state,
                    &self.state.output_selection,
                    &self.queue,
                    cx,
                ))
        }
    }

    #[gpui_kit::test]
    fn missing_driver_keeps_playback_and_capability_errors_independently_accessible(
        cx: &mut TestAppContext,
    ) {
        let (window, _page) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: Point::default(),
                        size: size(px(1280.), px(900.)),
                    })),
                    ..Default::default()
                },
                cx,
                |_, cx| {
                    cx.new(|_| {
                        let (commands, _) = PlaybackCommands::with_output(Box::new(NoOutput));
                        MissingDriverPage {
                            queue: commands.queue().clone(),
                            state: QueueSnapshot {
                                output_selection: OutputSelection {
                                    backend: OutputBackend::Asio,
                                    device: Some(OutputDeviceId {
                                        backend: OutputBackend::Asio,
                                        stable_id: "{missing}".to_owned(),
                                    }),
                                    ..Default::default()
                                },
                                asio_error: Some(OutputError::NoDevice.to_string()),
                                ..Default::default()
                            },
                        }
                    })
                },
            )
            .unwrap()
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for id in ["output-failure", "asio-capability-error"] {
                assert_eq!(
                    window.find(id).label(),
                    Some("no audio output device is available")
                );
            }
            assert!(
                window
                    .find("asio-driver-missing")
                    .label()
                    .unwrap()
                    .contains("{missing}")
            );
        })
        .unwrap();
    }
}
