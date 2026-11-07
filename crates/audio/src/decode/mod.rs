//! Decoders producing internal PCM.
//!
//! [`Decoder`] opens a local file read-only through Symphonia and yields interleaved
//! [`PcmSample`]s. Integer sources are scaled by `2^(bits-1)`, so every decoded value is the exact
//! source sample.
//!
//! Only the container/codec pairs in [`SUPPORTED`] are enabled. Each pair has fixture tests; any
//! other pair fails with [`DecodeError::Unsupported`], naming the container and codec. Opus,
//! Vorbis, ALAC and FLAC in Ogg stay disabled until a second decoder validates them.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use parabolica_domain::format::{
    ChannelCount, Codec, PcmSample, SampleRate, SourceFormat, SourceSampleFormat,
};
use symphonia::core::audio::Channels;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known as codecs;
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::well_known as formats;
use symphonia::core::formats::{FormatId, FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia::core::units::{TimeBase, Timestamp};

mod mp4;

/// Container/codec pairs the player decodes, as `(container, codec)`.
pub const SUPPORTED: &[(&str, &str)] = &[
    ("FLAC", "FLAC"),
    ("WAV", "PCM"),
    ("MP3", "MP3"),
    ("MP4", "AAC-LC"),
];

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("can't read the file: {0}")]
    Open(#[source] std::io::Error),
    #[error("not a recognized audio file")]
    UnrecognizedFormat,
    #[error("the file has no audio track")]
    NoAudioTrack,
    #[error("{codec} audio in {container} files is not supported")]
    Unsupported { container: String, codec: String },
    #[error("unsupported {0}")]
    UnsupportedFeature(String),
    #[error("the file is corrupt at {}: {detail}", clock(*at))]
    Corrupt { at: Duration, detail: String },
    #[error("the file ends early: {decoded} of {expected} frames decoded")]
    Truncated { expected: u64, decoded: u64 },
    #[error("read error at {}: {source}", clock(*at))]
    Read {
        at: Duration,
        #[source]
        source: std::io::Error,
    },
    #[error("can't seek in this file: {0}")]
    Seek(String),
}

/// Encoder delay and padding as the container reports them, for gapless transitions.
///
/// When `trimmed` is true the decoder already removed these frames from its output, so frame 0 is
/// the first audible frame. Otherwise the output still contains them, or the container did not
/// report them (`None`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncoderDelay {
    pub delay_frames: Option<u32>,
    pub padding_frames: Option<u32>,
    pub trimmed: bool,
}

/// Display tags read with the audio. Library indexing owns the authoritative tags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BasicTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    pub container: &'static str,
    pub format: SourceFormat,
    pub channel_layout: Channels,
    /// Playable frames, when the container states them.
    pub frames: Option<u64>,
    pub encoder_delay: EncoderDelay,
    pub tags: BasicTags,
}

impl SourceInfo {
    pub fn continuous_with(&self, next: &Self) -> bool {
        self.format.rate == next.format.rate && self.channel_layout == next.channel_layout
    }

    pub fn duration(&self) -> Option<Duration> {
        self.frames
            .map(|frames| frames_to_duration(frames, self.format.rate))
    }
}

/// `m:ss.mmm`, for positions in error messages.
fn clock(at: Duration) -> String {
    let millis = at.as_millis();
    format!(
        "{}:{:02}.{:03}",
        millis / 60_000,
        millis / 1000 % 60,
        millis % 1000
    )
}

pub fn frames_to_duration(frames: u64, rate: SampleRate) -> Duration {
    let rate = u64::from(rate.hz());
    Duration::from_secs(frames / rate)
        + Duration::from_nanos((frames % rate) * 1_000_000_000 / rate)
}

/// The first frame at or after `position`.
pub fn duration_to_frames(position: Duration, rate: SampleRate) -> u64 {
    let nanos = position.as_nanos() * u128::from(rate.hz());
    u64::try_from(nanos.div_ceil(1_000_000_000)).unwrap_or(u64::MAX)
}

pub struct Decoder {
    path: PathBuf,
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    info: SourceInfo,
    /// Frame index of the next frame [`Decoder::next_block`] returns.
    position: u64,
    /// Frames still to drop after a seek landed before the requested frame.
    skip: u64,
    /// First frame of the current segment (0, or the last seek target).
    segment_start: u64,
    /// Lossy packets skipped as undecodable in the current segment.
    skipped_packets: u64,
    /// Whether packet timestamps are exact frame indices, so gaps reveal damaged data.
    exact_timestamps: bool,
    finished: bool,
    manual_delay: u64,
    manual_padding: usize,
    presentation_frames: Option<u64>,
    padding_tail: Vec<PcmSample>,
}

impl std::fmt::Debug for Decoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoder")
            .field("info", &self.info)
            .field("position", &self.position)
            .finish_non_exhaustive()
    }
}

impl Decoder {
    /// Open `path` read-only and prepare its default audio track.
    pub fn open(path: &Path) -> Result<Self, DecodeError> {
        let file = File::open(path).map_err(DecodeError::Open)?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            hint.with_extension(extension);
        }
        let mut reader = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|error| match error {
                SymphoniaError::IoError(error)
                    if error.kind() != std::io::ErrorKind::UnexpectedEof =>
                {
                    DecodeError::Open(error)
                }
                _ => DecodeError::UnrecognizedFormat,
            })?;

        let container_id = reader.format_info().format;
        let container = container_name(container_id, reader.format_info().short_name);
        let track = reader
            .default_track(symphonia::core::formats::TrackType::Audio)
            .ok_or(DecodeError::NoAudioTrack)?
            .clone();
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err(DecodeError::NoAudioTrack);
        };
        let codec = codec_name(params.codec);
        if !is_supported(container_id, params.codec) {
            return Err(DecodeError::Unsupported {
                container: container.to_owned(),
                codec,
            });
        }

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|error| DecodeError::UnsupportedFeature(error.to_string()))?;
        let format = source_format(&params, decoder.codec_params())?;
        let tags = read_tags(reader.as_mut());
        let decoder_trims = trims_delay(container_id, params.codec);
        let presentation = if container_id == formats::FORMAT_ID_ISOMP4 {
            mp4::presentation(path, track.id, format.rate.hz(), track.num_frames).map_err(
                |error| {
                    if error.kind() == std::io::ErrorKind::Unsupported {
                        DecodeError::UnsupportedFeature(error.to_string())
                    } else {
                        DecodeError::Corrupt {
                            at: Duration::ZERO,
                            detail: error.to_string(),
                        }
                    }
                },
            )?
        } else {
            None
        };
        let delay_frames = presentation.as_ref().map(|edit| edit.delay).or(track.delay);
        let manual_delay = if decoder_trims {
            0
        } else {
            u64::from(delay_frames.unwrap_or(0))
        };
        let manual_padding = if decoder_trims {
            0
        } else {
            track.padding.unwrap_or(0) as usize
        };
        if manual_padding > 65_536 {
            return Err(DecodeError::UnsupportedFeature(
                "encoder padding exceeds 65536 frames".to_owned(),
            ));
        }
        let encoder_delay = EncoderDelay {
            delay_frames,
            padding_frames: track.padding,
            trimmed: delay_frames.is_some() || track.padding.is_some(),
        };

        Ok(Self {
            path: path.to_owned(),
            info: SourceInfo {
                container,
                format,
                channel_layout: decoder
                    .codec_params()
                    .channels
                    .clone()
                    .or(params.channels.clone())
                    .expect("source_format validated the channel layout"),
                frames: presentation
                    .as_ref()
                    .map(|edit| edit.frames)
                    .or(track.num_frames),
                encoder_delay,
                tags,
            },
            track_id: track.id,
            time_base: track.time_base,
            reader,
            decoder,
            position: 0,
            skip: manual_delay,
            segment_start: 0,
            skipped_packets: 0,
            exact_timestamps: matches!(
                container_id,
                formats::FORMAT_ID_FLAC | formats::FORMAT_ID_WAVE
            ),
            finished: false,
            manual_delay,
            manual_padding,
            presentation_frames: presentation.map(|edit| edit.frames),
            padding_tail: Vec::new(),
        })
    }

    pub fn info(&self) -> &SourceInfo {
        &self.info
    }

    /// Frame index of the next frame `next_block` returns.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Append the next decoded frames to `out` as interleaved PCM and return how many frames
    /// were added. `Ok(0)` means the stream ended.
    pub fn next_block(&mut self, out: &mut Vec<PcmSample>) -> Result<usize, DecodeError> {
        let channels = usize::from(self.info.format.channels.get());
        loop {
            if self.finished {
                return Ok(0);
            }
            if self
                .presentation_frames
                .is_some_and(|end| self.position >= end)
            {
                return self.finish();
            }
            let packet = match self.reader.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return self.finish(),
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return self.finish();
                }
                Err(SymphoniaError::IoError(source)) => {
                    return Err(DecodeError::Read {
                        at: self.at(),
                        source,
                    });
                }
                Err(error) => return Err(self.corrupt(error)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            // Lossless packets carry their first frame index. A gap means the reader skipped
            // damaged data while resynchronising.
            if self.exact_timestamps && self.skip == 0 {
                let frame = self.timestamp_to_frame(packet.pts, self.info.format.rate);
                let expected =
                    self.position + self.manual_delay + (self.padding_tail.len() / channels) as u64;
                if frame != expected {
                    return Err(DecodeError::Corrupt {
                        at: self.at(),
                        detail: format!("{} frames are missing", frame.abs_diff(expected)),
                    });
                }
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // Lossy decoders recover at the next packet, so a damaged frame is skipped.
                // Lossless sources must decode exactly, so any damage fails the file.
                Err(SymphoniaError::DecodeError(_)) if !self.exact_timestamps => {
                    self.skipped_packets += 1;
                    continue;
                }
                Err(error) => return Err(self.corrupt(error)),
            };
            if decoded.spec().channels() != &self.info.channel_layout {
                return Err(DecodeError::UnsupportedFeature(
                    "channel layout change within the stream".to_owned(),
                ));
            }
            let frames = decoded.frames();
            let start = out.len();
            out.append(&mut self.padding_tail);
            let block_start = out.len();
            out.resize(block_start + frames * channels, 0.0);
            decoded.copy_to_slice_interleaved::<PcmSample, _>(&mut out[block_start..]);

            let dropped = usize::try_from(self.skip).map_or(frames, |skip| skip.min(frames));
            if dropped > 0 {
                out.drain(block_start..block_start + dropped * channels);
                self.skip -= dropped as u64;
            }
            let available = (out.len() - start) / channels;
            let withheld = self.manual_padding.min(available);
            let tail_start = out.len() - withheld * channels;
            self.padding_tail.extend_from_slice(&out[tail_start..]);
            out.truncate(tail_start);
            let kept = self
                .presentation_frames
                .map_or(available - withheld, |end| {
                    (available - withheld)
                        .min(usize::try_from(end - self.position).unwrap_or(usize::MAX))
                });
            out.truncate(start + kept * channels);
            self.position += kept as u64;
            if kept > 0 {
                return Ok(kept);
            }
        }
    }

    /// Seek so the next frame returned is exactly `frame`, and return the frame reached. A frame
    /// past the end clamps to the end.
    pub fn seek(&mut self, frame: u64) -> Result<u64, DecodeError> {
        if let Some(total) = self.info.frames.filter(|total| frame >= *total) {
            self.decoder.reset();
            self.position = total;
            self.skip = 0;
            self.finished = true;
            return Ok(total);
        }
        if self.finished {
            // Symphonia's MP4 reader cannot seek once it has read to the end; start afresh.
            *self = Self::open(&self.path)?;
        }
        let rate = self.info.format.rate;
        self.padding_tail.clear();
        let target = frame.saturating_add(self.manual_delay);
        // AAC overlap state needs preceding packets even when seeking to audible frame zero.
        let seek_target = if self.info.format.codec == Codec::Aac {
            target.saturating_sub(2048)
        } else {
            target
        };
        let seeked = self
            .reader
            .seek(
                SeekMode::Accurate,
                SeekTo::Timestamp {
                    ts: self.frame_to_timestamp(seek_target),
                    track_id: self.track_id,
                },
            )
            .map_err(|error| DecodeError::Seek(error.to_string()))?;
        self.decoder.reset();
        let required = if self.info.format.codec == Codec::Aac {
            target
        } else {
            self.timestamp_to_frame(seeked.required_ts, rate)
        };
        let actual = self.timestamp_to_frame(seeked.actual_ts, rate);
        self.skip = required.saturating_sub(actual);
        self.skipped_packets = 0;
        self.position = required.saturating_sub(self.manual_delay);
        self.segment_start = self.position;
        self.finished = false;
        Ok(self.position)
    }

    fn finish(&mut self) -> Result<usize, DecodeError> {
        self.finished = true;
        self.padding_tail.clear();
        if self.skipped_packets > 0 && self.position == self.segment_start {
            return Err(DecodeError::Corrupt {
                at: self.at(),
                detail: format!("{} packets could not be decoded", self.skipped_packets),
            });
        }
        match self.info.frames {
            // Only lossless containers state an exact length; an MP3 without a Xing header
            // reports an estimate.
            Some(expected)
                if self.exact_timestamps && self.position < expected && self.skip == 0 =>
            {
                Err(DecodeError::Truncated {
                    expected,
                    decoded: self.position,
                })
            }
            _ => Ok(0),
        }
    }

    fn at(&self) -> Duration {
        frames_to_duration(self.position, self.info.format.rate)
    }

    fn corrupt(&self, error: SymphoniaError) -> DecodeError {
        match error {
            SymphoniaError::Unsupported(feature) => {
                DecodeError::UnsupportedFeature(feature.to_owned())
            }
            error => DecodeError::Corrupt {
                at: self.at(),
                detail: error.to_string(),
            },
        }
    }

    fn frame_to_timestamp(&self, frame: u64) -> Timestamp {
        let ts = match self.time_base {
            Some(base) => {
                let numer = u128::from(base.numer.get());
                let denom = u128::from(base.denom.get());
                let rate = u128::from(self.info.format.rate.hz());
                u128::from(frame) * denom / (numer * rate)
            }
            None => u128::from(frame),
        };
        Timestamp::new(i64::try_from(ts).unwrap_or(i64::MAX))
    }

    fn timestamp_to_frame(&self, ts: Timestamp, rate: SampleRate) -> u64 {
        let ts = u128::try_from(ts.get()).unwrap_or(0);
        let frames = match self.time_base {
            Some(base) => {
                let numer = u128::from(base.numer.get());
                let denom = u128::from(base.denom.get());
                ts * numer * u128::from(rate.hz()) / denom
            }
            None => ts,
        };
        u64::try_from(frames).unwrap_or(u64::MAX)
    }
}

fn is_supported(container: FormatId, codec: AudioCodecId) -> bool {
    match container {
        formats::FORMAT_ID_FLAC => codec == codecs::CODEC_ID_FLAC,
        formats::FORMAT_ID_WAVE => pcm_bits(codec).is_some(),
        formats::FORMAT_ID_MP3 => codec == codecs::CODEC_ID_MP3,
        formats::FORMAT_ID_ISOMP4 => codec == codecs::CODEC_ID_AAC,
        _ => false,
    }
}

/// MP3 applies packet trimming inside Symphonia. Other codecs need reported trimming here.
/// AAC presentation edits are applied by the wrapper because the MP4 reader ignores them.
fn trims_delay(container: FormatId, codec: AudioCodecId) -> bool {
    container == formats::FORMAT_ID_MP3 && codec == codecs::CODEC_ID_MP3
}

fn container_name(id: FormatId, short_name: &'static str) -> &'static str {
    match id {
        formats::FORMAT_ID_FLAC => "FLAC",
        formats::FORMAT_ID_WAVE => "WAV",
        formats::FORMAT_ID_MP3 => "MP3",
        formats::FORMAT_ID_ISOMP4 => "MP4",
        formats::FORMAT_ID_OGG => "Ogg",
        formats::FORMAT_ID_ADTS => "ADTS",
        _ => short_name,
    }
}

fn codec_name(codec: AudioCodecId) -> String {
    let name = match codec {
        codecs::CODEC_ID_FLAC => "FLAC",
        codecs::CODEC_ID_MP3 => "MP3",
        codecs::CODEC_ID_MP2 => "MP2",
        codecs::CODEC_ID_MP1 => "MP1",
        codecs::CODEC_ID_AAC => "AAC",
        codecs::CODEC_ID_ALAC => "ALAC",
        codecs::CODEC_ID_OPUS => "Opus",
        codecs::CODEC_ID_VORBIS => "Vorbis",
        codecs::CODEC_ID_SPEEX => "Speex",
        codecs::CODEC_ID_AC3 => "AC-3",
        codecs::CODEC_ID_EAC3 => "E-AC-3",
        codecs::CODEC_ID_WAVPACK => "WavPack",
        codecs::CODEC_ID_MONKEYS_AUDIO => "Monkey's Audio",
        codecs::CODEC_ID_PCM_ALAW => "A-law",
        codecs::CODEC_ID_PCM_MULAW => "µ-law",
        symphonia::core::codecs::audio::CODEC_ID_NULL_AUDIO => "unknown",
        codec if pcm_bits(codec).is_some() => "PCM",
        codecs::CODEC_ID_ADPCM_G722
        | codecs::CODEC_ID_ADPCM_G726
        | codecs::CODEC_ID_ADPCM_G726LE
        | codecs::CODEC_ID_ADPCM_MS
        | codecs::CODEC_ID_ADPCM_IMA_WAV
        | codecs::CODEC_ID_ADPCM_IMA_QT => "ADPCM",
        codec => return format!("codec {codec}"),
    };
    name.to_owned()
}

/// Width of an uncompressed PCM codec, and whether it is float.
fn pcm_bits(codec: AudioCodecId) -> Option<SourceSampleFormat> {
    use SourceSampleFormat::{Float, Int};
    let format = match codec {
        codecs::CODEC_ID_PCM_U8 | codecs::CODEC_ID_PCM_S8 => Int { bits: 8 },
        codecs::CODEC_ID_PCM_S16LE | codecs::CODEC_ID_PCM_S16BE => Int { bits: 16 },
        codecs::CODEC_ID_PCM_U16LE | codecs::CODEC_ID_PCM_U16BE => Int { bits: 16 },
        codecs::CODEC_ID_PCM_S24LE | codecs::CODEC_ID_PCM_S24BE => Int { bits: 24 },
        codecs::CODEC_ID_PCM_U24LE | codecs::CODEC_ID_PCM_U24BE => Int { bits: 24 },
        codecs::CODEC_ID_PCM_S32LE | codecs::CODEC_ID_PCM_S32BE => Int { bits: 32 },
        codecs::CODEC_ID_PCM_U32LE | codecs::CODEC_ID_PCM_U32BE => Int { bits: 32 },
        codecs::CODEC_ID_PCM_F32LE | codecs::CODEC_ID_PCM_F32BE => Float { bits: 32 },
        codecs::CODEC_ID_PCM_F64LE | codecs::CODEC_ID_PCM_F64BE => Float { bits: 64 },
        _ => return None,
    };
    Some(format)
}

fn source_format(
    params: &symphonia::core::codecs::audio::AudioCodecParameters,
    decoded: &symphonia::core::codecs::audio::AudioCodecParameters,
) -> Result<SourceFormat, DecodeError> {
    let missing = |what: &str| DecodeError::UnsupportedFeature(format!("stream without {what}"));
    let rate = decoded
        .sample_rate
        .or(params.sample_rate)
        .and_then(SampleRate::new)
        .ok_or_else(|| missing("a sample rate"))?;
    let channels = decoded
        .channels
        .as_ref()
        .or(params.channels.as_ref())
        .and_then(|channels| u16::try_from(channels.count()).ok())
        .and_then(ChannelCount::new)
        .ok_or_else(|| missing("a channel count"))?;
    let codec = match params.codec {
        codecs::CODEC_ID_FLAC => Codec::Flac,
        codecs::CODEC_ID_MP3 => Codec::Mp3,
        codecs::CODEC_ID_AAC => Codec::Aac,
        _ => Codec::Wav,
    };
    let sample = match codec {
        Codec::Flac => params
            .bits_per_sample
            .and_then(|bits| u8::try_from(bits).ok())
            .map(|bits| SourceSampleFormat::Int { bits }),
        Codec::Wav => pcm_bits(params.codec),
        _ => None,
    };
    Ok(SourceFormat {
        codec,
        sample,
        rate,
        channels,
    })
}

fn read_tags(reader: &mut dyn FormatReader) -> BasicTags {
    let mut tags = BasicTags::default();
    let mut metadata = reader.metadata();
    let Some(revision) = metadata.skip_to_latest() else {
        return tags;
    };
    let all = revision
        .media
        .tags
        .iter()
        .chain(revision.per_track.iter().flat_map(|t| &t.metadata.tags));
    for tag in all {
        let (slot, value) = match &tag.std {
            Some(StandardTag::TrackTitle(value)) => (&mut tags.title, value),
            Some(StandardTag::Artist(value)) => (&mut tags.artist, value),
            Some(StandardTag::Album(value)) => (&mut tags.album, value),
            _ => continue,
        };
        let value = value.trim();
        if slot.is_none() && !value.is_empty() {
            *slot = Some(value.to_owned());
        }
    }
    tags
}

#[cfg(test)]
mod trim_tests;
