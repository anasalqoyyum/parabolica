use parabolica_domain::format::OutputSampleFormat;
use parabolica_platform::windows::asio::sample_format;

#[derive(Debug, Clone, Copy)]
pub(super) struct SampleType {
    pub raw: i32,
    pub format: OutputSampleFormat,
    pub bytes: usize,
}

impl SampleType {
    pub fn new(raw: i32) -> Option<Self> {
        let format = sample_format(raw)?;
        let bits = match format {
            OutputSampleFormat::Int { container_bits, .. } => container_bits,
            OutputSampleFormat::Float { bits } => bits,
        };
        Some(Self {
            raw,
            format,
            bytes: usize::from(bits / 8),
        })
    }

    pub fn write(self, sample: f64, destination: &mut [u8]) {
        let big_endian = self.raw < 16;
        match self.format {
            OutputSampleFormat::Float { bits: 64 } => {
                destination.copy_from_slice(&if big_endian {
                    sample.to_be_bytes()
                } else {
                    sample.to_le_bytes()
                });
            }
            OutputSampleFormat::Float { .. } => {
                let sample = sample as f32;
                destination.copy_from_slice(&if big_endian {
                    sample.to_be_bytes()
                } else {
                    sample.to_le_bytes()
                });
            }
            OutputSampleFormat::Int { valid_bits, .. } => {
                let scale = (1u64 << (valid_bits - 1)) as f64;
                let value = (sample * scale).round().clamp(-scale, scale - 1.) as i32;
                // ASIO's reduced-precision Int32 types are sign-extended, right-aligned values.
                let bytes = if big_endian {
                    value.to_be_bytes()
                } else {
                    value.to_le_bytes()
                };
                let start = if big_endian { 4 - self.bytes } else { 0 };
                destination.copy_from_slice(&bytes[start..start + self.bytes]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_samples_preserve_endianness_valid_bits_and_signed_limits() {
        for raw in [
            0, 1, 2, 3, 4, 8, 9, 10, 11, 16, 17, 18, 19, 20, 24, 25, 26, 27,
        ] {
            let sample = SampleType::new(raw).unwrap();
            let mut bytes = [0; 8];
            for value in [-1., -0.5, 0., 0.5] {
                sample.write(value, &mut bytes[..sample.bytes]);
                let actual = match sample.format {
                    OutputSampleFormat::Float { bits: 64 } => {
                        if raw < 16 {
                            f64::from_be_bytes(bytes)
                        } else {
                            f64::from_le_bytes(bytes)
                        }
                    }
                    OutputSampleFormat::Float { .. } => {
                        let bytes: [u8; 4] = bytes[..4].try_into().unwrap();
                        f64::from(if raw < 16 {
                            f32::from_be_bytes(bytes)
                        } else {
                            f32::from_le_bytes(bytes)
                        })
                    }
                    OutputSampleFormat::Int { valid_bits, .. } => {
                        let mut integer = [if value < 0. { 255 } else { 0 }; 4];
                        if raw < 16 {
                            integer[4 - sample.bytes..].copy_from_slice(&bytes[..sample.bytes]);
                        } else {
                            integer[..sample.bytes].copy_from_slice(&bytes[..sample.bytes]);
                        }
                        let integer = if raw < 16 {
                            i32::from_be_bytes(integer)
                        } else {
                            i32::from_le_bytes(integer)
                        };
                        f64::from(integer) / (1u64 << (valid_bits - 1)) as f64
                    }
                };
                assert_eq!(value, actual, "sample type {raw}");
            }
        }
        assert!(SampleType::new(32).is_none());
        let mut packed = [0; 3];
        SampleType::new(17).unwrap().write(1., &mut packed);
        assert_eq!(packed, [255, 255, 127]);
    }
}
