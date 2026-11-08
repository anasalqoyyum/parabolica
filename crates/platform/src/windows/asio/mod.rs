//! Registry identities and native ASIO driver access. WASAPI endpoint IDs never enter this module.

mod registry;
pub use registry::enumerate;

#[cfg(feature = "asio")]
mod native;
#[cfg(feature = "asio")]
pub use native::{BufferInfo, Callbacks, Driver, DriverError};

use parabolica_domain::format::OutputSampleFormat;

pub fn sample_format(sample_type: i32) -> Option<OutputSampleFormat> {
    use OutputSampleFormat::{Float, Int};
    Some(match sample_type {
        0 | 16 => Int {
            container_bits: 16,
            valid_bits: 16,
        },
        1 | 17 => Int {
            container_bits: 24,
            valid_bits: 24,
        },
        2 | 18 => Int {
            container_bits: 32,
            valid_bits: 32,
        },
        3 | 19 => Float { bits: 32 },
        4 | 20 => Float { bits: 64 },
        8 | 24 => Int {
            container_bits: 32,
            valid_bits: 16,
        },
        9 | 25 => Int {
            container_bits: 32,
            valid_bits: 18,
        },
        10 | 26 => Int {
            container_bits: 32,
            valid_bits: 20,
        },
        11 | 27 => Int {
            container_bits: 32,
            valid_bits: 24,
        },
        _ => return None,
    })
}
