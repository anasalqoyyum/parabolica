//! Event-driven WASAPI streams. Setup and capability queries run outside rendering.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_domain::format::{OutputSampleFormat, SourceFormat};
use parabolica_domain::output::OutputSelection;
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
};
use parabolica_platform::windows::audio_devices::{friendly_name, read_format, take_string};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, AUDCLNT_E_UNSUPPORTED_FORMAT,
    AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    DEVICE_STATE_ACTIVE, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    eConsole, eRender,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
    WaitForSingleObject,
};
use windows::core::{HRESULT, PCWSTR, w};

use crate::engine::{Output, OutputError, OutputSample, OutputStream, StreamRenderer};
use crate::realtime::RealtimeRender;

/// `HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`, returned when no render endpoint exists.
const E_NOTFOUND: HRESULT = HRESULT(0x8007_0490_u32 as i32);
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);
/// A shared-mode endpoint signals every period (about 10 ms); this long without one is a stall.
const STALL_TIMEOUT: Duration = Duration::from_secs(2);

/// WASAPI Shared on the default console render endpoint.
#[derive(Debug, Default)]
pub struct WasapiShared;

#[derive(Debug, Default)]
pub struct WasapiOutput;

impl Output for WasapiOutput {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        Err(OutputError::UnsupportedFormat(
            "a source format is required".to_owned(),
        ))
    }

    fn open_for(
        &mut self,
        selection: &OutputSelection,
        source: &SourceFormat,
    ) -> Result<Box<dyn OutputStream>, OutputError> {
        if !matches!(
            selection.backend,
            OutputBackend::WasapiShared | OutputBackend::WasapiExclusive
        ) {
            return Err(OutputError::BackendUnavailable);
        }
        if selection
            .device
            .as_ref()
            .is_some_and(|id| id.backend != selection.backend)
        {
            return Err(OutputError::BackendUnavailable);
        }
        open_stream(selection.clone(), Some(source.clone()))
    }
}

impl Output for WasapiShared {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        open_stream(OutputSelection::default(), None)
    }
}

fn open_stream(
    selection: OutputSelection,
    source: Option<SourceFormat>,
) -> Result<Box<dyn OutputStream>, OutputError> {
    let (ready_sender, ready) = std::sync::mpsc::channel();
    let (start, start_receiver) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let failure = Arc::new(OnceLock::new());
    let thread = std::thread::Builder::new()
        .name("parabolica-wasapi".to_owned())
        .spawn({
            let stop = stop.clone();
            let failure = failure.clone();
            move || {
                render_thread(
                    ready_sender,
                    start_receiver,
                    stop,
                    failure,
                    selection,
                    source,
                )
            }
        })
        .map_err(|error| OutputError::Backend(error.to_string()))?;
    let mut stream = SharedStream {
        negotiated: None,
        start: Some(start),
        stop,
        failure,
        thread: Some(thread),
    };
    let negotiated = match ready.recv_timeout(OPEN_TIMEOUT) {
        Ok(result) => result?,
        Err(_) => {
            // The thread is stuck in the driver. Detach it rather than block on a join; it
            // exits once the call returns, because the start channel is closed.
            drop(stream.thread.take());
            return Err(OutputError::Stalled);
        }
    };
    stream.negotiated = Some(negotiated);
    Ok(Box::new(stream))
}

struct SharedStream {
    negotiated: Option<NegotiatedOutput>,
    start: Option<Sender<StreamRenderer>>,
    stop: Arc<AtomicBool>,
    failure: Arc<OnceLock<OutputError>>,
    thread: Option<JoinHandle<()>>,
}

impl OutputStream for SharedStream {
    fn negotiated(&self) -> &NegotiatedOutput {
        self.negotiated
            .as_ref()
            .expect("set before the stream is returned")
    }

    fn start(&mut self, renderer: StreamRenderer) -> Result<(), OutputError> {
        let start = self.start.take().ok_or(OutputError::Stalled)?;
        start
            .send(renderer)
            .map_err(|_| self.failure.get().cloned().unwrap_or(OutputError::Stalled))
    }

    fn failure(&self) -> Option<OutputError> {
        self.failure.get().cloned()
    }
}

impl Drop for SharedStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.start = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy)]
enum SampleKind {
    F32,
    I16,
    I32,
    I24,
    I24In32,
}

struct Endpoint {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    kind: SampleKind,
    channels: usize,
    buffer_frames: u32,
    exclusive: bool,
}

fn render_thread(
    ready: Sender<Result<NegotiatedOutput, OutputError>>,
    start: Receiver<StreamRenderer>,
    stop: Arc<AtomicBool>,
    failure: Arc<OnceLock<OutputError>>,
    selection: OutputSelection,
    source: Option<SourceFormat>,
) {
    // SAFETY: initializes COM for this thread; balanced by CoUninitialize below.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    match open_endpoint(&selection, source.as_ref()) {
        Err(error) => {
            let _ = ready.send(Err(error));
        }
        Ok((endpoint, negotiated)) => {
            let _ = ready.send(Ok(negotiated));
            if let Ok(mut renderer) = start.recv()
                && let Err(error) = run(&endpoint, &mut renderer, &stop)
            {
                let _ = failure.set(error);
            }
            // SAFETY: the handle was created by CreateEventW and is closed once.
            let _ = unsafe { CloseHandle(endpoint.event) };
        }
    }
    if initialized {
        // SAFETY: balances the successful CoInitializeEx on this thread.
        unsafe { CoUninitialize() };
    }
}

fn backend(context: &str, error: windows::core::Error) -> OutputError {
    match error.code() {
        AUDCLNT_E_DEVICE_INVALIDATED => OutputError::DeviceLost,
        E_NOTFOUND => OutputError::NoDevice,
        AUDCLNT_E_DEVICE_IN_USE => OutputError::Busy,
        AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED => OutputError::UnsupportedFormat(
            "Exclusive access is disabled in Windows; enable it there or choose WASAPI Shared"
                .to_owned(),
        ),
        AUDCLNT_E_UNSUPPORTED_FORMAT => OutputError::UnsupportedFormat(
            "choose a supported fixed rate or WASAPI Shared".to_owned(),
        ),
        _ => OutputError::Backend(format!("{context}: {error}")),
    }
}

fn open_endpoint(
    selection: &OutputSelection,
    source: Option<&SourceFormat>,
) -> Result<(Endpoint, NegotiatedOutput), OutputError> {
    // SAFETY: COM is initialized on this thread; every pointer returned by these calls is either
    // an owned interface or freed with CoTaskMemFree after it has been read.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| backend("device enumerator", error))?;
        let device = if let Some(id) = &selection.device {
            let wide: Vec<u16> = id.stable_id.encode_utf16().chain(Some(0)).collect();
            enumerator.GetDevice(PCWSTR(wide.as_ptr()))
        } else {
            enumerator.GetDefaultAudioEndpoint(eRender, eConsole)
        }
        .map_err(|error| backend("selected endpoint", error))?;
        if device
            .GetState()
            .map_err(|error| backend("endpoint state", error))?
            != DEVICE_STATE_ACTIVE
        {
            return Err(OutputError::DeviceLost);
        }
        let id = take_string(
            device
                .GetId()
                .map_err(|error| backend("endpoint ID", error))?,
        );
        let name = friendly_name(&device).unwrap_or_else(|| id.clone());
        let mut client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| backend("activate", error))?;
        let exclusive = selection.backend == OutputBackend::WasapiExclusive;
        let format = if exclusive {
            let source = source.ok_or_else(|| {
                OutputError::UnsupportedFormat("missing source format".to_owned())
            })?;
            let wave = super::wasapi_exclusive::negotiate(&client, selection, source)?;
            let parsed =
                read_format(&wave.Format).map_err(|error| backend("requested format", error))?;
            let mut period = 0;
            client
                .GetDevicePeriod(None, Some(&mut period))
                .map_err(|error| backend("device period", error))?;
            let initialized = client.Initialize(
                AUDCLNT_SHAREMODE_EXCLUSIVE,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                period,
                period,
                &wave.Format,
                None,
            );
            if let Err(error) = initialized {
                if error.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                    return Err(backend("initialize Exclusive", error));
                }
                let frames = client
                    .GetBufferSize()
                    .map_err(|error| backend("aligned buffer", error))?;
                period = ((u64::from(frames) * 10_000_000 + u64::from(parsed.rate.hz()) / 2)
                    / u64::from(parsed.rate.hz())) as i64;
                client = device
                    .Activate(CLSCTX_ALL, None)
                    .map_err(|error| backend("reactivate", error))?;
                client
                    .Initialize(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                        period,
                        period,
                        &wave.Format,
                        None,
                    )
                    .map_err(|error| backend("initialize aligned Exclusive", error))?;
            }
            parsed
        } else {
            let mix = client
                .GetMixFormat()
                .map_err(|error| backend("mix format", error))?;
            let parsed = read_format(mix).map_err(|error| backend("mix format", error));
            let initialized = parsed.as_ref().ok().map(|_| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    0,
                    0,
                    mix,
                    None,
                )
            });
            CoTaskMemFree(Some(mix.cast()));
            if let Some(result) = initialized {
                result.map_err(|error| backend("initialize Shared", error))?;
            }
            parsed?
        };
        let kind = match format.sample {
            OutputSampleFormat::Float { bits: 32 } => SampleKind::F32,
            OutputSampleFormat::Int {
                container_bits: 16,
                valid_bits: 16,
            } => SampleKind::I16,
            OutputSampleFormat::Int {
                container_bits: 24,
                valid_bits: 24,
            } => SampleKind::I24,
            OutputSampleFormat::Int {
                container_bits: 32,
                valid_bits: 24,
            } => SampleKind::I24In32,
            OutputSampleFormat::Int {
                container_bits: 32,
                valid_bits: 32,
            } => SampleKind::I32,
            other => return Err(OutputError::UnsupportedFormat(format!("{other:?}"))),
        };
        let channels = format.channels;

        let buffer_frames = client
            .GetBufferSize()
            .map_err(|error| backend("buffer size", error))?;
        let latency = client.GetStreamLatency().ok();
        let render: IAudioRenderClient = client
            .GetService()
            .map_err(|error| backend("render client", error))?;
        let event =
            CreateEventW(None, false, false, None).map_err(|error| backend("event", error))?;
        if let Err(error) = client.SetEventHandle(event) {
            let _ = CloseHandle(event);
            return Err(backend("event handle", error));
        }

        let negotiated = NegotiatedOutput {
            device: OutputDeviceId {
                backend: selection.backend,
                stable_id: id,
            },
            device_name: name,
            format,
            channel_map: (0..channels.get()).collect(),
            buffer: BufferConfig {
                frames: buffer_frames,
                owner: BufferOwner::App,
            },
            reported_latency: latency
                .filter(|hns| *hns > 0)
                .map(|hns| Duration::from_nanos(hns as u64 * 100)),
        };
        let endpoint = Endpoint {
            client,
            render,
            event,
            kind,
            channels: usize::from(channels.get()),
            buffer_frames,
            exclusive,
        };
        Ok((endpoint, negotiated))
    }
}

fn run(
    endpoint: &Endpoint,
    renderer: &mut StreamRenderer,
    stop: &AtomicBool,
) -> Result<(), OutputError> {
    let mut task_index = 0u32;
    // SAFETY: registers this thread with MMCSS; reverted below. Failure only lowers priority.
    let mmcss = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task_index) }.ok();

    let result = (|| {
        fill(endpoint, renderer, endpoint.buffer_frames)?;
        // SAFETY: the client was initialized and its buffer prefilled.
        unsafe { endpoint.client.Start() }.map_err(|error| backend("start", error))?;
        let mut last_signal = Instant::now();
        while !stop.load(Ordering::Acquire) {
            // SAFETY: the event handle stays valid for the life of this loop.
            let signalled = unsafe { WaitForSingleObject(endpoint.event, 100) } == WAIT_OBJECT_0;
            if !signalled {
                if last_signal.elapsed() > STALL_TIMEOUT {
                    return Err(OutputError::Stalled);
                }
                continue;
            }
            if last_signal.elapsed() > STALL_TIMEOUT {
                return Err(OutputError::DeviceLost);
            }
            last_signal = Instant::now();
            // SAFETY: plain query on an initialized client.
            let padding = if endpoint.exclusive {
                0
            } else {
                unsafe { endpoint.client.GetCurrentPadding() }
                    .map_err(|error| backend("padding", error))?
            };
            let frames = endpoint.buffer_frames.saturating_sub(padding);
            if frames > 0 {
                fill(endpoint, renderer, frames)?;
            }
        }
        Ok(())
    })();

    // SAFETY: stopping an initialized client; harmless if it never started.
    let _ = unsafe { endpoint.client.Stop() };
    if let Some(handle) = mmcss {
        // SAFETY: reverts the registration made above.
        let _ = unsafe { AvRevertMmThreadCharacteristics(handle) };
    }
    result
}

/// Render `frames` frames into the endpoint buffer.
fn fill(
    endpoint: &Endpoint,
    renderer: &mut StreamRenderer,
    frames: u32,
) -> Result<(), OutputError> {
    // SAFETY: GetBuffer returns a buffer of `frames` frames in the mix format, which the parsed
    // `kind` and `channels` describe; it stays valid until ReleaseBuffer.
    unsafe {
        let data = endpoint
            .render
            .GetBuffer(frames)
            .map_err(|error| backend("get buffer", error))?;
        let samples = frames as usize * endpoint.channels;
        match endpoint.kind {
            SampleKind::F32 => render_into::<f32>(renderer, data, samples, endpoint.channels),
            SampleKind::I16 => render_into::<i16>(renderer, data, samples, endpoint.channels),
            SampleKind::I32 => render_into::<i32>(renderer, data, samples, endpoint.channels),
            SampleKind::I24 => render_into::<Packed24>(renderer, data, samples, endpoint.channels),
            SampleKind::I24In32 => {
                render_into::<Valid24>(renderer, data, samples, endpoint.channels)
            }
        }
        endpoint
            .render
            .ReleaseBuffer(frames, 0)
            .map_err(|error| backend("release buffer", error))
    }
}

/// # Safety
/// `data` must be a writable, suitably aligned buffer of `samples` values of `S`.
unsafe fn render_into<S: OutputSample>(
    renderer: &mut StreamRenderer,
    data: *mut u8,
    samples: usize,
    channels: usize,
) where
    StreamRenderer: RealtimeRender<S>,
{
    // SAFETY: guaranteed by the caller.
    let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<S>(), samples) };
    renderer.render(out, channels);
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Packed24([u8; 3]);

impl OutputSample for Packed24 {
    const SILENCE: Self = Self([0; 3]);
    fn from_pcm(sample: f64) -> Self {
        let bytes = quantize24(sample).to_le_bytes();
        Self([bytes[0], bytes[1], bytes[2]])
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
struct Valid24(i32);

impl OutputSample for Valid24 {
    const SILENCE: Self = Self(0);
    fn from_pcm(sample: f64) -> Self {
        Self(quantize24(sample) << 8)
    }
}

fn quantize24(sample: f64) -> i32 {
    (sample * 8_388_608.0)
        .round()
        .clamp(-8_388_608.0, 8_388_607.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_24_bit_output_preserves_pcm_and_zeros_container_padding() {
        for value in [-8_388_608i32, -1, 0, 1, 8_388_607] {
            let pcm = f64::from(value) / 8_388_608.0;
            let packed = Packed24::from_pcm(pcm).0;
            let bytes = value.to_le_bytes();
            assert_eq!(packed, bytes[..3]);
            assert_eq!(Valid24::from_pcm(pcm).0, value << 8);
        }
        assert_eq!(quantize24(1.), 8_388_607);
        assert_eq!(quantize24(-2.), -8_388_608);
    }
}
