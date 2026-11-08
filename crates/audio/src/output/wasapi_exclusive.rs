//! Exclusive format selection. No rate, endpoint or backend fallback is implicit.
#![allow(unsafe_code)]

use parabolica_domain::format::{OutputFormat, SourceFormat, SourceSampleFormat};
use parabolica_domain::output::{OutputSelection, SampleRatePolicy};
use parabolica_platform::windows::audio_devices::{SAMPLE_FORMATS, wave_format};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_EXCLUSIVE, IAudioClient, WAVEFORMATEXTENSIBLE,
};

use crate::engine::OutputError;

pub(super) fn negotiate(
    client: &IAudioClient,
    selection: &OutputSelection,
    source: &SourceFormat,
) -> Result<WAVEFORMATEXTENSIBLE, OutputError> {
    let rate = match selection.rate {
        SampleRatePolicy::FollowSource => source.rate,
        SampleRatePolicy::Fixed(rate) => rate,
    };
    for sample in SAMPLE_FORMATS {
        let lossless = match (source.sample, sample) {
            (
                Some(SourceSampleFormat::Int { bits }),
                parabolica_domain::format::OutputSampleFormat::Int { valid_bits, .. },
            ) => valid_bits >= bits,
            (
                Some(SourceSampleFormat::Int { bits }),
                parabolica_domain::format::OutputSampleFormat::Float { bits: 32 },
            ) => bits <= 24,
            (
                Some(SourceSampleFormat::Float { bits }),
                parabolica_domain::format::OutputSampleFormat::Float { bits: 32 },
            ) => bits <= 32,
            (None, parabolica_domain::format::OutputSampleFormat::Float { bits: 32 }) => true,
            _ => false,
        };
        if !lossless {
            continue;
        }
        let wave = wave_format(OutputFormat {
            sample,
            rate,
            channels: source.channels,
        });
        // SAFETY: the format includes its complete extension and COM belongs to this thread.
        if unsafe { client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wave.Format, None) }
            == windows::core::HRESULT(0)
        {
            return Ok(wave);
        }
    }
    Err(OutputError::UnsupportedFormat(format!(
        "{} Hz / {} ch; choose a supported fixed rate or WASAPI Shared",
        rate.hz(),
        source.channels.get(),
    )))
}
