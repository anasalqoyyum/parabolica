use parabolica_domain::format::SampleRate;
use parabolica_domain::output::{OutputBackend, OutputDeviceId, OutputSelection, SampleRatePolicy};
use parabolica_storage::preferences::Preference;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedOutput(pub OutputSelection);

impl Preference for SavedOutput {
    const KEY: &'static str = "audio.output";

    fn encode(&self) -> String {
        let backend = match self.0.backend {
            OutputBackend::WasapiShared => "shared",
            OutputBackend::WasapiExclusive => "exclusive",
            OutputBackend::Asio => "asio",
            _ => "unavailable",
        };
        let rate = match self.0.rate {
            SampleRatePolicy::FollowSource => "source".to_owned(),
            SampleRatePolicy::Fixed(rate) => rate.hz().to_string(),
        };
        format!(
            "v1\n{backend}\n{rate}\n{}",
            self.0
                .device
                .as_ref()
                .map_or("", |id| id.stable_id.as_str())
        )
    }

    fn decode(stored: &str) -> Option<Self> {
        let mut fields = stored.splitn(4, '\n');
        if fields.next()? != "v1" {
            return None;
        }
        let backend = match fields.next()? {
            "shared" => OutputBackend::WasapiShared,
            "exclusive" => OutputBackend::WasapiExclusive,
            "asio" => OutputBackend::Asio,
            _ => return None,
        };
        let rate = match fields.next()? {
            "source" => SampleRatePolicy::FollowSource,
            hz => SampleRatePolicy::Fixed(SampleRate::new(hz.parse().ok()?)?),
        };
        let id = fields.next()?;
        if id.contains('\0') {
            return None;
        }
        Some(Self(OutputSelection {
            backend,
            rate,
            device: (!id.is_empty()).then(|| OutputDeviceId {
                backend,
                stable_id: id.to_owned(),
            }),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_survives_storage_reopen_and_keeps_driver_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("output.db");
        let saved = SavedOutput(OutputSelection {
            backend: OutputBackend::Asio,
            device: Some(OutputDeviceId {
                backend: OutputBackend::Asio,
                stable_id: "{driver-clsid}\n日本語".into(),
            }),
            rate: SampleRatePolicy::Fixed(SampleRate::new(96_000).unwrap()),
        });
        let storage = parabolica_storage::Storage::open(&path).unwrap();
        storage.save_preference(&saved).unwrap();
        drop(storage);
        let storage = parabolica_storage::Storage::open(&path).unwrap();
        assert_eq!(storage.preference::<SavedOutput>().unwrap(), Some(saved));
        for text in [
            "v1\nasio\n0\nid",
            "v1\nother\nsource\nid",
            "v2\nshared\nsource\nid",
            "v1\nshared\nsource\nx\0",
        ] {
            assert!(SavedOutput::decode(text).is_none());
        }
    }
}
