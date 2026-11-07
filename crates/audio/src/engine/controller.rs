//! The playback controller thread: owns transport state, the output stream and the worker.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::transitions::{PlaybackSequence, SequenceError};
use parabolica_domain::control::{Generation, GenerationCounter, SnapshotSequencer, Tagged};
use parabolica_domain::format::{OutputFormat, PcmFormat, PcmSample, SampleRate};
use parabolica_domain::gain::{GainSelection, Volume};
use parabolica_domain::ids::QueueEntryId;
use parabolica_domain::output::{NegotiatedOutput, PlaybackRoute};
use parabolica_domain::playback::{PlaybackSnapshot, PositionStamp, TransportState};
use parabolica_domain::queue::RepeatMode;
use parabolica_domain::signal_path::{ProcessingState, SignalPath};
use parabolica_domain::source::{FrameSpan, ResolvedSource};

use super::output::{Output, OutputError, OutputStream};
use super::render::{StreamRenderer, StreamShared};
use super::worker::{WorkerCommand, WorkerHandle, WorkerReport};
use super::{EngineEvent, EngineSnapshot, PlaybackError, PlaybackFailure};
use crate::decode::{SourceInfo, frames_to_duration};

/// Ring buffer length between the decoder worker and the device callback.
const RING_SECONDS: f64 = 0.5;
const TICK: Duration = Duration::from_millis(20);
const POSITION_INTERVAL: Duration = Duration::from_millis(500);
/// Close the device stream after this long without playing. An open shared-mode stream keeps an
/// active Windows audio session, which can keep the system awake.
const IDLE_CLOSE: Duration = if cfg!(test) {
    Duration::from_secs(1)
} else {
    Duration::from_secs(15)
};

#[derive(Debug)]
pub(crate) enum Command {
    Play {
        entry: QueueEntryId,
        source: ResolvedSource,
    },
    StartSequence {
        sequence: PlaybackSequence,
        index: usize,
        position: Duration,
        paused: bool,
    },
    UpdateSequence(PlaybackSequence),
    Pause,
    Resume,
    Stop,
    Seek {
        generation: Option<Generation>,
        entry: QueueEntryId,
        position: Duration,
    },
    SetVolume(Volume),
}

#[derive(Debug)]
pub(crate) enum Message {
    Command(Command),
    Worker(Tagged<WorkerReport>),
    Shutdown,
}

/// Handle to the playback controller. Commands never block the caller; state arrives on the
/// event receiver returned by [`PlaybackEngine::start`].
#[derive(Debug)]
pub struct PlaybackEngine {
    inbox: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl PlaybackEngine {
    pub fn start(output: Box<dyn Output>) -> (Self, Receiver<EngineEvent>) {
        let (inbox, messages) = std::sync::mpsc::channel();
        let (events, receiver) = std::sync::mpsc::channel();
        let controller = Controller::new(output, inbox.clone(), events);
        let thread = std::thread::Builder::new()
            .name("parabolica-playback".to_owned())
            .spawn(move || controller.run(messages))
            .expect("the playback thread starts");
        (
            Self {
                inbox,
                thread: Some(thread),
            },
            receiver,
        )
    }

    fn send(&self, command: Command) {
        let _ = self.inbox.send(Message::Command(command));
    }

    /// Load `source` as `entry` and play it from the start.
    pub fn play(&self, entry: QueueEntryId, source: ResolvedSource) {
        self.send(Command::Play { entry, source });
    }

    /// Start a deterministic sequence at `selected`. A restored session passes `paused = true`.
    /// Shuffle order must already be resolved by the caller. IDs must be unique in the sequence.
    pub fn start_sequence(
        &self,
        sequence: PlaybackSequence,
        selected: QueueEntryId,
        position: Duration,
        paused: bool,
    ) -> Result<(), SequenceError> {
        let index = sequence.index(selected)?;
        self.send(Command::StartSequence {
            sequence,
            index,
            position,
            paused,
        });
        Ok(())
    }

    /// Replace the order and repeat policy, keeping the current position and pause state.
    /// Buffered upcoming audio is cancelled and rebuffered in a new generation. If the current
    /// entry was removed, its source finishes as a detached entry before the new sequence begins.
    pub fn update_sequence(&self, sequence: PlaybackSequence) {
        self.send(Command::UpdateSequence(sequence));
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Resume after a pause, or play the current entry again after it ended or failed.
    pub fn resume(&self) {
        self.send(Command::Resume);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Seek within `entry`. Ignored if another entry has been selected since.
    pub fn seek(&self, entry: QueueEntryId, position: Duration) {
        self.send(Command::Seek {
            generation: None,
            entry,
            position,
        });
    }

    /// A delayed seek may also name the snapshot generation it was requested from.
    pub fn seek_in_generation(
        &self,
        generation: Generation,
        entry: QueueEntryId,
        position: Duration,
    ) {
        self.send(Command::Seek {
            generation: Some(generation),
            entry,
            position,
        });
    }

    pub fn set_volume(&self, volume: Volume) {
        self.send(Command::SetVolume(volume));
    }
}

impl Drop for PlaybackEngine {
    fn drop(&mut self) {
        let _ = self.inbox.send(Message::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct ActiveStream {
    // Field order matters: the worker stops before the device stream is torn down.
    worker: WorkerHandle,
    stream: Box<dyn OutputStream>,
    shared: Arc<StreamShared>,
    negotiated: NegotiatedOutput,
}

struct Current {
    entry: QueueEntryId,
    source: ResolvedSource,
    path: PathBuf,
    info: Option<SourceInfo>,
    /// Source frame at which the current epoch started.
    base_frame: u64,
    primed: bool,
    ended: bool,
    output_start: u64,
    requested_position: Duration,
}

struct Boundary {
    output_frame: u64,
    index: usize,
    start: u64,
    info: Result<Box<SourceInfo>, PlaybackError>,
}

struct Controller {
    output: Box<dyn Output>,
    stream: Option<ActiveStream>,
    generations: Arc<GenerationCounter>,
    sequencer: SnapshotSequencer,
    inbox: Sender<Message>,
    events: Sender<EngineEvent>,
    transport: TransportState,
    want_playing: bool,
    current: Option<Current>,
    sequence: Option<PlaybackSequence>,
    boundaries: VecDeque<Boundary>,
    announce_start: bool,
    sequence_end: Option<u64>,
    volume: Volume,
    failure: Option<PlaybackFailure>,
    last_underruns: u64,
    last_position: Instant,
    last_position_frame: Option<u64>,
    idle_since: Option<Instant>,
}

impl Controller {
    fn new(output: Box<dyn Output>, inbox: Sender<Message>, events: Sender<EngineEvent>) -> Self {
        Self {
            output,
            stream: None,
            generations: Arc::new(GenerationCounter::default()),
            sequencer: SnapshotSequencer::default(),
            inbox,
            events,
            transport: TransportState::Stopped,
            want_playing: false,
            current: None,
            sequence: None,
            boundaries: VecDeque::new(),
            announce_start: false,
            sequence_end: None,
            volume: Volume::UNITY,
            failure: None,
            last_underruns: 0,
            last_position: Instant::now(),
            last_position_frame: None,
            idle_since: None,
        }
    }

    fn run(mut self, messages: Receiver<Message>) {
        self.publish();
        loop {
            match messages.recv_timeout(TICK) {
                Ok(Message::Command(command)) => self.command(command),
                Ok(Message::Worker(report)) => {
                    if let Some(report) = report.accept(&self.generations) {
                        self.worker(report);
                    }
                }
                Ok(Message::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
        self.generations.advance();
        self.boundaries.clear();
        self.stream = None;
    }

    fn command(&mut self, command: Command) {
        self.apply_boundaries();
        match command {
            Command::Play { entry, source } => {
                let sequence = PlaybackSequence::new(vec![(entry, source)], RepeatMode::Off)
                    .expect("one entry is a valid sequence");
                self.load_sequence(sequence, 0, Duration::ZERO, false, true);
            }
            Command::StartSequence {
                sequence,
                index,
                position,
                paused,
            } => {
                self.load_sequence(sequence, index, position, paused, true);
            }
            Command::UpdateSequence(sequence) => self.update_sequence(sequence),
            Command::Pause => {
                self.want_playing = false;
                if matches!(
                    self.transport,
                    TransportState::Playing | TransportState::Loading
                ) {
                    self.set_playing(false);
                    self.transport = TransportState::Paused;
                    self.publish();
                }
            }
            Command::Resume => self.resume(),
            Command::Stop => {
                self.failure = None;
                self.stop_playback();
                self.publish();
            }
            Command::Seek {
                generation,
                entry,
                position,
            } => {
                if generation.is_none_or(|generation| self.generations.is_current(generation)) {
                    self.seek(entry, position);
                }
            }
            Command::SetVolume(volume) => {
                self.volume = volume;
                if let Some(stream) = &self.stream {
                    stream
                        .shared
                        .volume
                        .store(volume.get().to_bits(), Ordering::Release);
                }
                self.publish();
            }
        }
    }

    /// Stop and forget the current entry. The worker flushes anything already buffered.
    fn stop_playback(&mut self) {
        let generation = self.generations.advance();
        self.set_playing(false);
        if let Some(stream) = &self.stream {
            stream.worker.send(WorkerCommand::Stop { generation });
        }
        self.current = None;
        self.sequence = None;
        self.boundaries.clear();
        self.want_playing = false;
        self.transport = TransportState::Stopped;
    }

    fn load_sequence(
        &mut self,
        sequence: PlaybackSequence,
        index: usize,
        position: Duration,
        paused: bool,
        announce_start: bool,
    ) {
        self.failure = None;
        let (entry, source) = sequence.entries[index].clone();
        let path = match &source {
            ResolvedSource::Local {
                path,
                span: FrameSpan::Whole,
                ..
            } => path.clone(),
            ResolvedSource::Local { path, .. } => {
                let path = path.clone();
                self.stop_playback();
                return self.fail(Some(entry), Some(path), PlaybackError::CueRange);
            }
            _ => {
                self.stop_playback();
                return self.fail(Some(entry), None, PlaybackError::NotLocal);
            }
        };
        if let Err(error) = self.ensure_stream() {
            self.current = None;
            return self.fail(Some(entry), Some(path), PlaybackError::Output(error));
        }
        let generation = self.generations.advance();
        self.set_playing(false);
        self.boundaries.clear();
        self.sequence_end = None;
        self.announce_start = announce_start;
        self.sequence = Some(sequence.clone());
        self.current = Some(Current {
            entry,
            source,
            path,
            info: None,
            base_frame: 0,
            primed: false,
            ended: false,
            output_start: 0,
            requested_position: position,
        });
        self.want_playing = !paused;
        self.transport = if paused {
            TransportState::Paused
        } else {
            TransportState::Loading
        };
        if let Some(stream) = &self.stream {
            stream.worker.send(WorkerCommand::Load {
                generation,
                sequence,
                index,
                position,
            });
        }
        self.publish();
    }

    fn update_sequence(&mut self, mut sequence: PlaybackSequence) {
        let Some(current) = &self.current else {
            return;
        };
        if sequence.index(current.entry).is_err() {
            sequence.prepend_detached(current.entry, current.source.clone());
        }
        let index = sequence
            .index(current.entry)
            .expect("current entry is retained");
        let position = self.position().map_or(current.requested_position, |stamp| {
            frames_to_duration(stamp.frame, stamp.rate)
        });
        let paused = !self.want_playing;
        self.load_sequence(sequence, index, position, paused, false);
    }

    fn resume(&mut self) {
        let Some(current) = &self.current else {
            return;
        };
        if matches!(
            self.transport,
            TransportState::Loading | TransportState::Playing
        ) {
            return;
        }
        if self.transport == TransportState::Paused
            && self.failure.is_none()
            && self.stream.is_some()
        {
            self.want_playing = true;
            if current.primed {
                self.set_playing(true);
                self.transport = TransportState::Playing;
            } else {
                self.transport = TransportState::Loading;
            }
            self.publish();
            return;
        }
        let Some(sequence) = self.sequence.clone() else {
            return;
        };
        let index = sequence
            .index(current.entry)
            .expect("current entry belongs to the sequence");
        let position = if current.ended {
            Duration::ZERO
        } else {
            current
                .info
                .as_ref()
                .map_or(current.requested_position, |info| {
                    frames_to_duration(current.base_frame, info.format.rate)
                })
        };
        self.load_sequence(sequence, index, position, false, false);
    }

    fn seek(&mut self, entry: QueueEntryId, position: Duration) {
        let Some(current) = &mut self.current else {
            return;
        };
        if current.entry != entry {
            return;
        }
        let Some(sequence) = self.sequence.clone() else {
            return;
        };
        let index = sequence
            .index(entry)
            .expect("current entry belongs to the sequence");
        let paused = !self.want_playing;
        self.load_sequence(sequence, index, position, paused, false);
    }

    fn worker(&mut self, report: WorkerReport) {
        if self.current.is_none() {
            return;
        }
        match report {
            WorkerReport::Boundary {
                output_frame,
                index,
                start,
                info,
            } => {
                self.boundaries.push_back(Boundary {
                    output_frame,
                    index,
                    start,
                    info,
                });
                self.apply_boundaries();
            }
            WorkerReport::Finished { output_frame } => {
                self.sequence_end = Some(output_frame);
            }
            WorkerReport::Primed => {
                if self.failure.is_some() {
                    return;
                }
                if let Some(current) = &mut self.current {
                    current.primed = true;
                }
                if self.want_playing {
                    self.set_playing(true);
                    self.transport = TransportState::Playing;
                } else {
                    self.transport = TransportState::Paused;
                }
                self.publish();
            }
            WorkerReport::Failed(error) => {
                if let Some(current) = &mut self.current {
                    current.primed = false;
                    if let (PlaybackError::Output(_), Some(stream), Some(info)) =
                        (&error, &self.stream, &current.info)
                    {
                        current.base_frame += convert_frames(
                            stream
                                .shared
                                .played_in(self.generations.current().get())
                                .saturating_sub(current.output_start),
                            stream.negotiated.format.rate,
                            info.format.rate,
                        );
                    } else {
                        current.base_frame = 0;
                    }
                }
                self.boundaries.clear();
                self.generations.advance();
                self.set_playing(false);
                self.want_playing = false;
                let current = self.current.as_ref().expect("current was checked");
                self.fail(Some(current.entry), Some(current.path.clone()), error);
            }
        }
    }

    fn apply_boundaries(&mut self) {
        let Some(stream) = &self.stream else {
            return;
        };
        let generation = self.generations.current();
        let played = stream.shared.played_in(generation.get());
        let mut changed = false;
        while self
            .boundaries
            .front()
            .is_some_and(|boundary| boundary.output_frame <= played)
        {
            let initial = self
                .current
                .as_ref()
                .is_none_or(|current| current.info.is_none());
            if !initial && played == 0 && self.transport != TransportState::Playing {
                break;
            }
            let boundary = self.boundaries.pop_front().expect("front was checked");
            let Some(sequence) = &self.sequence else {
                break;
            };
            let (entry, source) = sequence.entries[boundary.index].clone();
            let path = match &source {
                ResolvedSource::Local { path, .. } => path.clone(),
                _ => PathBuf::new(),
            };
            let info = match boundary.info {
                Ok(info) => *info,
                Err(error) => {
                    self.set_playing(false);
                    self.want_playing = false;
                    self.boundaries.clear();
                    if let Some(current) = &mut self.current {
                        current.primed = false;
                    }
                    let _ = self.events.send(EngineEvent::EntryBoundary {
                        generation,
                        from: self.current.as_ref().map(|current| current.entry),
                        to: None,
                        output_frame: boundary.output_frame,
                    });
                    self.fail(
                        Some(entry),
                        (!path.as_os_str().is_empty()).then_some(path),
                        error,
                    );
                    return;
                }
            };
            let previous = self
                .current
                .as_ref()
                .filter(|_| !initial)
                .map(|current| current.entry);
            if let Some(previous_info) = self
                .current
                .as_ref()
                .and_then(|current| current.info.as_ref())
                && !previous_info.continuous_with(&info)
            {
                let _ = self.events.send(EngineEvent::SourceFormatChanged {
                    generation,
                    entry,
                    output_frame: boundary.output_frame,
                    previous_layout: previous_info.channel_layout.clone(),
                    next_layout: info.channel_layout.clone(),
                    previous: previous_info.format.clone(),
                    next: info.format.clone(),
                });
            }
            let primed = self.current.as_ref().is_some_and(|current| current.primed);
            let info_rate = info.format.rate;
            self.current = Some(Current {
                entry,
                source,
                path,
                info: Some(info),
                base_frame: boundary.start,
                primed,
                ended: false,
                output_start: boundary.output_frame,
                requested_position: frames_to_duration(boundary.start, info_rate),
            });
            if !initial || self.announce_start {
                let _ = self.events.send(EngineEvent::EntryBoundary {
                    generation,
                    from: previous,
                    to: Some(entry),
                    output_frame: boundary.output_frame,
                });
            }
            self.announce_start = false;
            changed = true;
        }
        if changed {
            self.publish();
        }
    }

    fn tick(&mut self) {
        self.apply_boundaries();
        let Some(stream) = &self.stream else {
            return;
        };
        if let Some(error) = stream.stream.failure() {
            let device = stream.negotiated.device.clone();
            let entry = self.current.as_ref().map(|current| current.entry);
            let path = self.current.as_ref().map(|current| current.path.clone());
            self.close_stream();
            self.want_playing = false;
            let _ = self.events.send(EngineEvent::OutputLost { device });
            self.failure = Some(PlaybackFailure {
                entry,
                path,
                error: PlaybackError::Output(error),
            });
            self.transport = TransportState::Blocked;
            self.publish();
            return;
        }

        let generation = self.generations.current().get();
        let drained = stream.shared.drained.load(Ordering::Acquire) == generation;
        if let Some(current) = &mut self.current
            && self.transport == TransportState::Playing
            && current.primed
            && self
                .sequence_end
                .is_some_and(|end| stream.shared.played_in(generation) >= end)
            && drained
        {
            current.ended = true;
            let _ = self.events.send(EngineEvent::EntryBoundary {
                generation: self.generations.current(),
                from: Some(current.entry),
                to: None,
                output_frame: stream.shared.played_in(generation),
            });
            self.want_playing = false;
            stream.shared.playing.store(false, Ordering::Release);
            self.transport = TransportState::Stopped;
            self.publish();
            return;
        }

        let idle = matches!(
            self.transport,
            TransportState::Paused | TransportState::Stopped
        );
        if !idle {
            self.idle_since = None;
        } else if self.idle_since.get_or_insert_with(Instant::now).elapsed() >= IDLE_CLOSE {
            self.close_stream();
            self.publish();
            return;
        }

        let underruns = stream.shared.underruns.total();
        if underruns != self.last_underruns {
            self.last_underruns = underruns;
            let _ = self
                .events
                .send(EngineEvent::Underruns { total: underruns });
        }
        let Some(position) = self.position() else {
            return;
        };
        // While playing, refresh the stamp periodically to correct drift. Otherwise send it when
        // it changes, for example when a pause fade-out finishes.
        let due = if position.advancing {
            self.last_position.elapsed() >= POSITION_INTERVAL
        } else {
            self.last_position_frame != Some(position.frame)
        };
        if due {
            self.last_position = Instant::now();
            self.last_position_frame = Some(position.frame);
            let _ = self.events.send(EngineEvent::Position(position));
        }
    }

    /// Close the device stream, keeping the current position as the source frame to resume from.
    fn close_stream(&mut self) {
        let generation = self.generations.current().get();
        if let (Some(current), Some(stream)) = (&mut self.current, &self.stream)
            && let Some(info) = &current.info
        {
            let played = convert_frames(
                stream
                    .shared
                    .played_in(generation)
                    .saturating_sub(current.output_start),
                stream.negotiated.format.rate,
                info.format.rate,
            );
            current.base_frame += played;
            if let Some(total) = info.frames {
                current.base_frame = current.base_frame.min(total);
            }
            current.primed = false;
        }
        self.generations.advance();
        self.boundaries.clear();
        self.stream = None;
        self.idle_since = None;
        self.set_playing(false);
    }

    fn ensure_stream(&mut self) -> Result<(), OutputError> {
        if self.stream.is_some() {
            return Ok(());
        }
        let mut stream = self.output.open()?;
        let negotiated = stream.negotiated().clone();
        let OutputFormat { rate, channels, .. } = negotiated.format;
        let channels = usize::from(channels.get());
        let ring_frames = (RING_SECONDS * f64::from(rate.hz())) as usize;
        let (producer, consumer) = rtrb::RingBuffer::<PcmSample>::new(ring_frames * channels);
        let shared = Arc::new(StreamShared::new(self.volume.get()));
        let renderer = StreamRenderer::new(consumer, shared.clone(), channels, rate.hz());
        stream.start(renderer)?;
        let worker = WorkerHandle::spawn(
            producer,
            shared.clone(),
            rate.hz(),
            channels,
            self.generations.clone(),
            self.inbox.clone(),
        );
        self.last_underruns = 0;
        self.stream = Some(ActiveStream {
            worker,
            stream,
            shared,
            negotiated,
        });
        Ok(())
    }

    fn set_playing(&self, playing: bool) {
        if let Some(stream) = &self.stream {
            stream.shared.playing.store(playing, Ordering::Release);
        }
    }

    fn fail(&mut self, entry: Option<QueueEntryId>, path: Option<PathBuf>, error: PlaybackError) {
        self.transport = match error {
            PlaybackError::Output(_) => TransportState::Blocked,
            _ => TransportState::Stopped,
        };
        self.failure = Some(PlaybackFailure { entry, path, error });
        self.publish();
    }

    /// Position on the output clock, so the UI can extrapolate it without drift.
    fn position(&self) -> Option<PositionStamp> {
        let current = self.current.as_ref()?;
        let info = current.info.as_ref()?;
        let source_rate = info.format.rate;
        let (rate, played) = match &self.stream {
            Some(stream) => (
                stream.negotiated.format.rate,
                stream
                    .shared
                    .played_in(self.generations.current().get())
                    .saturating_sub(current.output_start),
            ),
            None => (source_rate, 0),
        };
        let base = convert_frames(current.base_frame, source_rate, rate);
        let mut frame = base + played;
        if let Some(total) = info.frames {
            frame = frame.min(convert_frames(total, source_rate, rate));
        }
        Some(PositionStamp {
            frame,
            rate,
            captured_at: Instant::now(),
            advancing: self.transport == TransportState::Playing,
        })
    }

    fn publish(&mut self) {
        let route = self.route();
        let source_format = self
            .current
            .as_ref()
            .and_then(|current| current.info.as_ref())
            .map(|info| &info.format);
        let processing = ProcessingState {
            volume: self.volume,
            ..ProcessingState::TRANSPARENT
        };
        let signal_path = route
            .as_ref()
            .map(|route| SignalPath::describe(route, source_format, &processing));
        let playback = PlaybackSnapshot {
            generation: self.generations.current(),
            transport: self.transport,
            current: self.current.as_ref().map(|current| current.entry),
            position: self.position(),
            duration: self
                .current
                .as_ref()
                .and_then(|current| current.info.as_ref())
                .and_then(SourceInfo::duration),
            route,
            signal_path,
            gain: self.current.as_ref().map(|_| GainSelection::Bypassed),
            volume: self.volume,
        };
        let snapshot = EngineSnapshot {
            playback,
            path: self.current.as_ref().map(|current| current.path.clone()),
            source: self
                .current
                .as_ref()
                .and_then(|current| current.info.clone()),
            output: self.stream.as_ref().map(|stream| stream.negotiated.clone()),
            underruns: self.last_underruns,
            failure: self.failure.clone(),
        };
        self.last_position = Instant::now();
        self.last_position_frame = snapshot.playback.position.map(|position| position.frame);
        let _ = self.events.send(EngineEvent::Snapshot(Box::new(
            self.sequencer.wrap(snapshot),
        )));
    }

    fn route(&self) -> Option<PlaybackRoute> {
        let current = self.current.as_ref()?;
        let info = current.info.as_ref()?;
        let stream = self.stream.as_ref()?;
        let pcm = PcmFormat {
            rate: info.format.rate,
            channels: info.format.channels,
        };
        let mut output = stream.negotiated.clone();
        output.channel_map = (0..info.format.channels.get()).collect();
        PlaybackRoute::native(current.source.clone(), pcm, output).ok()
    }
}

fn convert_frames(frames: u64, from: SampleRate, to: SampleRate) -> u64 {
    if from == to {
        return frames;
    }
    let converted = u128::from(frames) * u128::from(to.hz()) / u128::from(from.hz());
    u64::try_from(converted).unwrap_or(u64::MAX)
}

impl std::fmt::Debug for Controller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Controller")
            .field("transport", &self.transport)
            .finish_non_exhaustive()
    }
}
