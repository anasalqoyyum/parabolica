//! Signal-path stages and the provenance of what each reports.
//!
//! The inspector shows these stages. Each carries where its value came from so the UI never
//! presents an assumption as a measurement.

use crate::format::{OutputSampleFormat, SourceFormat, SourceSampleFormat};
use crate::gain::{AppliedGain, Volume};
use crate::output::{OutputBackend, PlaybackRoute};
use crate::provider::ProviderId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageKind {
    Source,
    Decode,
    Processing,
    Resampling,
    Conversion,
    AppOutput,
    SystemMixer,
    SessionVolume,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageStatus {
    /// The stage processes this stream.
    Active,
    /// The stage exists in this route but passes samples through.
    Bypassed,
    /// The stage is not part of this route (for example Windows session volume under ASIO).
    NotInPath,
    ManagedBy(ProviderId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    Decoder,
    AppState,
    Backend,
    Driver,
    Provider,
    /// The app cannot inspect this value.
    Unverified,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage {
    pub kind: StageKind,
    pub status: StageStatus,
    pub provenance: Provenance,
}

/// App-side processing for the current item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessingState {
    pub gain: AppliedGain,
    pub volume: Volume,
    pub optional_dsp: bool,
}

impl ProcessingState {
    pub const TRANSPARENT: Self = Self {
        gain: AppliedGain::UNITY,
        volume: Volume::UNITY,
        optional_dsp: false,
    };

    fn changes_samples(&self) -> bool {
        self.gain.applied.get() != 0.0 || self.volume != Volume::UNITY || self.optional_dsp
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalPath {
    pub stages: Vec<Stage>,
}

impl SignalPath {
    pub fn describe(
        route: &PlaybackRoute,
        source: Option<&SourceFormat>,
        processing: &ProcessingState,
    ) -> Self {
        let stage = |kind, status, provenance| Stage {
            kind,
            status,
            provenance,
        };
        let active_if = |condition: bool| {
            if condition {
                StageStatus::Active
            } else {
                StageStatus::Bypassed
            }
        };

        let stages = match route {
            PlaybackRoute::ProviderManaged { item, .. } => {
                let managed = StageStatus::ManagedBy(item.provider);
                vec![
                    stage(StageKind::Source, StageStatus::Active, Provenance::Provider),
                    stage(StageKind::Decode, managed, Provenance::Unavailable),
                    stage(
                        StageKind::Processing,
                        StageStatus::NotInPath,
                        Provenance::AppState,
                    ),
                    stage(StageKind::Resampling, managed, Provenance::Unavailable),
                    stage(StageKind::Conversion, managed, Provenance::Unavailable),
                    stage(StageKind::AppOutput, managed, Provenance::Unavailable),
                    stage(StageKind::SystemMixer, managed, Provenance::Unverified),
                    stage(StageKind::SessionVolume, managed, Provenance::Unverified),
                    stage(
                        StageKind::Device,
                        StageStatus::Active,
                        Provenance::Unverified,
                    ),
                ]
            }
            PlaybackRoute::Native { pcm, output, .. } => {
                let shared = output.device.backend.uses_system_mixer();
                let reduces_precision = source
                    .and_then(source_precision_bits)
                    .is_some_and(|bits| output_precision_bits(output.format.sample) < bits);
                let output_provenance = if output.device.backend == OutputBackend::Asio {
                    Provenance::Driver
                } else {
                    Provenance::Backend
                };
                let source_provenance = if source.is_some() {
                    Provenance::Decoder
                } else {
                    Provenance::Unavailable
                };
                vec![
                    stage(StageKind::Source, StageStatus::Active, source_provenance),
                    stage(StageKind::Decode, StageStatus::Active, Provenance::Decoder),
                    stage(
                        StageKind::Processing,
                        active_if(processing.changes_samples()),
                        Provenance::AppState,
                    ),
                    stage(
                        StageKind::Resampling,
                        active_if(pcm.rate != output.format.rate),
                        Provenance::AppState,
                    ),
                    stage(
                        StageKind::Conversion,
                        active_if(reduces_precision),
                        Provenance::AppState,
                    ),
                    stage(StageKind::AppOutput, StageStatus::Active, output_provenance),
                    if shared {
                        stage(
                            StageKind::SystemMixer,
                            StageStatus::Active,
                            Provenance::Unverified,
                        )
                    } else {
                        stage(
                            StageKind::SystemMixer,
                            StageStatus::Bypassed,
                            Provenance::Backend,
                        )
                    },
                    if shared {
                        stage(
                            StageKind::SessionVolume,
                            StageStatus::Active,
                            Provenance::Unverified,
                        )
                    } else {
                        stage(
                            StageKind::SessionVolume,
                            StageStatus::NotInPath,
                            Provenance::Backend,
                        )
                    },
                    stage(
                        StageKind::Device,
                        StageStatus::Active,
                        Provenance::Unverified,
                    ),
                ]
            }
        };
        SignalPath { stages }
    }

    pub fn stage(&self, kind: StageKind) -> Option<&Stage> {
        self.stages.iter().find(|stage| stage.kind == kind)
    }

    /// Whether the app itself changes sample values before the backend. Device and driver
    /// processing remain unverified either way, so `false` is not a bit-perfect claim.
    pub fn app_changes_samples(&self) -> bool {
        self.stages.iter().any(|stage| {
            matches!(
                stage.kind,
                StageKind::Processing | StageKind::Resampling | StageKind::Conversion
            ) && stage.status == StageStatus::Active
        })
    }
}

fn source_precision_bits(source: &SourceFormat) -> Option<u8> {
    match source.sample? {
        SourceSampleFormat::Int { bits } => Some(bits),
        SourceSampleFormat::Float { bits: 32 } => Some(24),
        SourceSampleFormat::Float { .. } => Some(53),
    }
}

fn output_precision_bits(sample: OutputSampleFormat) -> u8 {
    match sample {
        OutputSampleFormat::Int { valid_bits, .. } => valid_bits,
        OutputSampleFormat::Float { bits: 32 } => 24,
        OutputSampleFormat::Float { .. } => 53,
    }
}
