//! Native endpoint identities and exact WASAPI format queries.
#![allow(unsafe_code)]

use parabolica_domain::format::{ChannelCount, OutputFormat, OutputSampleFormat, SampleRate};
use parabolica_domain::output::AudioEndpoint;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATEMASK_ALL,
    IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{KSDATAFORMAT_SUBTYPE_PCM, WAVE_FORMAT_EXTENSIBLE};
use windows::Win32::Media::Multimedia::{KSDATAFORMAT_SUBTYPE_IEEE_FLOAT, WAVE_FORMAT_IEEE_FLOAT};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::core::{PWSTR, Result};

pub const PROBE_RATES: [u32; 7] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 384_000];

pub fn enumerate() -> Result<Vec<AudioEndpoint>> {
    // All interfaces are released before this thread's COM apartment is uninitialized.
    unsafe {
        CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        let result = enumerate_initialized();
        CoUninitialize();
        result
    }
}

unsafe fn enumerate_initialized() -> Result<Vec<AudioEndpoint>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let default = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .ok()
            .and_then(|device| device.GetId().ok())
            .map(|id| take_string(id));
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE(DEVICE_STATEMASK_ALL))?;
        let mut endpoints = Vec::new();
        for index in 0..devices.GetCount()? {
            let device = devices.Item(index)?;
            let stable_id = take_string(device.GetId()?);
            let name = friendly_name(&device).unwrap_or_else(|| stable_id.clone());
            let available = device.GetState()? == DEVICE_STATE_ACTIVE;
            let mut endpoint = AudioEndpoint {
                available,
                is_default: default.as_ref() == Some(&stable_id),
                stable_id,
                name,
                mix_format: None,
                exclusive_formats: Vec::new(),
                capability_error: None,
            };
            if available {
                match capabilities(&device) {
                    Ok((mix, formats)) => {
                        endpoint.mix_format = Some(mix);
                        endpoint.exclusive_formats = formats;
                    }
                    Err(error) => endpoint.capability_error = Some(error.to_string()),
                }
            }
            endpoints.push(endpoint);
        }
        Ok(endpoints)
    }
}

unsafe fn capabilities(device: &IMMDevice) -> Result<(OutputFormat, Vec<OutputFormat>)> {
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let mix = client.GetMixFormat()?;
        let parsed = read_format(mix);
        CoTaskMemFree(Some(mix.cast()));
        let parsed = parsed?;
        let mut formats = Vec::new();
        for hz in PROBE_RATES {
            for sample in SAMPLE_FORMATS {
                let format = OutputFormat {
                    sample,
                    rate: SampleRate::new(hz).expect("nonzero"),
                    channels: ChannelCount::STEREO,
                };
                let wave = wave_format(format);
                if client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wave.Format, None)
                    == windows::core::HRESULT(0)
                {
                    formats.push(format);
                }
            }
        }
        Ok((parsed, formats))
    }
}

pub const SAMPLE_FORMATS: [OutputSampleFormat; 5] = [
    OutputSampleFormat::Int {
        container_bits: 16,
        valid_bits: 16,
    },
    OutputSampleFormat::Int {
        container_bits: 24,
        valid_bits: 24,
    },
    OutputSampleFormat::Int {
        container_bits: 32,
        valid_bits: 24,
    },
    OutputSampleFormat::Int {
        container_bits: 32,
        valid_bits: 32,
    },
    OutputSampleFormat::Float { bits: 32 },
];

pub fn wave_format(format: OutputFormat) -> WAVEFORMATEXTENSIBLE {
    let (bits, valid, float) = match format.sample {
        OutputSampleFormat::Int {
            container_bits,
            valid_bits,
        } => (container_bits, valid_bits, false),
        OutputSampleFormat::Float { bits } => (bits, bits, true),
    };
    let align = format.channels.get() * u16::from(bits) / 8;
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE as u16,
            nChannels: format.channels.get(),
            nSamplesPerSec: format.rate.hz(),
            nAvgBytesPerSec: format.rate.hz() * u32::from(align),
            nBlockAlign: align,
            wBitsPerSample: u16::from(bits),
            cbSize: 22,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: u16::from(valid),
        },
        dwChannelMask: match format.channels.get() {
            1 => 4,
            2 => 3,
            _ => 0,
        },
        SubFormat: if float {
            KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            KSDATAFORMAT_SUBTYPE_PCM
        },
    }
}

/// # Safety
/// `format` points to a complete WAVEFORMATEX, including the declared extension.
pub unsafe fn read_format(format: *const WAVEFORMATEX) -> Result<OutputFormat> {
    unsafe {
        let base = format.read_unaligned();
        let bits = base.wBitsPerSample;
        let (float, valid) =
            if u32::from(base.wFormatTag) == WAVE_FORMAT_EXTENSIBLE && base.cbSize >= 22 {
                let ext = format.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned();
                let sub = ext.SubFormat;
                let valid = ext.Samples.wValidBitsPerSample;
                if sub != KSDATAFORMAT_SUBTYPE_PCM && sub != KSDATAFORMAT_SUBTYPE_IEEE_FLOAT {
                    return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                        0x88890008u32 as i32,
                    )));
                }
                (
                    sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
                    if valid == 0 { bits } else { valid },
                )
            } else {
                match u32::from(base.wFormatTag) {
                    1 => (false, bits),
                    WAVE_FORMAT_IEEE_FLOAT => (true, bits),
                    _ => {
                        return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                            0x88890008u32 as i32,
                        )));
                    }
                }
            };
        let invalid =
            || windows::core::Error::from_hresult(windows::core::HRESULT(0x88890008u32 as i32));
        if bits > 64
            || valid == 0
            || valid > bits
            || u32::from(base.nBlockAlign) != u32::from(base.nChannels) * u32::from(bits) / 8
        {
            return Err(invalid());
        }
        Ok(OutputFormat {
            sample: if float {
                OutputSampleFormat::Float { bits: bits as u8 }
            } else {
                OutputSampleFormat::Int {
                    container_bits: bits as u8,
                    valid_bits: valid as u8,
                }
            },
            rate: SampleRate::new(base.nSamplesPerSec).ok_or_else(invalid)?,
            channels: ChannelCount::new(base.nChannels).ok_or_else(invalid)?,
        })
    }
}

/// # Safety
/// `text` is allocated by COM and is consumed and freed here.
pub unsafe fn take_string(text: PWSTR) -> String {
    unsafe {
        let value = text.to_string().unwrap_or_default();
        CoTaskMemFree(Some(text.0.cast()));
        value
    }
}

pub fn friendly_name(device: &IMMDevice) -> Option<String> {
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
