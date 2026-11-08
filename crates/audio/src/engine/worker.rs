//! The decoder worker fills one output ring for the complete deterministic sequence.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_domain::control::{Generation, GenerationCounter, Tagged};
use parabolica_domain::format::PcmSample;
use parabolica_domain::source::{FrameSpan, ResolvedSource};
use rtrb::Producer;

use super::controller::Message;
use super::render::StreamShared;
use super::resample::RateConverter;
use super::{OutputError, PlaybackError};
use crate::decode::{Decoder, SourceInfo, duration_to_frames};
use crate::transitions::PlaybackSequence;

// Repeated tiny entries must not reopen files at the device frame rate.
const CACHE_SAMPLES: usize = 65_536;
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    Load {
        generation: Generation,
        sequence: PlaybackSequence,
        index: usize,
        position: Duration,
    },
    Stop {
        generation: Generation,
    },
}

#[derive(Debug)]
pub(crate) enum WorkerReport {
    Boundary {
        output_frame: u64,
        index: usize,
        start: u64,
        info: Result<Box<SourceInfo>, PlaybackError>,
    },
    Finished {
        output_frame: u64,
    },
    Reconfigure {
        output_frame: u64,
        index: usize,
        info: Box<SourceInfo>,
    },
    Primed,
    Failed(PlaybackError),
}

#[derive(Debug)]
pub(crate) struct WorkerHandle {
    commands: Option<Sender<WorkerCommand>>,
    thread: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub fn spawn(
        producer: Producer<PcmSample>,
        shared: Arc<StreamShared>,
        generations: Arc<GenerationCounter>,
        reports: Sender<Message>,
        selection: parabolica_domain::output::OutputSelection,
        negotiated: parabolica_domain::output::NegotiatedOutput,
    ) -> Self {
        let output_rate = negotiated.format.rate.hz();
        let output_channels = usize::from(negotiated.format.channels.get());
        let (commands, receiver) = std::sync::mpsc::channel();
        let worker = Worker {
            commands: receiver,
            selection,
            negotiated,
            reports,
            producer,
            shared,
            generations,
            output_rate,
            output_channels,
            prime_frames: output_rate as usize / 10,
            track: None,
        };
        let thread = std::thread::Builder::new()
            .name("parabolica-decoder".to_owned())
            .spawn(move || worker.run())
            .expect("the decoder thread starts");
        Self {
            commands: Some(commands),
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: WorkerCommand) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        self.commands = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct CachedSource {
    info: SourceInfo,
    samples: Vec<PcmSample>,
}

enum Input {
    Decoder(Box<Decoder>),
    Cached {
        source: Arc<CachedSource>,
        at: usize,
    },
}

impl Input {
    fn info(&self) -> &SourceInfo {
        match self {
            Self::Decoder(decoder) => decoder.info(),
            Self::Cached { source, .. } => &source.info,
        }
    }

    fn next_block(
        &mut self,
        out: &mut Vec<PcmSample>,
    ) -> Result<usize, crate::decode::DecodeError> {
        match self {
            Self::Decoder(decoder) => decoder.next_block(out),
            Self::Cached { source, at } => {
                let channels = usize::from(source.info.format.channels.get());
                let end = (*at + 1024 * channels).min(source.samples.len());
                out.extend_from_slice(&source.samples[*at..end]);
                let frames = (end - *at) / channels;
                *at = end;
                Ok(frames)
            }
        }
    }
}

struct Track {
    generation: Generation,
    sequence: PlaybackSequence,
    index: usize,
    decoder: Input,
    cache: HashMap<usize, Arc<CachedSource>>,
    cache_samples: usize,
    cache_candidate: Option<Vec<PcmSample>>,
    converter: Option<RateConverter>,
    channels: usize,
    decoded: Vec<PcmSample>,
    converted: Vec<PcmSample>,
    pending: Vec<PcmSample>,
    pending_at: usize,
    pushed_frames: u64,
    group_frames: u64,
    group_origin: u64,
    entry_frames: u64,
    primed: bool,
    decoder_done: bool,
    finished: bool,
}

struct Worker {
    selection: parabolica_domain::output::OutputSelection,
    negotiated: parabolica_domain::output::NegotiatedOutput,
    commands: Receiver<WorkerCommand>,
    reports: Sender<Message>,
    producer: Producer<PcmSample>,
    shared: Arc<StreamShared>,
    generations: Arc<GenerationCounter>,
    output_rate: u32,
    output_channels: usize,
    prime_frames: usize,
    track: Option<Track>,
}

impl Worker {
    fn run(mut self) {
        loop {
            let busy = self.track.as_ref().is_some_and(|track| !track.finished);
            let command = if busy {
                match self.commands.try_recv() {
                    Ok(command) => Some(command),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match self.commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            };
            if let Some(command) = command {
                self.handle(command);
                continue;
            }
            if !self.step() {
                match self.commands.recv_timeout(Duration::from_millis(1)) {
                    Ok(command) => self.handle(command),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        }
    }

    fn report(&self, generation: Generation, value: WorkerReport) {
        let _ = self
            .reports
            .send(Message::Worker(Tagged { generation, value }));
    }

    fn handle(&mut self, command: WorkerCommand) {
        self.track = None;
        match command {
            WorkerCommand::Load {
                generation,
                sequence,
                index,
                position,
            } => {
                if self.flush(generation)
                    && let Err(error) = self.load(generation, sequence, index, position)
                {
                    self.report(generation, WorkerReport::Failed(error));
                }
            }
            WorkerCommand::Stop { generation } => {
                self.flush(generation);
            }
        }
    }

    fn flush(&self, generation: Generation) -> bool {
        if !self.generations.is_current(generation) {
            return false;
        }
        self.shared
            .epoch_request
            .store(generation.get(), Ordering::Release);
        let deadline = Instant::now() + FLUSH_TIMEOUT;
        while self.shared.epoch_ack.load(Ordering::Acquire) != generation.get() {
            if !self.generations.is_current(generation) {
                return false;
            }
            if Instant::now() > deadline {
                self.report(
                    generation,
                    WorkerReport::Failed(PlaybackError::Output(OutputError::Stalled)),
                );
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    fn open(&self, source: &ResolvedSource) -> Result<Decoder, PlaybackError> {
        let path = match source {
            ResolvedSource::Local {
                path,
                span: FrameSpan::Whole,
                ..
            } => path,
            ResolvedSource::Local { .. } => return Err(PlaybackError::CueRange),
            _ => return Err(PlaybackError::NotLocal),
        };
        let decoder =
            Decoder::open(path).map_err(|error| PlaybackError::Decode(error.to_string()))?;
        let channels = usize::from(decoder.info().format.channels.get());
        if channels > self.output_channels
            && !self
                .selection
                .needs_new_stream(&decoder.info().format, &self.negotiated)
        {
            return Err(PlaybackError::ChannelLayout {
                source_channels: channels,
                output_channels: self.output_channels,
            });
        }
        Ok(decoder)
    }

    fn converter(&self, info: &SourceInfo) -> Result<Option<RateConverter>, PlaybackError> {
        if info.format.rate.hz() == self.output_rate {
            return Ok(None);
        }
        RateConverter::new(
            info.format.rate.hz(),
            self.output_rate,
            usize::from(info.format.channels.get()),
        )
        .map(Some)
        .map_err(PlaybackError::Resampler)
    }

    fn load(
        &mut self,
        generation: Generation,
        sequence: PlaybackSequence,
        index: usize,
        position: Duration,
    ) -> Result<(), PlaybackError> {
        let mut decoder = self.open(&sequence.entries[index].1)?;
        let info = decoder.info().clone();
        if self
            .selection
            .needs_new_stream(&info.format, &self.negotiated)
        {
            return Err(PlaybackError::Output(OutputError::UnsupportedFormat(
                "the source format changed while opening; retry selected output".to_owned(),
            )));
        }
        let requested = duration_to_frames(position, info.format.rate);
        let start = if requested > 0 {
            decoder
                .seek(requested)
                .map_err(|error| PlaybackError::Decode(error.to_string()))?
        } else {
            0
        };
        let converter = self.converter(&info)?;
        self.report(
            generation,
            WorkerReport::Boundary {
                output_frame: 0,
                index,
                start,
                info: Ok(Box::new(info.clone())),
            },
        );
        self.track = Some(Track {
            generation,
            sequence,
            index,
            decoder: Input::Decoder(Box::new(decoder)),
            converter,
            cache: HashMap::new(),
            cache_samples: 0,
            cache_candidate: (start == 0).then(Vec::new),
            channels: usize::from(info.format.channels.get()),
            decoded: Vec::new(),
            converted: Vec::new(),
            pending: Vec::new(),
            pending_at: 0,
            pushed_frames: 0,
            group_frames: 0,
            group_origin: 0,
            entry_frames: start,
            primed: false,
            decoder_done: false,
            finished: false,
        });
        Ok(())
    }

    fn flush_converter(&self, track: &mut Track) {
        if let Some(converter) = &mut track.converter {
            track.converted.clear();
            converter.finish(&mut track.converted);
            map_channels(
                &track.converted,
                track.channels,
                self.output_channels,
                &mut track.pending,
            );
        }
    }

    fn advance(&self, track: &mut Track) {
        let rate = track.decoder.info().format.rate.hz();
        let end = track.group_origin
            + ((u128::from(track.group_frames) * u128::from(self.output_rate)
                + u128::from(rate) / 2)
                / u128::from(rate)) as u64;
        if track.entry_frames == 0 {
            self.flush_converter(track);
            self.report(
                track.generation,
                WorkerReport::Boundary {
                    output_frame: end,
                    index: track.index,
                    start: 0,
                    info: Err(PlaybackError::Decode(
                        "the entry has no playable frames".to_owned(),
                    )),
                },
            );
            track.decoder_done = true;
            return;
        }
        if let Some(samples) = track.cache_candidate.take() {
            track.cache_samples += samples.len();
            track.cache.insert(
                track.index,
                Arc::new(CachedSource {
                    info: track.decoder.info().clone(),
                    samples,
                }),
            );
        }
        let next = track.sequence.next(track.index);
        let Some(index) = next else {
            self.flush_converter(track);
            track.decoder_done = true;
            return;
        };
        let opened = if let Some(source) = track.cache.get(&index) {
            Ok(Input::Cached {
                source: source.clone(),
                at: 0,
            })
        } else {
            self.open(&track.sequence.entries[index].1)
                .map(|decoder| Input::Decoder(Box::new(decoder)))
        };
        let decoder = match opened {
            Ok(decoder) => decoder,
            Err(error) => {
                self.flush_converter(track);
                self.report(
                    track.generation,
                    WorkerReport::Boundary {
                        output_frame: end,
                        index,
                        start: 0,
                        info: Err(error),
                    },
                );
                track.decoder_done = true;
                return;
            }
        };
        let info = decoder.info().clone();
        if self
            .selection
            .needs_new_stream(&info.format, &self.negotiated)
        {
            self.flush_converter(track);
            self.report(
                track.generation,
                WorkerReport::Reconfigure {
                    output_frame: end,
                    index,
                    info: Box::new(info),
                },
            );
            track.decoder_done = true;
            return;
        }
        if !track.decoder.info().continuous_with(&info) {
            self.flush_converter(track);
            match self.converter(&info) {
                Ok(converter) => track.converter = converter,
                Err(error) => {
                    self.report(
                        track.generation,
                        WorkerReport::Boundary {
                            output_frame: end,
                            index,
                            start: 0,
                            info: Err(error),
                        },
                    );
                    track.decoder_done = true;
                    return;
                }
            }
            track.group_frames = 0;
            track.group_origin = end;
        }
        self.report(
            track.generation,
            WorkerReport::Boundary {
                output_frame: end,
                index,
                start: 0,
                info: Ok(Box::new(info)),
            },
        );
        track.channels = usize::from(decoder.info().format.channels.get());
        track.cache_candidate = matches!(decoder, Input::Decoder(_)).then(Vec::new);
        track.decoder = decoder;
        track.index = index;
        track.entry_frames = 0;
    }

    fn step(&mut self) -> bool {
        let Some(mut track) = self.track.take() else {
            return true;
        };
        if !self.generations.is_current(track.generation) {
            self.track = Some(track);
            return false;
        }
        let progressed = if track.pending_at == track.pending.len() {
            track.pending.clear();
            track.pending_at = 0;
            if track.decoder_done {
                self.report(
                    track.generation,
                    WorkerReport::Finished {
                        output_frame: track.pushed_frames,
                    },
                );
                self.shared
                    .source_end
                    .store(track.generation.get(), Ordering::Release);
                track.finished = true;
                if !track.primed {
                    track.primed = true;
                    self.report(track.generation, WorkerReport::Primed);
                }
            } else {
                track.decoded.clear();
                match track.decoder.next_block(&mut track.decoded) {
                    Ok(0) => self.advance(&mut track),
                    Ok(frames) => {
                        if let Some(candidate) = &mut track.cache_candidate {
                            if candidate.len() + track.decoded.len()
                                <= CACHE_SAMPLES - track.cache_samples
                            {
                                candidate.extend_from_slice(&track.decoded);
                            } else {
                                track.cache_candidate = None;
                            }
                        }
                        track.group_frames += frames as u64;
                        track.entry_frames += frames as u64;
                        let block = if let Some(converter) = &mut track.converter {
                            track.converted.clear();
                            converter.process(&track.decoded, &mut track.converted);
                            &track.converted
                        } else {
                            &track.decoded
                        };
                        map_channels(
                            block,
                            track.channels,
                            self.output_channels,
                            &mut track.pending,
                        );
                    }
                    Err(error) => {
                        self.flush_converter(&mut track);
                        let rate = track.decoder.info().format.rate.hz();
                        let end = track.group_origin
                            + ((u128::from(track.group_frames) * u128::from(self.output_rate)
                                + u128::from(rate) / 2)
                                / u128::from(rate)) as u64;
                        self.report(
                            track.generation,
                            WorkerReport::Boundary {
                                output_frame: end,
                                index: track.index,
                                start: 0,
                                info: Err(PlaybackError::Decode(error.to_string())),
                            },
                        );
                        track.decoder_done = true;
                    }
                }
            }
            true
        } else {
            let space = self.producer.slots() / self.output_channels * self.output_channels;
            let count = space.min(track.pending.len() - track.pending_at);
            if count == 0 {
                false
            } else {
                self.producer
                    .write_chunk_uninit(count)
                    .expect("slots were checked")
                    .fill_from_iter(
                        track.pending[track.pending_at..track.pending_at + count]
                            .iter()
                            .copied(),
                    );
                track.pending_at += count;
                track.pushed_frames += (count / self.output_channels) as u64;
                if !track.primed && track.pushed_frames >= self.prime_frames as u64 {
                    track.primed = true;
                    self.report(track.generation, WorkerReport::Primed);
                }
                true
            }
        };
        self.track = Some(track);
        progressed
    }
}

fn map_channels(input: &[PcmSample], source: usize, output: usize, out: &mut Vec<PcmSample>) {
    if source == output {
        out.extend_from_slice(input);
        return;
    }
    for frame in input.chunks_exact(source) {
        let start = out.len();
        out.resize(start + output, 0.0);
        let target = &mut out[start..];
        target[..source].copy_from_slice(frame);
        if source == 1 && output >= 2 {
            target[1] = frame[0];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::map_channels;

    #[test]
    fn maps_mono_to_both_front_channels_and_pads_extra_channels() {
        let mut out = Vec::new();
        map_channels(&[0.1, 0.2], 1, 2, &mut out);
        assert_eq!(out, [0.1, 0.1, 0.2, 0.2]);
        out.clear();
        map_channels(&[0.1, 0.2, 0.3, 0.4], 2, 4, &mut out);
        assert_eq!(out, [0.1, 0.2, 0.0, 0.0, 0.3, 0.4, 0.0, 0.0]);
    }
}
