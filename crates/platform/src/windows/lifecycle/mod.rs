//! Single instance, open-with forwarding and the app identity Windows shows for media controls and
//! notifications. Sleep, resume and session end reach the app through GPUI.

mod single_instance;
#[cfg(test)]
mod tests;

use std::io;

pub use single_instance::{
    Claim, InstanceServer, MAX_REQUEST_BYTES, RunningInstance, ServerHandle, claim,
};

/// The pipe name for the current user's instance: `Parabolica-<user SID>`.
pub fn instance_name() -> io::Result<String> {
    Ok(format!(
        "Parabolica-{}",
        single_instance::current_user_sid()?
    ))
}

/// Sets the process AppUserModelID's display name for the current user, so the media overlay and
/// notifications name the app without a Start menu shortcut. Writes
/// `HKCU\Software\Classes\AppUserModelId\<identifier>`.
pub fn register_app_identity(identifier: &str, display_name: &str) -> io::Result<()> {
    let (key, _) = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
        .create_subkey(format!(r"Software\Classes\AppUserModelId\{identifier}"))?;
    key.set_value("DisplayName", &display_name)
}
