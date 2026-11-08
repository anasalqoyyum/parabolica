//! Explicit native driver inspection. Does not start audio or change the sample rate.
#[cfg(all(windows, feature = "asio"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use parabolica_domain::output::{OutputBackend, OutputDeviceId};
    use parabolica_platform::windows::asio::{Driver, enumerate};
    println!("Drivers: {:#?}", enumerate()?);
    if let Some(id) = std::env::args().nth(1) {
        let driver = Driver::load(&OutputDeviceId {
            backend: OutputBackend::Asio,
            stable_id: id,
        })?;
        println!("Capabilities: {:#?}", driver.capabilities()?);
    }
    Ok(())
}
#[cfg(not(all(windows, feature = "asio")))]
fn main() {
    eprintln!("Use Windows MSVC and --features asio to inspect native ASIO drivers.");
}
