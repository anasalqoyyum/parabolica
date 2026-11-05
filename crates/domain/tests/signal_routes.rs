use std::time::Duration;

use parabolica_domain::format::{
    ChannelCount, Codec, DeliveryFormat, OutputFormat, OutputSampleFormat, PcmFormat, SampleRate,
    SourceFormat, SourceSampleFormat, int_to_pcm, pcm_to_int_exact,
};
use parabolica_domain::gain::{AppliedGain, Decibels, Volume};
use parabolica_domain::ids::MediaFileId;
use parabolica_domain::output::{
    BufferConfig, BufferOwner, NegotiatedOutput, OutputBackend, OutputDeviceId, PlaybackRoute,
    RouteError,
};
use parabolica_domain::provider::{
    Availability, Capability, PlaybackOwner, ProviderCapabilities, ProviderId, ProviderItemId,
    ProviderItemRef, UnavailableReason,
};
use parabolica_domain::signal_path::{
    ProcessingState, Provenance, SignalPath, StageKind, StageStatus,
};
use parabolica_domain::source::ResolvedSource;

fn rate(hz: u32) -> SampleRate {
    SampleRate::new(hz).unwrap()
}

fn flac_24_96() -> SourceFormat {
    SourceFormat {
        codec: Codec::Flac,
        sample: Some(SourceSampleFormat::Int { bits: 24 }),
        rate: rate(96_000),
        channels: ChannelCount::STEREO,
    }
}

fn pcm(hz: u32) -> PcmFormat {
    PcmFormat {
        rate: rate(hz),
        channels: ChannelCount::STEREO,
    }
}

fn output(backend: OutputBackend, sample: OutputSampleFormat, hz: u32) -> NegotiatedOutput {
    NegotiatedOutput {
        device: OutputDeviceId {
            backend,
            stable_id: "device".into(),
        },
        device_name: "USB DAC".into(),
        format: OutputFormat {
            sample,
            rate: rate(hz),
            channels: ChannelCount::STEREO,
        },
        channel_map: vec![0, 1],
        buffer: BufferConfig {
            frames: 512,
            owner: BufferOwner::Driver,
        },
        reported_latency: None,
    }
}

fn local() -> ResolvedSource {
    ResolvedSource::local_file(MediaFileId::generate(), "track.flac".into())
}

fn youtube_item() -> ProviderItemRef {
    ProviderItemRef {
        provider: ProviderId::YouTubeMusic,
        item: ProviderItemId("abc".into()),
    }
}

const ASIO_INT32_24: OutputSampleFormat = OutputSampleFormat::Int {
    container_bits: 32,
    valid_bits: 24,
};
const FLOAT32: OutputSampleFormat = OutputSampleFormat::Float { bits: 32 };

fn status(path: &SignalPath, kind: StageKind) -> StageStatus {
    path.stage(kind).unwrap().status
}

#[test]
fn internal_pcm_holds_every_supported_integer_sample_exactly() {
    for bits in [16u8, 24, 32] {
        let min = -(1i64 << (bits - 1)) as i32;
        let max = ((1i64 << (bits - 1)) - 1) as i32;
        for sample in [min, min + 1, -1, 0, 1, max - 1, max] {
            assert_eq!(
                pcm_to_int_exact(int_to_pcm(sample, bits), bits),
                Some(sample),
                "{bits}-bit {sample}"
            );
        }
    }
    for sample in -(1 << 23)..(1 << 23) {
        assert_eq!(pcm_to_int_exact(int_to_pcm(sample, 24), 24), Some(sample));
    }
}

#[test]
fn source_pcm_and_output_formats_stay_separate() {
    let route = PlaybackRoute::native(
        local(),
        pcm(96_000),
        output(OutputBackend::WasapiShared, FLOAT32, 48_000),
    )
    .unwrap();
    let PlaybackRoute::Native {
        pcm: internal,
        output: negotiated,
        ..
    } = &route
    else {
        unreachable!()
    };

    assert_eq!(flac_24_96().rate, internal.rate);
    assert_ne!(internal.rate, negotiated.format.rate);
    assert!(route.requires_resampling());
    let path = SignalPath::describe(&route, Some(&flac_24_96()), &ProcessingState::TRANSPARENT);
    assert_eq!(status(&path, StageKind::Resampling), StageStatus::Active);
    assert_eq!(status(&path, StageKind::SystemMixer), StageStatus::Active);
    assert_eq!(
        path.stage(StageKind::SystemMixer).unwrap().provenance,
        Provenance::Unverified
    );
}

#[test]
fn asio_bypasses_the_windows_mixer_and_session_volume() {
    let asio = output(OutputBackend::Asio, ASIO_INT32_24, 96_000);
    let route = PlaybackRoute::native(local(), pcm(96_000), asio).unwrap();
    let path = SignalPath::describe(&route, Some(&flac_24_96()), &ProcessingState::TRANSPARENT);

    assert_eq!(status(&path, StageKind::SystemMixer), StageStatus::Bypassed);
    assert_eq!(
        status(&path, StageKind::SessionVolume),
        StageStatus::NotInPath
    );
    assert_eq!(
        path.stage(StageKind::AppOutput).unwrap().provenance,
        Provenance::Driver
    );
    assert_eq!(
        path.stage(StageKind::Device).unwrap().provenance,
        Provenance::Unverified
    );
    assert!(!path.app_changes_samples());
}

#[test]
fn asio_alone_does_not_mean_unmodified_samples() {
    let album_gain = ProcessingState {
        gain: AppliedGain {
            requested: Decibels::new(-7.4).unwrap(),
            applied: Decibels::new(-7.4).unwrap(),
        },
        ..ProcessingState::TRANSPARENT
    };
    let asio_route = |hz| {
        PlaybackRoute::native(
            local(),
            pcm(96_000),
            output(OutputBackend::Asio, ASIO_INT32_24, hz),
        )
        .unwrap()
    };

    let with_gain = SignalPath::describe(&asio_route(96_000), Some(&flac_24_96()), &album_gain);
    assert_eq!(
        status(&with_gain, StageKind::Processing),
        StageStatus::Active
    );
    assert!(with_gain.app_changes_samples());

    let resampled = SignalPath::describe(
        &asio_route(48_000),
        Some(&flac_24_96()),
        &ProcessingState::TRANSPARENT,
    );
    assert!(resampled.app_changes_samples());

    let quieter = ProcessingState {
        volume: Volume::new(0.5).unwrap(),
        ..ProcessingState::TRANSPARENT
    };
    assert!(
        SignalPath::describe(&asio_route(96_000), Some(&flac_24_96()), &quieter)
            .app_changes_samples()
    );
}

#[test]
fn reducing_integer_precision_is_a_visible_conversion() {
    let int16 = OutputSampleFormat::Int {
        container_bits: 16,
        valid_bits: 16,
    };
    let route = PlaybackRoute::native(
        local(),
        pcm(96_000),
        output(OutputBackend::WasapiExclusive, int16, 96_000),
    )
    .unwrap();
    let path = SignalPath::describe(&route, Some(&flac_24_96()), &ProcessingState::TRANSPARENT);

    assert_eq!(status(&path, StageKind::Conversion), StageStatus::Active);
    assert_eq!(status(&path, StageKind::SystemMixer), StageStatus::Bypassed);
}

#[test]
fn browser_owned_playback_fits_the_route_model_without_native_controls() {
    let mut capabilities = ProviderCapabilities::new(ProviderId::YouTubeMusic);
    capabilities.set(Capability::Playback, Availability::Available);
    assert_eq!(
        capabilities.playback_owner(),
        PlaybackOwner::Provider(ProviderId::YouTubeMusic)
    );

    let source = ResolvedSource::ProviderManaged {
        item: youtube_item(),
        delivery: DeliveryFormat {
            codec: Some(Codec::Opus),
            ..DeliveryFormat::default()
        },
    };
    assert_eq!(
        PlaybackRoute::native(
            source.clone(),
            pcm(48_000),
            output(OutputBackend::Asio, ASIO_INT32_24, 48_000)
        ),
        Err(RouteError::ProviderOwnsPlayback)
    );

    let route = PlaybackRoute::provider_managed(source).unwrap();
    assert!(!route.native_controls_apply());
    let path = SignalPath::describe(&route, None, &ProcessingState::TRANSPARENT);
    let managed = StageStatus::ManagedBy(ProviderId::YouTubeMusic);
    assert_eq!(status(&path, StageKind::Processing), StageStatus::NotInPath);
    assert_eq!(status(&path, StageKind::AppOutput), managed);
    assert_eq!(
        path.stage(StageKind::AppOutput).unwrap().provenance,
        Provenance::Unavailable
    );
    assert!(!path.app_changes_samples());
}

#[test]
fn provider_media_with_app_owned_pcm_uses_the_native_engine() {
    let mut capabilities = ProviderCapabilities::new(ProviderId::YouTubeMusic);
    capabilities.set(Capability::AppOwnedPcm, Availability::Available);
    assert_eq!(capabilities.playback_owner(), PlaybackOwner::NativeEngine);

    let source = ResolvedSource::ProviderPcm {
        item: youtube_item(),
        delivery: DeliveryFormat::default(),
        expires_at: None,
    };
    let route = PlaybackRoute::native(
        source,
        pcm(48_000),
        output(OutputBackend::WasapiShared, FLOAT32, 48_000),
    )
    .unwrap();
    assert!(route.native_controls_apply());
    assert_eq!(
        PlaybackRoute::provider_managed(local()),
        Err(RouteError::NotProviderManaged)
    );
}

#[test]
fn unreported_capabilities_are_unsupported() {
    let mut capabilities = ProviderCapabilities::new(ProviderId::Discogs);
    capabilities.set(
        Capability::ReleaseLookup,
        Availability::Unavailable(UnavailableReason::RateLimited {
            retry_after: Some(Duration::from_secs(60)),
        }),
    );
    assert!(!capabilities.is_available(Capability::ReleaseLookup));
    assert_eq!(
        capabilities.availability(Capability::Seek),
        Availability::Unavailable(UnavailableReason::NotSupported)
    );
    assert_eq!(
        ProviderId::from_key(ProviderId::YouTubeMusic.key()),
        Some(ProviderId::YouTubeMusic)
    );
}

#[test]
fn channel_map_must_cover_internal_channels() {
    let mut mono_map = output(OutputBackend::Asio, ASIO_INT32_24, 96_000);
    mono_map.channel_map = vec![2];
    assert_eq!(
        PlaybackRoute::native(local(), pcm(96_000), mono_map),
        Err(RouteError::ChannelMapMismatch { mapped: 1, pcm: 2 })
    );
}

#[test]
fn buffer_duration_is_derived_from_frames_and_rate() {
    let asio = output(OutputBackend::Asio, ASIO_INT32_24, 48_000);
    assert_eq!(
        asio.buffer_duration(),
        Duration::from_secs_f64(512.0 / 48_000.0)
    );
    assert_eq!(asio.reported_latency, None);
}

#[test]
fn float_sources_are_quantized_by_integer_output() {
    let float_wav = SourceFormat {
        codec: Codec::Wav,
        sample: Some(SourceSampleFormat::Float { bits: 32 }),
        ..flac_24_96()
    };
    let opus = SourceFormat {
        codec: Codec::Opus,
        sample: None,
        ..flac_24_96()
    };
    let conversion = |source: &SourceFormat, sample| {
        let route = PlaybackRoute::native(
            local(),
            pcm(96_000),
            output(OutputBackend::Asio, sample, 96_000),
        )
        .unwrap();
        status(
            &SignalPath::describe(&route, Some(source), &ProcessingState::TRANSPARENT),
            StageKind::Conversion,
        )
    };

    assert_eq!(conversion(&float_wav, ASIO_INT32_24), StageStatus::Active);
    assert_eq!(conversion(&float_wav, FLOAT32), StageStatus::Bypassed);
    assert_eq!(conversion(&opus, ASIO_INT32_24), StageStatus::Active);
    assert_eq!(conversion(&opus, FLOAT32), StageStatus::Bypassed);
    assert_eq!(conversion(&flac_24_96(), FLOAT32), StageStatus::Bypassed);
}

#[test]
fn unknown_source_format_never_reports_conversion_bypassed() {
    let route = PlaybackRoute::native(
        local(),
        pcm(96_000),
        output(OutputBackend::Asio, ASIO_INT32_24, 96_000),
    )
    .unwrap();
    let path = SignalPath::describe(&route, None, &ProcessingState::TRANSPARENT);
    assert_eq!(status(&path, StageKind::Conversion), StageStatus::Active);
}
