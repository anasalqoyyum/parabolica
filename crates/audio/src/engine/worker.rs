//! The decoder worker: decodes, converts the rate, maps channels and fills the ring buffer.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parabolica_domain::control::{Generation, GenerationCounter, Tagged};
use parabolica_domain::format::PcmSample;
use rtrb::Producer;

use super::controller::Message;
use super::render::StreamShared;
use super::resample::RateConverter;
use super::{OutputError, PlaybackError};
use crate::decode::{Decoder, SourceInfo};

/// How long the worker waits for the renderer to acknowledge a flush.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) enum WorkerCommand {
    /// Open `path` and start at source frame `start`.
    Load {
        generation: Generation,
        path: PathBuf,
        start: u64,
    },
    Seek {
        generation: Generation,
        frame: u64,
    },
    Stop {
        generation: Generation,
    },
}

#[derive(Debug)]
pub(crate) enum WorkerReport {
    Loaded(Box<SourceInfo>),
    Seeked {
        frame: u64,
    },
    /// Enough audio is buffered to start without an immediate underrun, or the source ended.
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
        output_rate: u32,
        output_channels: usize,
        generations: Arc<GenerationCounter>,
        reports: Sender<Message>,
    ) -> Self {
        let (commands, receiver) = std::sync::mpsc::channel();
        let worker = Worker {
            commands: receiver,
            reports,
            producer,
            shared,
            generations,
            output_rate,
            output_channels,
            prime_frames: output_rate as usize / 10,
            track: None,
            last_error: None,
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

struct Track {
    generation: Generation,
    decoder: Decoder,
    converter: Option<RateConverter>,
    channels: usize,
    decoded: Vec<PcmSample>,
    converted: Vec<PcmSample>,
    /// Frames in the output channel layout, waiting for ring space.
    pending: Vec<PcmSample>,
    pending_at: usize,
    pushed_frames: usize,
    primed: bool,
    decoder_done: bool,
    finished: bool,
}

struct Worker {
    commands: Receiver<WorkerCommand>,
    reports: Sender<Message>,
    producer: Producer<PcmSample>,
    shared: Arc<StreamShared>,
    generations: Arc<GenerationCounter>,
    output_rate: u32,
    output_channels: usize,
    prime_frames: usize,
    track: Option<Track>,
    /// The error that made the worker drop its track, re-reported to a seek that arrives after.
    last_error: Option<PlaybackError>,
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
                match self.commands.recv_timeout(Duration::from_millis(5)) {
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
        match command {
            WorkerCommand::Load {
                generation,
                path,
                start,
            } => {
                self.track = None;
                self.last_error = None;
                if self.flush(generation) {
                    self.load(generation, path, start);
                }
            }
            WorkerCommand::Seek { generation, frame } => {
                if self.track.is_none() && self.generations.is_current(generation) {
                    // The decoder failed while this seek was on its way, and that failure was
                    // reported under the older generation. Report again so the controller
                    // reopens the file instead of waiting for audio.
                    let error = self.last_error.clone().unwrap_or_else(|| {
                        PlaybackError::Decode("the file is no longer open".to_owned())
                    });
                    return self.report(generation, WorkerReport::Failed(error));
                }
                if self.track.is_none() || !self.flush(generation) {
                    // Superseded: the newer command, already queued, uses the track.
                    return;
                }
                let Some(mut track) = self.track.take() else {
                    return;
                };
                match track.decoder.seek(frame) {
                    Ok(frame) => {
                        if let Some(converter) = &mut track.converter {
                            converter.reset();
                        }
                        track.generation = generation;
                        track.pending.clear();
                        track.pending_at = 0;
                        track.pushed_frames = 0;
                        track.primed = false;
                        track.decoder_done = false;
                        track.finished = false;
                        self.track = Some(track);
                        self.report(generation, WorkerReport::Seeked { frame });
                    }
                    Err(error) => self.report(
                        generation,
                        WorkerReport::Failed(PlaybackError::Decode(error.to_string())),
                    ),
                }
            }
            WorkerCommand::Stop { generation } => {
                self.track = None;
                self.flush(generation);
            }
        }
    }

    /// Discard everything buffered and wait for the renderer to start `generation`'s epoch.
    /// Returns false when a newer command superseded it or the device stopped consuming.
    fn flush(&self, generation: Generation) -> bool {
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

    fn load(&mut self, generation: Generation, path: PathBuf, start: u64) {
        let mut decoder = match Decoder::open(&path) {
            Ok(decoder) => decoder,
            Err(error) => {
                let error = PlaybackError::Decode(error.to_string());
                return self.report(generation, WorkerReport::Failed(error));
            }
        };
        let info = decoder.info().clone();
        let channels = usize::from(info.format.channels.get());
        if channels > self.output_channels {
            let error = PlaybackError::ChannelLayout {
                source_channels: channels,
                output_channels: self.output_channels,
            };
            return self.report(generation, WorkerReport::Failed(error));
        }
        let source_rate = info.format.rate.hz();
        let converter = if source_rate == self.output_rate {
            None
        } else {
            match RateConverter::new(source_rate, self.output_rate, channels) {
                Ok(converter) => Some(converter),
                Err(error) => {
                    let error = PlaybackError::Resampler(error);
                    return self.report(generation, WorkerReport::Failed(error));
                }
            }
        };
        self.report(generation, WorkerReport::Loaded(Box::new(info)));
        if start > 0 {
            match decoder.seek(start) {
                Ok(frame) => self.report(generation, WorkerReport::Seeked { frame }),
                Err(error) => {
                    let error = PlaybackError::Decode(error.to_string());
                    return self.report(generation, WorkerReport::Failed(error));
                }
            }
        }
        self.track = Some(Track {
            generation,
            decoder,
            converter,
            channels,
            decoded: Vec::new(),
            converted: Vec::new(),
            pending: Vec::new(),
            pending_at: 0,
            pushed_frames: 0,
            primed: false,
            decoder_done: false,
            finished: false,
        });
    }

    /// Decode or push one unit of work. Returns false when the ring buffer is full.
    fn step(&mut self) -> bool {
        let output_channels = self.output_channels;
        let Some(track) = &mut self.track else {
            return true;
        };
        if !self.generations.is_current(track.generation) {
            // A newer command is on its way. Stop writing and let it decide what happens to
            // the track; a seek still needs it.
            return false;
        }

        if track.pending_at == track.pending.len() {
            track.pending.clear();
            track.pending_at = 0;
            if track.decoder_done {
                self.shared
                    .source_end
                    .store(track.generation.get(), Ordering::Release);
                track.finished = true;
                if !track.primed {
                    track.primed = true;
                    let generation = track.generation;
                    self.report(generation, WorkerReport::Primed);
                }
                return true;
            }
            track.decoded.clear();
            match track.decoder.next_block(&mut track.decoded) {
                Ok(frames) => {
                    let block = if let Some(converter) = &mut track.converter {
                        track.converted.clear();
                        if frames == 0 {
                            converter.finish(&mut track.converted);
                        } else {
                            converter.process(&track.decoded, &mut track.converted);
                        }
                        &track.converted
                    } else {
                        &track.decoded
                    };
                    map_channels(block, track.channels, output_channels, &mut track.pending);
                    track.decoder_done = frames == 0;
                }
                Err(error) => {
                    let generation = track.generation;
                    self.track = None;
                    let error = PlaybackError::Decode(error.to_string());
                    self.last_error = Some(error.clone());
                    self.report(generation, WorkerReport::Failed(error));
                    return true;
                }
            }
            return true;
        }

        let space = self.producer.slots() / output_channels * output_channels;
        let count = space.min(track.pending.len() - track.pending_at);
        if count == 0 {
            return false;
        }
        let chunk = self
            .producer
            .write_chunk_uninit(count)
            .expect("slots were checked");
        chunk.fill_from_iter(
            track.pending[track.pending_at..track.pending_at + count]
                .iter()
                .copied(),
        );
        track.pending_at += count;
        track.pushed_frames += count / output_channels;
        if !track.primed && track.pushed_frames >= self.prime_frames {
            track.primed = true;
            let generation = track.generation;
            self.report(generation, WorkerReport::Primed);
        }
        true
    }
}

/// Copy interleaved source frames into the output layout. Source channels go to the first output
/// channels in order and any further output channels are silent. Mono goes to both front channels.
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
