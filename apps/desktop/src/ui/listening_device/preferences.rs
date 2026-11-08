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
            "v2\n{backend}\n{rate}\n{}\n{}\n{}",
            self.0
                .asio
                .channels
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(","),
            self.0
                .asio
                .buffer_frames
                .map_or_else(|| "driver".to_owned(), |frames| frames.to_string()),
            self.0
                .device
                .as_ref()
                .map_or("", |id| id.stable_id.as_str())
        )
    }

    fn decode(stored: &str) -> Option<Self> {
        let version = stored.split('\n').next()?;
        let mut fields = stored.splitn(if version == "v2" { 6 } else { 4 }, '\n');
        if !matches!(fields.next()?, "v1" | "v2") {
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
        let asio = if version == "v2" {
            let channels: Vec<u16> = fields
                .next()?
                .split(',')
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            if channels.is_empty()
                || channels.len() > 32
                || channels.iter().any(|channel| *channel >= 256)
                || channels
                    .iter()
                    .enumerate()
                    .any(|(index, channel)| channels[..index].contains(channel))
            {
                return None;
            }
            let buffer_frames = match fields.next()? {
                "driver" => None,
                frames => Some(
                    frames
                        .parse::<u32>()
                        .ok()
                        .filter(|frames| *frames > 0 && *frames <= 1_048_576)?,
                ),
            };
            parabolica_domain::output::AsioSelection {
                channels,
                buffer_frames,
            }
        } else {
            Default::default()
        };
        let id = fields.next()?;
        if id.contains('\0') {
            return None;
        }
        Some(Self(OutputSelection {
            backend,
            rate,
            asio,
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
            asio: parabolica_domain::output::AsioSelection {
                channels: vec![3, 2],
                buffer_frames: Some(512),
            },
        });
        let storage = parabolica_storage::Storage::open(&path).unwrap();
        storage.save_preference(&saved).unwrap();
        drop(storage);
        let storage = parabolica_storage::Storage::open(&path).unwrap();
        assert_eq!(storage.preference::<SavedOutput>().unwrap(), Some(saved));
        assert_eq!(
            SavedOutput::decode("v1\nasio\nsource\n{clsid}")
                .unwrap()
                .0
                .asio,
            Default::default()
        );
        for text in [
            "v1\nasio\n0\nid",
            "v1\nother\nsource\nid",
            "v3\nshared\nsource\nid",
            "v2\nasio\nsource\n0,0\ndriver\nid",
            "v2\nasio\nsource\n0,1\n0\nid",
            "v1\nshared\nsource\nx\0",
        ] {
            assert!(SavedOutput::decode(text).is_none());
        }
    }
}
