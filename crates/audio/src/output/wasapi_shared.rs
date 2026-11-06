//! WASAPI shared-mode output.
//!
//! Opens the default console render endpoint in shared mode at the endpoint's mix format, so the
//! Windows audio engine performs no format conversion of its own; the engine's single resampling
//! stage targets the mix rate. Rendering is event-driven on a dedicated thread registered with
//! MMCSS ("Pro Audio"). All COM objects live on that thread.
//!
//! The negotiated values come from the endpoint: `GetMixFormat` for the sample type, rate and
//! channels, `GetBufferSize` for the buffer and `GetStreamLatency` for the reported latency.
#![allow(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_domain::format::{ChannelCount, OutputFormat, OutputSampleFormat, SampleRate};
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId,
};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
    WaitForSingleObject,
};
use windows::core::{HRESULT, PWSTR, w};

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

impl Output for WasapiShared {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        let (ready_sender, ready) = std::sync::mpsc::channel();
        let (start, start_receiver) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let failure = Arc::new(OnceLock::new());
        let thread = std::thread::Builder::new()
            .name("parabolica-wasapi".to_owned())
            .spawn({
                let stop = stop.clone();
                let failure = failure.clone();
                move || render_thread(ready_sender, start_receiver, stop, failure)
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
}

struct Endpoint {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    kind: SampleKind,
    channels: usize,
    buffer_frames: u32,
}

fn render_thread(
    ready: Sender<Result<NegotiatedOutput, OutputError>>,
    start: Receiver<StreamRenderer>,
    stop: Arc<AtomicBool>,
    failure: Arc<OnceLock<OutputError>>,
) {
    // SAFETY: initializes COM for this thread; balanced by CoUninitialize below.
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    match open_endpoint() {
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
        _ => OutputError::Backend(format!("{context}: {error}")),
    }
}

fn open_endpoint() -> Result<(Endpoint, NegotiatedOutput), OutputError> {
    // SAFETY: COM is initialized on this thread; every pointer returned by these calls is either
    // an owned interface or freed with CoTaskMemFree after it has been read.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| backend("device enumerator", error))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|error| backend("default endpoint", error))?;
        let id = take_string(
            device
                .GetId()
                .map_err(|error| backend("endpoint ID", error))?,
        );
        let name = friendly_name(&device).unwrap_or_else(|| "Default output".to_owned());

        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| backend("activate", error))?;
        let mix = client
            .GetMixFormat()
            .map_err(|error| backend("mix format", error))?;
        let parsed = parse_format(mix);
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
        CoTaskMemFree(Some(mix as *const _));
        let (kind, sample, rate, channels) = parsed?;
        if let Some(result) = initialized {
            result.map_err(|error| backend("initialize", error))?;
        }

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
                backend: OutputBackend::WasapiShared,
                stable_id: id,
            },
            device_name: name,
            format: OutputFormat {
                sample,
                rate,
                channels,
            },
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
        };
        Ok((endpoint, negotiated))
    }
}

/// # Safety
/// `text` must be a string allocated with CoTaskMemAlloc; it is freed here.
unsafe fn take_string(text: PWSTR) -> String {
    // SAFETY: guaranteed by the caller.
    unsafe {
        let value = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0 as *const _));
        value
    }
}

fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: the property store and PROPVARIANT are owned here and released before returning.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let mut value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let text = PropVariantToStringAlloc(&value)
            .ok()
            .map(|text| take_string(text));
        let _ = PropVariantClear(&mut value);
        text.filter(|text| !text.is_empty())
    }
}

/// # Safety
/// `format` must point to a valid mix format returned by `GetMixFormat`.
unsafe fn parse_format(
    format: *const WAVEFORMATEX,
) -> Result<(SampleKind, OutputSampleFormat, SampleRate, ChannelCount), OutputError> {
    // SAFETY: guaranteed by the caller; packed structs are read unaligned.
    let base = unsafe { format.read_unaligned() };
    let tag = u32::from(base.wFormatTag);
    let bits = base.wBitsPerSample;
    let (float, valid_bits) = if tag == WAVE_FORMAT_EXTENSIBLE && base.cbSize >= 22 {
        // SAFETY: cbSize confirms the extensible fields are present.
        let extensible = unsafe { (format as *const WAVEFORMATEXTENSIBLE).read_unaligned() };
        let sub_format = extensible.SubFormat;
        // SAFETY: every union member is a u16.
        let valid = unsafe { extensible.Samples.wValidBitsPerSample };
        let valid = if valid == 0 { bits } else { valid };
        if sub_format == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
            (true, valid)
        } else if sub_format == KSDATAFORMAT_SUBTYPE_PCM {
            (false, valid)
        } else {
            return Err(OutputError::UnsupportedFormat(format!(
                "sub-format {sub_format:?}"
            )));
        }
    } else if tag == WAVE_FORMAT_IEEE_FLOAT {
        (true, bits)
    } else if tag == WAVE_FORMAT_PCM {
        (false, bits)
    } else {
        return Err(OutputError::UnsupportedFormat(format!(
            "format tag {tag:#x}"
        )));
    };

    let (kind, sample) = match (float, bits) {
        (true, 32) => (SampleKind::F32, OutputSampleFormat::Float { bits: 32 }),
        (false, 16) => (
            SampleKind::I16,
            OutputSampleFormat::Int {
                container_bits: 16,
                valid_bits: 16,
            },
        ),
        (false, 32) => (
            SampleKind::I32,
            OutputSampleFormat::Int {
                container_bits: 32,
                valid_bits: valid_bits as u8,
            },
        ),
        _ => {
            let kind = if float { "float" } else { "integer" };
            return Err(OutputError::UnsupportedFormat(format!("{bits}-bit {kind}")));
        }
    };
    let rate = SampleRate::new(base.nSamplesPerSec)
        .ok_or_else(|| OutputError::UnsupportedFormat("0 Hz".to_owned()))?;
    let channels = ChannelCount::new(base.nChannels)
        .ok_or_else(|| OutputError::UnsupportedFormat("0 channels".to_owned()))?;
    Ok((kind, sample, rate, channels))
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
            last_signal = Instant::now();
            // SAFETY: plain query on an initialized client.
            let padding = unsafe { endpoint.client.GetCurrentPadding() }
                .map_err(|error| backend("padding", error))?;
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
