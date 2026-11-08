#![allow(unsafe_code)]

use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use parabolica_domain::format::SampleRate;
use parabolica_domain::output::{
    AsioBufferSizes, AsioCapabilities, AsioChannel, OutputBackend, OutputDeviceId,
};

static DRIVER_IN_USE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverError {
    Busy,
    Missing,
    Native { operation: &'static str, code: i32 },
    Invalid(&'static str),
}

impl std::fmt::Display for DriverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("ASIO driver is in use by this app"),
            Self::Missing => f.write_str("selected compatible ASIO driver is missing"),
            Self::Native { operation, code } => write!(
                f,
                "ASIO {operation} failed ({code:#x}); close other audio apps, check the device, then retry"
            ),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for DriverError {}

#[repr(C)]
#[derive(Debug)]
pub struct BufferInfo {
    pub is_input: i32,
    pub channel: i32,
    pub buffers: [*mut c_void; 2],
}

#[repr(C)]
#[derive(Debug)]
pub struct Callbacks {
    pub buffer_switch: extern "C" fn(i32, i32),
    pub rate_changed: extern "C" fn(f64),
    pub message: extern "C" fn(i32, i32, *mut c_void, *mut f64) -> i32,
    pub time_info: extern "C" fn(*mut c_void, i32, i32) -> *mut c_void,
}

unsafe extern "C" {
    fn parabolica_asio_open(clsid: *const u16, driver: *mut *mut c_void) -> i32;
    fn parabolica_asio_close(driver: *mut c_void);
    fn parabolica_asio_name(driver: *mut c_void, name: *mut c_char);
    fn parabolica_asio_version(driver: *mut c_void) -> i32;
    fn parabolica_asio_channels(driver: *mut c_void, outputs: *mut i32) -> i32;
    fn parabolica_asio_channel(
        driver: *mut c_void,
        index: i32,
        sample_type: *mut i32,
        name: *mut c_char,
    ) -> i32;
    fn parabolica_asio_buffers(driver: *mut c_void, sizes: *mut i32) -> i32;
    fn parabolica_asio_can_rate(driver: *mut c_void, rate: f64) -> i32;
    fn parabolica_asio_rate(driver: *mut c_void, rate: *mut f64) -> i32;
    fn parabolica_asio_set_rate(driver: *mut c_void, rate: f64) -> i32;
    fn parabolica_asio_latency(driver: *mut c_void, frames: *mut i32) -> i32;
    fn parabolica_asio_create(
        driver: *mut c_void,
        buffers: *mut BufferInfo,
        channels: i32,
        frames: i32,
        callbacks: *mut Callbacks,
    ) -> i32;
    fn parabolica_asio_start(driver: *mut c_void) -> i32;
    fn parabolica_asio_panel(driver: *mut c_void) -> i32;
    fn parabolica_asio_pump();
}

/// Lives on one initialized STA worker. ASIO permits only one active driver per host.
#[derive(Debug)]
pub struct Driver {
    pointer: NonNull<c_void>,
    id: OutputDeviceId,
    _thread: PhantomData<Rc<()>>,
}

impl Driver {
    pub fn load(id: &OutputDeviceId) -> Result<Self, DriverError> {
        if id.backend != OutputBackend::Asio
            || !super::enumerate()
                .map_err(|_| DriverError::Missing)?
                .iter()
                .any(|driver| {
                    driver.compatible && driver.stable_id.eq_ignore_ascii_case(&id.stable_id)
                })
        {
            return Err(DriverError::Missing);
        }
        if DRIVER_IN_USE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(DriverError::Busy);
        }
        let clsid: Vec<u16> = id.stable_id.encode_utf16().chain(Some(0)).collect();
        let mut pointer = std::ptr::null_mut();
        // SAFETY: the bridge initializes COM and returns a unique IASIO owner on this thread.
        let code = unsafe { parabolica_asio_open(clsid.as_ptr(), &mut pointer) };
        if code != 0 || pointer.is_null() {
            DRIVER_IN_USE.store(false, Ordering::Release);
            return Err(DriverError::Native {
                operation: "initialization",
                code,
            });
        }
        Ok(Self {
            pointer: NonNull::new(pointer).expect("checked"),
            id: id.clone(),
            _thread: PhantomData,
        })
    }

    pub fn capabilities(&self) -> Result<AsioCapabilities, DriverError> {
        // SAFETY: the driver and fixed-size buffers belong to this worker; no callbacks use them.
        unsafe {
            let driver = self.pointer.as_ptr();
            let mut name = [0i8; 64];
            parabolica_asio_name(driver, name.as_mut_ptr());
            let mut count = 0;
            check(
                "output channels",
                parabolica_asio_channels(driver, &mut count),
            )?;
            if !(1..=256).contains(&count) {
                return Err(DriverError::Invalid(
                    "ASIO reported an invalid output count",
                ));
            }
            let mut outputs = Vec::new();
            for index in 0..count {
                let mut name = [0i8; 32];
                let mut sample_type = 0;
                check(
                    "output channel",
                    parabolica_asio_channel(driver, index, &mut sample_type, name.as_mut_ptr()),
                )?;
                outputs.push(AsioChannel {
                    index: index as u16,
                    name: text(&name),
                    sample_type,
                    sample: super::sample_format(sample_type),
                });
            }
            let mut sizes = [0; 4];
            check(
                "buffer sizes",
                parabolica_asio_buffers(driver, sizes.as_mut_ptr()),
            )?;
            if sizes[0] <= 0 || sizes[1] < sizes[0] || sizes[2] < sizes[0] || sizes[2] > sizes[1] {
                return Err(DriverError::Invalid("ASIO reported invalid buffer sizes"));
            }
            let buffer = AsioBufferSizes {
                min: sizes[0] as u32,
                max: sizes[1] as u32,
                preferred: sizes[2] as u32,
                granularity: sizes[3],
            };
            if !buffer.supports(buffer.preferred) {
                return Err(DriverError::Invalid(
                    "ASIO preferred buffer contradicts its limits",
                ));
            }
            let rates = [
                8000, 11025, 16000, 22050, 32000, 44100, 48000, 88200, 96000, 176400, 192000,
                352800, 384000, 705600, 768000,
            ]
            .into_iter()
            .filter(|rate| self.supports_rate(*rate))
            .filter_map(SampleRate::new)
            .collect();
            Ok(AsioCapabilities {
                device: self.id.clone(),
                driver_name: text(&name),
                driver_version: parabolica_asio_version(driver),
                outputs,
                rates,
                current_rate: self.rate().ok(),
                buffer,
            })
        }
    }

    pub fn supports_rate(&self, rate: u32) -> bool {
        // SAFETY: query only, on the driver owner thread.
        unsafe { parabolica_asio_can_rate(self.pointer.as_ptr(), f64::from(rate)) == 0 }
    }
    pub fn set_rate(&self, rate: SampleRate) -> Result<(), DriverError> {
        // SAFETY: called before buffer creation on the driver owner thread.
        check("sample rate", unsafe {
            parabolica_asio_set_rate(self.pointer.as_ptr(), f64::from(rate.hz()))
        })
    }
    pub fn rate(&self) -> Result<SampleRate, DriverError> {
        let mut rate = 0.;
        // SAFETY: output points to a live f64.
        check("current rate", unsafe {
            parabolica_asio_rate(self.pointer.as_ptr(), &mut rate)
        })?;
        if !rate.is_finite() || rate.fract() != 0. || rate <= 0. || rate > f64::from(u32::MAX) {
            return Err(DriverError::Invalid("ASIO reported an invalid sample rate"));
        }
        SampleRate::new(rate as u32).ok_or(DriverError::Invalid("ASIO rate is zero"))
    }
    pub fn latency_frames(&self) -> Option<u32> {
        let mut frames = 0;
        // SAFETY: output points to a live i32.
        (unsafe { parabolica_asio_latency(self.pointer.as_ptr(), &mut frames) } == 0)
            .then(|| u32::try_from(frames).ok())
            .flatten()
    }
    /// # Safety
    /// Buffer descriptors and callbacks must stay alive until this driver is dropped. Callbacks
    /// must tolerate creation-time calls, and own all memory reached by the driver thread.
    pub unsafe fn create_buffers(
        &self,
        buffers: &mut [BufferInfo],
        frames: u32,
        callbacks: &mut Callbacks,
    ) -> Result<(), DriverError> {
        // SAFETY: the caller guarantees descriptor and callback lifetimes; bounds were queried.
        check("create buffers", unsafe {
            parabolica_asio_create(
                self.pointer.as_ptr(),
                buffers.as_mut_ptr(),
                buffers.len() as i32,
                frames as i32,
                callbacks,
            )
        })
    }
    /// # Safety
    /// Callback state must be fully published and remain valid until this driver is dropped.
    pub unsafe fn start(&self) -> Result<(), DriverError> {
        // SAFETY: the caller has installed its callback state and created the buffers.
        check("start", unsafe {
            parabolica_asio_start(self.pointer.as_ptr())
        })
    }
    pub fn control_panel(&self) -> Result<(), DriverError> {
        // SAFETY: called on the STA owner without an active stream.
        check("driver panel", unsafe {
            parabolica_asio_panel(self.pointer.as_ptr())
        })
    }
    pub fn pump_messages(&self) {
        // SAFETY: pumps only this worker's Windows message queue.
        unsafe { parabolica_asio_pump() };
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // SAFETY: stops and disposes callbacks before releasing the sole interface on its STA.
        unsafe { parabolica_asio_close(self.pointer.as_ptr()) };
        DRIVER_IN_USE.store(false, Ordering::Release);
    }
}

fn check(operation: &'static str, code: i32) -> Result<(), DriverError> {
    if code == 0 {
        Ok(())
    } else {
        Err(DriverError::Native { operation, code })
    }
}
fn text(bytes: &[i8]) -> String {
    let bytes: Vec<u8> = bytes
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}
