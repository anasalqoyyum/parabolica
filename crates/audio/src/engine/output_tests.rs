use super::*;
use parabolica_domain::format::SourceFormat;
use parabolica_domain::output::{OutputSelection, SampleRatePolicy};

type Requests = Arc<Mutex<Vec<(OutputSelection, SourceFormat)>>>;

struct SelectedCapture {
    capture: Capture,
    requests: Requests,
    rates: Vec<u32>,
}

struct SelectedStream {
    inner: Box<dyn OutputStream>,
    negotiated: NegotiatedOutput,
}
impl OutputStream for SelectedStream {
    fn negotiated(&self) -> &NegotiatedOutput {
        &self.negotiated
    }
    fn start(&mut self, renderer: StreamRenderer) -> Result<(), OutputError> {
        self.inner.start(renderer)
    }
    fn failure(&self) -> Option<OutputError> {
        self.inner.failure()
    }
}
impl Output for SelectedCapture {
    fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
        unreachable!()
    }
    fn open_for(
        &mut self,
        selection: &OutputSelection,
        source: &SourceFormat,
    ) -> Result<Box<dyn OutputStream>, OutputError> {
        self.requests
            .lock()
            .unwrap()
            .push((selection.clone(), source.clone()));
        self.capture.rate = match (selection.backend, selection.rate) {
            (OutputBackend::WasapiShared, _) => 48_000,
            (_, SampleRatePolicy::FollowSource) => source.rate.hz(),
            (_, SampleRatePolicy::Fixed(rate)) => rate.hz(),
        };
        if !self.rates.contains(&self.capture.rate) {
            return Err(OutputError::UnsupportedFormat(
                "test unsupported rate".into(),
            ));
        }
        let inner = self.capture.open()?;
        let mut negotiated = inner.negotiated().clone();
        negotiated.device = selection.device.clone().unwrap_or(OutputDeviceId {
            backend: selection.backend,
            stable_id: "resolved-default".into(),
        });
        negotiated.device.backend = selection.backend;
        Ok(Box::new(SelectedStream { inner, negotiated }))
    }
}

fn selected(rates: Vec<u32>) -> (Harness, Requests) {
    let mut harness = Harness::new(48_000);
    let requests = Requests::default();
    let output = SelectedCapture {
        capture: harness.capture.clone(),
        requests: requests.clone(),
        rates,
    };
    let (engine, events) = PlaybackEngine::start(Box::new(output));
    harness.engine = engine;
    harness.events = events;
    (harness, requests)
}
fn exclusive(rate: SampleRatePolicy) -> OutputSelection {
    OutputSelection {
        backend: OutputBackend::WasapiExclusive,
        rate,
        device: Some(OutputDeviceId {
            backend: OutputBackend::WasapiExclusive,
            stable_id: "headphones".into(),
        }),
    }
}

#[test]
fn exclusive_rate_changes_reopen_and_compatible_entries_keep_the_stream() {
    let (mut harness, requests) = selected(vec![44_100, 48_000]);
    harness
        .engine
        .select_output(exclusive(SampleRatePolicy::FollowSource));
    let paths = [
        fixture("flac/s16_44100.flac"),
        fixture("flac/s24_44100.flac"),
        fixture("flac/s24_48000.flac"),
    ];
    let (sequence, ids) = super::sequence_tests::sequence(&paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let ended = harness.wait("last rate ended", |state| {
        state.playback.current == Some(ids[2])
            && state.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none(), "{:?}", ended.failure);
    let opens = requests.lock().unwrap();
    assert_eq!(
        opens
            .iter()
            .map(|(_, source)| source.rate.hz())
            .collect::<Vec<_>>(),
        [44_100, 48_000]
    );
    assert!(
        opens
            .iter()
            .all(|(selection, _)| selection.device.as_ref().unwrap().stable_id == "headphones")
    );
    assert_eq!(ended.output.unwrap().format.rate.hz(), 48_000);
    assert!(harness.format_changes.contains(&ids[2]));
}

#[test]
fn fixed_rate_converts_multiple_sources_without_reopening() {
    let (mut harness, requests) = selected(vec![48_000]);
    harness
        .engine
        .select_output(exclusive(SampleRatePolicy::Fixed(
            SampleRate::new(48_000).unwrap(),
        )));
    let paths = [
        fixture("flac/s24_44100.flac"),
        fixture("flac/s24_48000.flac"),
    ];
    let (sequence, ids) = super::sequence_tests::sequence(&paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let ended = harness.wait("fixed ended", |state| {
        state.playback.current == Some(ids[1])
            && state.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none());
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(ended.output.unwrap().format.rate.hz(), 48_000);
}

#[test]
fn busy_or_unsupported_selection_retains_entry_and_requires_explicit_alternative() {
    for error in [
        OutputError::Busy,
        OutputError::UnsupportedFormat("not supported".into()),
    ] {
        let (mut harness, requests) = selected(vec![44_100, 48_000]);
        *harness.capture.open_error.lock().unwrap() = Some(error.clone());
        harness
            .engine
            .select_output(exclusive(SampleRatePolicy::FollowSource));
        let id = harness.play(&fixture("flac/s24_44100.flac"));
        let blocked = harness.wait_transport(TransportState::Blocked);
        assert_eq!(blocked.playback.current, Some(id));
        assert!(blocked.output.is_none());
        assert_eq!(blocked.failure.unwrap().error, PlaybackError::Output(error));
        harness.drain_for(Duration::from_millis(100));
        assert_eq!(requests.lock().unwrap().len(), 1);
        *harness.capture.open_error.lock().unwrap() = None;
        let mut shared = exclusive(SampleRatePolicy::FollowSource);
        shared.backend = OutputBackend::WasapiShared;
        shared.device.as_mut().unwrap().backend = OutputBackend::WasapiShared;
        harness.engine.select_output(shared);
        let paused = harness.wait("alternative paused", |state| {
            state.playback.transport == TransportState::Paused && state.failure.is_none()
        });
        assert!(paused.output.is_none());
        harness.engine.resume();
        let playing = harness.wait_transport(TransportState::Playing);
        let output = playing.output.unwrap();
        assert_eq!(output.device.stable_id, "headphones");
        assert_eq!(output.device.backend, OutputBackend::WasapiShared);
        assert_eq!(output.format.rate.hz(), 48_000);
    }
}

#[test]
fn device_loss_retains_position_and_default_identity_until_explicit_retry() {
    let (mut harness, requests) = selected(vec![48_000]);
    let id = harness.play(&fixture("flac/s24_48000.flac"));
    harness.wait_transport(TransportState::Playing);
    harness.drain_for(Duration::from_millis(40));
    *harness.capture.lost.lock().unwrap() = Some(OutputError::DeviceLost);
    let blocked = harness.wait_transport(TransportState::Blocked);
    let position = blocked.playback.position.unwrap().frame;
    assert!(position > 0);
    assert_eq!(blocked.playback.current, Some(id));
    assert!(blocked.output.is_none());
    harness.drain_for(Duration::from_millis(100));
    assert_eq!(requests.lock().unwrap().len(), 1);
    harness.engine.resume();
    let playing = harness.wait_transport(TransportState::Playing);
    assert!(playing.playback.position.unwrap().frame >= position);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].0.device.as_ref().unwrap().stable_id,
        "resolved-default"
    );
}

#[test]
fn unsupported_next_rate_blocks_at_the_next_entry_without_playing_it_at_the_old_rate() {
    let (mut harness, requests) = selected(vec![44_100]);
    harness
        .engine
        .select_output(exclusive(SampleRatePolicy::FollowSource));
    let paths = [
        fixture("flac/s24_44100.flac"),
        fixture("flac/s24_96000.flac"),
    ];
    let (sequence, ids) = super::sequence_tests::sequence(&paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let blocked = harness.wait_transport(TransportState::Blocked);
    assert_eq!(blocked.playback.current, Some(ids[1]));
    assert!(blocked.output.is_none());
    assert_eq!(blocked.playback.position.unwrap().frame, 0);
    assert_eq!(requests.lock().unwrap().len(), 2);
    let count = harness.capture.total_frames();
    let source = Decoder::open(&paths[0]).unwrap();
    assert_eq!(count as u64, source.info().frames.unwrap());
}

#[test]
fn exclusive_compatible_short_entries_deliver_exact_pcm_on_one_stream() {
    let directory = tempfile::tempdir().unwrap();
    let lengths = [1, 7, 255, 44_100, 1, 4_709];
    let expected: Vec<i16> = (0..lengths.iter().sum::<usize>() * 2)
        .map(|index| (index * 37 % 20_000) as i16 - 10_000)
        .collect();
    let mut offset = 0;
    let paths: Vec<_> = lengths
        .iter()
        .enumerate()
        .map(|(index, frames)| {
            let path = directory.path().join(format!("{index}.wav"));
            let end = offset + frames * 2;
            super::sequence_tests::write_wav(&path, 44_100, 2, &expected[offset..end]);
            offset = end;
            path
        })
        .collect();
    let (mut harness, requests) = selected(vec![44_100]);
    harness
        .engine
        .select_output(exclusive(SampleRatePolicy::FollowSource));
    let (sequence, ids) = super::sequence_tests::sequence(&paths, RepeatMode::Off);
    harness
        .engine
        .start_sequence(sequence, ids[0], Duration::ZERO, false)
        .unwrap();
    let ended = harness.wait("short Exclusive sequence ended", |state| {
        state.playback.current == ids.last().copied()
            && state.playback.transport == TransportState::Stopped
    });
    assert!(ended.failure.is_none());
    assert_eq!(ended.underruns, 0);
    assert_eq!(requests.lock().unwrap().len(), 1);
    let pcm: Vec<f32> = expected
        .iter()
        .map(|sample| f32::from(*sample) / 32768.)
        .collect();
    assert_eq!(harness.capture.epoch(ended.playback.generation.get()), pcm);
}

#[test]
fn initial_exclusive_load_rejects_a_file_replaced_during_output_negotiation() {
    struct ReplacingOutput {
        inner: SelectedCapture,
        path: PathBuf,
        replacement: Vec<u8>,
    }
    impl Output for ReplacingOutput {
        fn open(&mut self) -> Result<Box<dyn OutputStream>, OutputError> {
            unreachable!()
        }
        fn open_for(
            &mut self,
            selection: &OutputSelection,
            source: &SourceFormat,
        ) -> Result<Box<dyn OutputStream>, OutputError> {
            let inner = self.inner.open_for(selection, source)?;
            let mut negotiated = inner.negotiated().clone();
            negotiated.format.sample = OutputSampleFormat::Int {
                container_bits: 16,
                valid_bits: 16,
            };
            std::fs::write(&self.path, &self.replacement).unwrap();
            Ok(Box::new(SelectedStream { inner, negotiated }))
        }
    }
    for replacement in ["flac/s24_48000.flac", "flac/s24_44100.flac"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("replaced.flac");
        std::fs::copy(fixture("flac/s16_44100.flac"), &path).unwrap();
        let mut harness = Harness::new(44_100);
        let output = ReplacingOutput {
            inner: SelectedCapture {
                capture: harness.capture.clone(),
                requests: Requests::default(),
                rates: vec![44_100],
            },
            path: path.clone(),
            replacement: std::fs::read(fixture(replacement)).unwrap(),
        };
        let (engine, events) = PlaybackEngine::start(Box::new(output));
        harness.engine = engine;
        harness.events = events;
        harness
            .engine
            .select_output(exclusive(SampleRatePolicy::FollowSource));
        harness.play(&path);
        let state = harness.wait("worker accepted or rejected the replaced source", |state| {
            matches!(
                state.playback.transport,
                TransportState::Playing | TransportState::Blocked
            )
        });
        assert_eq!(
            state.playback.transport,
            TransportState::Blocked,
            "replacement {replacement}"
        );
        assert!(state.output.is_none());
        assert!(
            state
                .failure
                .unwrap()
                .error
                .to_string()
                .contains("changed while opening")
        );
        assert_eq!(harness.capture.total_frames(), 0);
    }
}
