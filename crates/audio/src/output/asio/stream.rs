#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use parabolica_domain::format::{ChannelCount, OutputFormat, SourceFormat};
use parabolica_domain::output::{
    AsioCapabilities, BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
    OutputSelection, SampleRatePolicy,
};
use parabolica_platform::windows::asio::{BufferInfo, Callbacks, Driver, DriverError};
use windows::Win32::System::SystemInformation::GetTickCount64;

use super::samples::SampleType;
use crate::engine::{OutputError, OutputStream, StreamRenderer};
use crate::realtime::RealtimeRender;

const DRIVER_TIMEOUT: Duration = Duration::from_secs(5);
const STALL_MS: u64 = 2000;
static CALLBACK: AtomicPtr<CallbackState> = AtomicPtr::new(ptr::null_mut());

fn driver_error(error: DriverError) -> OutputError {
    match error {
        DriverError::Busy => OutputError::Busy,
        DriverError::Missing => OutputError::NoDevice,
        error => OutputError::Backend(error.to_string()),
    }
}
fn device(selection: &OutputSelection) -> Result<OutputDeviceId, OutputError> {
    selection
        .device
        .clone()
        .filter(|id| selection.backend == OutputBackend::Asio && id.backend == OutputBackend::Asio)
        .ok_or_else(|| {
            OutputError::UnsupportedFormat(
                "Select an ASIO driver; a WASAPI endpoint cannot identify it".to_owned(),
            )
        })
}

fn with_driver<T: Send + 'static>(
    selection: &OutputSelection,
    operation: impl FnOnce(Driver) -> Result<T, OutputError> + Send + 'static,
) -> Result<T, OutputError> {
    let id = device(selection)?;
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("parabolica-asio-query".to_owned())
        .spawn(move || {
            let result = Driver::load(&id).map_err(driver_error).and_then(operation);
            let _ = sender.send(result);
        })
        .map_err(|error| OutputError::Backend(error.to_string()))?;
    receiver
        .recv_timeout(DRIVER_TIMEOUT)
        .map_err(|_| OutputError::Stalled)?
}

pub(in crate::output) fn inspect(
    selection: &OutputSelection,
) -> Result<AsioCapabilities, OutputError> {
    with_driver(selection, |driver| {
        driver.capabilities().map_err(driver_error)
    })
}

pub(in crate::output) fn driver_panel(selection: &OutputSelection) -> Result<(), OutputError> {
    // The panel can be modal. A timed-out owner retains the driver lease until the panel closes.
    with_driver(selection, |driver| {
        driver.control_panel().map_err(driver_error)
    })
}

pub(in crate::output) fn open(
    selection: &OutputSelection,
    source: &SourceFormat,
) -> Result<Box<dyn OutputStream>, OutputError> {
    let id = device(selection)?;
    let selection = selection.clone();
    let source = source.clone();
    let (ready_sender, ready) = mpsc::channel();
    let (start, start_receiver) = mpsc::channel();
    let (done_sender, done) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let fault = Arc::new(AtomicU32::new(0));
    let failure = Arc::new(OnceLock::new());
    let thread = std::thread::Builder::new()
        .name("parabolica-asio".to_owned())
        .spawn({
            let stop = stop.clone();
            let fault = fault.clone();
            let failure = failure.clone();
            move || {
                let result = run(
                    id,
                    selection,
                    source,
                    &ready_sender,
                    start_receiver,
                    &stop,
                    fault.clone(),
                );
                if let Err(error) = result {
                    let _ = ready_sender.send(Err(error.clone()));
                    let _ = failure.set(error);
                    fault
                        .compare_exchange(0, 8, Ordering::AcqRel, Ordering::Acquire)
                        .ok();
                }
                let _ = done_sender.send(());
            }
        })
        .map_err(|error| OutputError::Backend(error.to_string()))?;
    let mut stream = AsioStream {
        negotiated: None,
        capabilities: None,
        start: Some(start),
        stop,
        fault,
        failure,
        done,
        thread: Some(thread),
    };
    let (negotiated, capabilities) = ready
        .recv_timeout(DRIVER_TIMEOUT)
        .map_err(|_| OutputError::Stalled)??;
    stream.negotiated = Some(negotiated);
    stream.capabilities = Some(capabilities);
    Ok(Box::new(stream))
}

struct AsioStream {
    negotiated: Option<NegotiatedOutput>,
    capabilities: Option<AsioCapabilities>,
    start: Option<mpsc::Sender<StreamRenderer>>,
    stop: Arc<AtomicBool>,
    fault: Arc<AtomicU32>,
    failure: Arc<OnceLock<OutputError>>,
    done: mpsc::Receiver<()>,
    thread: Option<JoinHandle<()>>,
}
impl OutputStream for AsioStream {
    fn negotiated(&self) -> &NegotiatedOutput {
        self.negotiated.as_ref().expect("ready before returning")
    }
    fn asio_capabilities(&self) -> Option<AsioCapabilities> {
        self.capabilities.clone()
    }
    fn start(&mut self, renderer: StreamRenderer) -> Result<(), OutputError> {
        self.start
            .take()
            .ok_or(OutputError::Stalled)?
            .send(renderer)
            .map_err(|_| OutputError::Stalled)
    }
    fn failure(&self) -> Option<OutputError> {
        self.failure
            .get()
            .cloned()
            .or_else(|| fault_error(self.fault.load(Ordering::Acquire)))
    }
}
impl Drop for AsioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.start = None;
        // A stuck third-party driver must not hold the controller indefinitely. Its detached
        // owner retains all callback memory and the process-wide driver lease until it exits.
        if self.done.recv_timeout(DRIVER_TIMEOUT).is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

fn validate(
    selection: &OutputSelection,
    source: &SourceFormat,
    caps: &AsioCapabilities,
) -> Result<Vec<SampleType>, OutputError> {
    let map = &selection.asio.channels;
    if map.is_empty()
        || map.len() > 32
        || usize::from(source.channels.get()) > map.len()
        || map
            .iter()
            .enumerate()
            .any(|(index, channel)| map[..index].contains(channel))
    {
        return Err(OutputError::UnsupportedFormat(
            "Choose distinct ASIO outputs for every source channel".to_owned(),
        ));
    }
    let types: Vec<_> = map
        .iter()
        .map(|channel| {
            caps.outputs
                .iter()
                .find(|output| output.index == *channel)
                .and_then(|output| SampleType::new(output.sample_type))
                .ok_or_else(|| {
                    OutputError::UnsupportedFormat(format!(
                        "ASIO output {} is missing or its PCM sample type is unsupported",
                        channel + 1
                    ))
                })
        })
        .collect::<Result<_, _>>()?;
    if types.iter().any(|sample| sample.format != types[0].format) {
        return Err(OutputError::UnsupportedFormat(
            "Selected ASIO outputs report different sample representations".to_owned(),
        ));
    }
    Ok(types)
}

fn run(
    id: OutputDeviceId,
    selection: OutputSelection,
    source: SourceFormat,
    ready: &mpsc::Sender<Result<(NegotiatedOutput, AsioCapabilities), OutputError>>,
    start: mpsc::Receiver<StreamRenderer>,
    stop: &AtomicBool,
    fault: Arc<AtomicU32>,
) -> Result<(), OutputError> {
    let driver = Driver::load(&id).map_err(driver_error)?;
    let rate = match selection.rate {
        SampleRatePolicy::FollowSource => source.rate,
        SampleRatePolicy::Fixed(rate) => rate,
    };
    if !driver.supports_rate(rate.hz()) {
        return Err(OutputError::UnsupportedFormat(format!(
            "ASIO driver does not support {} Hz; choose a queried fixed rate or another backend",
            rate.hz()
        )));
    }
    if driver.rate().ok() != Some(rate) {
        driver.set_rate(rate).map_err(driver_error)?;
    }
    if driver.rate().map_err(driver_error)? != rate {
        return Err(OutputError::UnsupportedFormat(
            "ASIO driver did not accept the requested rate".to_owned(),
        ));
    }
    let caps = driver.capabilities().map_err(driver_error)?;
    let types = validate(&selection, &source, &caps)?;
    let frames = selection
        .asio
        .buffer_frames
        .unwrap_or(caps.buffer.preferred);
    if !caps.buffer.supports(frames) || frames > 1_048_576 {
        return Err(OutputError::UnsupportedFormat(format!(
            "ASIO driver does not accept a {frames}-frame buffer"
        )));
    }
    let buffers = selection
        .asio
        .channels
        .iter()
        .map(|channel| BufferInfo {
            is_input: 0,
            channel: i32::from(*channel),
            buffers: [ptr::null_mut(); 2],
        })
        .collect();
    let state = Box::new(CallbackState {
        data: UnsafeCell::new(RenderData {
            renderer: None,
            scratch: vec![0.; frames as usize * types.len()],
            buffers,
            types,
            frames: frames as usize,
        }),
        rendering: AtomicBool::new(false),
        armed: AtomicBool::new(false),
        fault,
        last_tick: AtomicU64::new(tick()),
    });
    let state_pointer = ptr::from_ref(state.as_ref()).cast_mut();
    if CALLBACK
        .compare_exchange(
            ptr::null_mut(),
            state_pointer,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return Err(OutputError::Busy);
    }
    let mut callbacks = Callbacks {
        buffer_switch,
        rate_changed,
        message,
        time_info,
    };
    // All paths after publishing the pointer must stop/dispose the driver before clearing it.
    let result = (|| {
        // SAFETY: descriptors and callbacks outlive Driver; rendering is disarmed during creation.
        let data = unsafe { &mut *state.data.get() };
        unsafe { driver.create_buffers(&mut data.buffers, frames, &mut callbacks) }
            .map_err(driver_error)?;
        if data
            .buffers
            .iter()
            .any(|buffer| buffer.buffers.iter().any(|pointer| pointer.is_null()))
        {
            return Err(OutputError::Backend(
                "ASIO driver returned a null output buffer".to_owned(),
            ));
        }
        for half in 0..2 {
            unsafe { data.silence(half) };
        }
        if driver.rate().map_err(driver_error)? != rate {
            return Err(OutputError::DriverReset(
                "rate changed while creating buffers".to_owned(),
            ));
        }
        let negotiated = NegotiatedOutput {
            device: id,
            device_name: caps.driver_name.clone(),
            format: OutputFormat {
                rate,
                channels: ChannelCount::new(data.types.len() as u16).expect("validated"),
                sample: data.types[0].format,
            },
            channel_map: selection.asio.channels,
            buffer: BufferConfig {
                frames,
                owner: BufferOwner::Driver,
            },
            reported_latency: driver
                .latency_frames()
                .map(|frames| Duration::from_secs_f64(f64::from(frames) / f64::from(rate.hz()))),
        };
        if ready.send(Ok((negotiated, caps))).is_err() {
            return Ok(());
        }
        let Ok(renderer) = start.recv() else {
            return Ok(());
        };
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        data.renderer = Some(renderer);
        state.last_tick.store(tick(), Ordering::Release);
        state.armed.store(true, Ordering::Release);
        // SAFETY: callback state, both buffer halves and renderer have been prepared and published.
        unsafe { driver.start() }.map_err(driver_error)?;
        while !stop.load(Ordering::Acquire) && state.fault.load(Ordering::Acquire) == 0 {
            if tick().saturating_sub(state.last_tick.load(Ordering::Acquire)) > STALL_MS {
                state.fault.store(6, Ordering::Release);
                break;
            }
            driver.pump_messages();
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    state.armed.store(false, Ordering::Release);
    drop(driver);
    CALLBACK.store(ptr::null_mut(), Ordering::Release);
    drop(state);
    result
}

struct RenderData {
    renderer: Option<StreamRenderer>,
    scratch: Vec<f64>,
    buffers: Vec<BufferInfo>,
    types: Vec<SampleType>,
    frames: usize,
}
impl RenderData {
    unsafe fn silence(&mut self, index: usize) {
        for (buffer, sample) in self.buffers.iter().zip(&self.types) {
            // SAFETY: ASIO owns a validated buffer of frames * sample width for each channel/half.
            unsafe { ptr::write_bytes(buffer.buffers[index], 0, self.frames * sample.bytes) };
        }
    }
    unsafe fn render(&mut self, index: usize) {
        let channels = self.buffers.len();
        if let Some(renderer) = &mut self.renderer {
            renderer.render(&mut self.scratch, channels);
        }
        for (channel, (buffer, sample_type)) in self.buffers.iter().zip(&self.types).enumerate() {
            // SAFETY: only this buffer callback writes the driver-owned inactive half.
            let destination = unsafe {
                std::slice::from_raw_parts_mut(
                    buffer.buffers[index].cast::<u8>(),
                    self.frames * sample_type.bytes,
                )
            };
            for (frame, sample) in destination.chunks_exact_mut(sample_type.bytes).enumerate() {
                sample_type.write(self.scratch[frame * channels + channel], sample);
            }
        }
    }
}
struct CallbackState {
    data: UnsafeCell<RenderData>,
    rendering: AtomicBool,
    armed: AtomicBool,
    fault: Arc<AtomicU32>,
    last_tick: AtomicU64,
}

fn tick() -> u64 {
    // SAFETY: infallible monotonic Windows tick read, includes suspend time, no handles or buffers.
    unsafe { GetTickCount64() }
}
fn state() -> Option<&'static CallbackState> {
    // SAFETY: owner publishes before createBuffers, then stops/disposes before clearing and freeing.
    unsafe { CALLBACK.load(Ordering::Acquire).as_ref() }
}
extern "C" fn buffer_switch(index: i32, _: i32) {
    let Some(state) = state() else {
        return;
    };
    if !state.armed.load(Ordering::Acquire) {
        return;
    }
    if state
        .rendering
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        state.fault.store(7, Ordering::Release);
        return;
    }
    if !(0..=1).contains(&index) {
        state.fault.store(7, Ordering::Release);
    } else {
        let now = tick();
        let previous = state.last_tick.swap(now, Ordering::AcqRel);
        if now.saturating_sub(previous) > STALL_MS {
            state.fault.store(6, Ordering::Release);
        }
        // SAFETY: the nonblocking gate admits one render callback; owner does not access data after start.
        let data = unsafe { &mut *state.data.get() };
        if state.fault.load(Ordering::Acquire) != 0 {
            unsafe { data.silence(index as usize) };
        } else {
            unsafe { data.render(index as usize) };
        }
    }
    state.rendering.store(false, Ordering::Release);
}
extern "C" fn rate_changed(_: f64) {
    if let Some(state) = state()
        && state.armed.load(Ordering::Acquire)
    {
        state.fault.store(3, Ordering::Release);
    }
}
extern "C" fn message(selector: i32, value: i32, _: *mut c_void, _: *mut f64) -> i32 {
    match selector {
        1 => i32::from(matches!(value, 2..=7)),
        2 => 2,
        3..=6 => {
            if let Some(state) = state() {
                state.fault.store(
                    match selector {
                        3 => 1,
                        4 => 5,
                        5 => 2,
                        _ => 4,
                    },
                    Ordering::Release,
                );
            }
            1
        }
        7 => 1,
        _ => 0,
    }
}
extern "C" fn time_info(time: *mut c_void, index: i32, direct: i32) -> *mut c_void {
    buffer_switch(index, direct);
    time
}
fn fault_error(fault: u32) -> Option<OutputError> {
    Some(match fault {
        0 => return None,
        6 => OutputError::Stalled,
        1 => OutputError::DriverReset("reset".to_owned()),
        2 => OutputError::DriverReset("resynchronization".to_owned()),
        3 => OutputError::DriverReset("sample rate or clock change".to_owned()),
        4 => OutputError::DriverReset("latency change".to_owned()),
        5 => OutputError::DriverReset("buffer size change".to_owned()),
        7 => OutputError::DriverReset("invalid or concurrent callback".to_owned()),
        _ => OutputError::Backend("ASIO stream failed to start or its owner exited".to_owned()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StreamShared;
    use crate::realtime::probe::allocations_during;
    use parabolica_domain::format::{Codec, SampleRate};
    use parabolica_domain::output::{AsioBufferSizes, AsioChannel};

    fn capabilities() -> AsioCapabilities {
        AsioCapabilities {
            device: OutputDeviceId {
                backend: OutputBackend::Asio,
                stable_id: "{fixture}".to_owned(),
            },
            driver_name: "Fixture".to_owned(),
            driver_version: 1,
            outputs: (0..4)
                .map(|index| AsioChannel {
                    index,
                    name: format!("Output {index}"),
                    sample_type: 27,
                    sample: SampleType::new(27).map(|sample| sample.format),
                })
                .collect(),
            rates: vec![SampleRate::new(48000).unwrap()],
            current_rate: SampleRate::new(48000),
            buffer: AsioBufferSizes {
                min: 32,
                max: 1024,
                preferred: 128,
                granularity: -1,
            },
        }
    }
    #[test]
    fn mapping_and_driver_buffer_constraints_fail_before_start() {
        let source = SourceFormat {
            codec: Codec::Wav,
            sample: None,
            rate: SampleRate::new(48000).unwrap(),
            channels: ChannelCount::STEREO,
        };
        let mut selection = OutputSelection {
            backend: OutputBackend::Asio,
            ..Default::default()
        };
        let caps = capabilities();
        selection.asio.channels = vec![3, 1];
        assert_eq!(validate(&selection, &source, &caps).unwrap().len(), 2);
        for map in [vec![], vec![0], vec![0, 0], vec![0, 4]] {
            selection.asio.channels = map;
            assert!(validate(&selection, &source, &caps).is_err());
        }
        assert!(caps.buffer.supports(256));
        assert!(!caps.buffer.supports(255));
        assert!(!caps.buffer.supports(2048));
        let fixed = AsioBufferSizes {
            min: 256,
            max: 256,
            preferred: 256,
            granularity: 0,
        };
        assert!(fixed.supports(256));
        assert!(!fixed.supports(128));
        let stepped = AsioBufferSizes {
            min: 48,
            max: 480,
            preferred: 96,
            granularity: 48,
        };
        assert!(stepped.supports(144));
        assert!(!stepped.supports(128));
    }

    #[test]
    fn native_callback_routes_pcm_and_faults_without_allocating_or_advancing_after_sleep() {
        let (mut producer, consumer) = rtrb::RingBuffer::new(16);
        for sample in [0.5, -0.5, 0.25, -0.25] {
            producer.push(sample).unwrap();
        }
        let shared = Arc::new(StreamShared::new(1.));
        shared.playing.store(true, Ordering::Release);
        let mut left = [[0u8; 8]; 2];
        let mut right = [[0u8; 8]; 2];
        let state = Box::new(CallbackState {
            data: UnsafeCell::new(RenderData {
                renderer: Some(StreamRenderer::new(consumer, shared.clone(), 2, 48000)),
                scratch: vec![0.; 4],
                frames: 2,
                buffers: vec![
                    BufferInfo {
                        is_input: 0,
                        channel: 3,
                        buffers: [left[0].as_mut_ptr().cast(), left[1].as_mut_ptr().cast()],
                    },
                    BufferInfo {
                        is_input: 0,
                        channel: 1,
                        buffers: [right[0].as_mut_ptr().cast(), right[1].as_mut_ptr().cast()],
                    },
                ],
                types: vec![SampleType::new(18).unwrap(); 2],
            }),
            rendering: AtomicBool::new(false),
            armed: AtomicBool::new(true),
            fault: Arc::new(AtomicU32::new(0)),
            last_tick: AtomicU64::new(tick()),
        });
        assert!(
            CALLBACK
                .compare_exchange(
                    ptr::null_mut(),
                    ptr::from_ref(state.as_ref()).cast_mut(),
                    Ordering::AcqRel,
                    Ordering::Acquire
                )
                .is_ok()
        );
        let (_, allocations) = allocations_during(|| buffer_switch(0, 1));
        assert_eq!(allocations, 0);
        assert_eq!(
            i32::from_le_bytes(left[0][..4].try_into().unwrap()),
            1 << 30
        );
        assert_eq!(
            i32::from_le_bytes(right[0][..4].try_into().unwrap()),
            -(1 << 30)
        );
        assert_eq!(shared.played.load(Ordering::Acquire), 2);
        for (selector, fault) in [(3, 1), (4, 5), (5, 2), (6, 4)] {
            state.fault.store(0, Ordering::Release);
            let (_, allocations) = allocations_during(|| {
                message(selector, 0, ptr::null_mut(), ptr::null_mut());
                buffer_switch(1, 1);
            });
            assert_eq!(allocations, 0);
            assert_eq!(state.fault.load(Ordering::Acquire), fault);
            assert_eq!(left[1], [0; 8]);
            assert_eq!(shared.played.load(Ordering::Acquire), 2);
        }
        state.fault.store(0, Ordering::Release);
        let (_, allocations) = allocations_during(|| rate_changed(96000.));
        assert_eq!(allocations, 0);
        assert_eq!(state.fault.load(Ordering::Acquire), 3);
        state.fault.store(0, Ordering::Release);
        state
            .last_tick
            .store(tick().saturating_sub(STALL_MS + 1), Ordering::Release);
        let (_, allocations) = allocations_during(|| buffer_switch(0, 1));
        assert_eq!(allocations, 0);
        assert_eq!(state.fault.load(Ordering::Acquire), 6);
        assert_eq!(left[0], [0; 8]);
        assert_eq!(shared.played.load(Ordering::Acquire), 2);
        CALLBACK.store(ptr::null_mut(), Ordering::Release);
    }
}
