//! Windows Credential Manager and `ShellExecuteW`.

#![allow(unsafe_code)]

use std::iter;
use std::slice;

use parabolica_provider_contracts::auth::AuthorizationError;
use parabolica_provider_contracts::credentials::{CredentialError, StoredCredential};
use parabolica_provider_contracts::secret::Secret;
use parabolica_provider_contracts::service::AccountIdentity;
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_MAX_STRING_LENGTH, CRED_MAX_USERNAME_LENGTH,
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

pub(super) fn read(target: &str) -> Result<Option<StoredCredential>, CredentialError> {
    let target = HSTRING::from(target);
    let mut raw: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success CredReadW stores a system-allocated CREDENTIALW in `raw`; `Allocation`
    // frees it with CredFree.
    if let Err(error) = unsafe { CredReadW(&target, CRED_TYPE_GENERIC, None, &mut raw) } {
        return if error.code() == ERROR_NOT_FOUND.to_hresult() {
            Ok(None)
        } else {
            Err(os_error("read", &error))
        };
    }
    let allocation = Allocation(raw);
    // SAFETY: `raw` is a valid CREDENTIALW until `allocation` is dropped.
    let credential = unsafe { &*allocation.0 };

    let blob: &mut [u8] = if credential.CredentialBlob.is_null() {
        &mut []
    } else {
        // SAFETY: CredentialBlob holds CredentialBlobSize bytes owned by the allocation.
        unsafe {
            slice::from_raw_parts_mut(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        }
    };
    let secret = Secret::new(blob.to_vec());
    blob.fill(0);

    // SAFETY: UserName and Comment are null or null-terminated strings in the allocation.
    let id = unsafe { text(credential.UserName) }.ok_or(CredentialError::Unreadable)?;
    let display_name = unsafe { text(credential.Comment) }
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.clone());
    Ok(Some(StoredCredential {
        account: AccountIdentity { id, display_name },
        secret,
    }))
}

pub(super) fn write(target: &str, credential: &StoredCredential) -> Result<(), CredentialError> {
    let secret = credential.secret.expose();
    let max_blob = CRED_MAX_CREDENTIAL_BLOB_SIZE as usize;
    if secret.len() > max_blob {
        return Err(CredentialError::TooLarge {
            field: "secret",
            len: secret.len(),
            max: max_blob,
        });
    }
    let mut user = wide(&credential.account.id);
    let max_user = CRED_MAX_USERNAME_LENGTH as usize;
    if user.len() - 1 > max_user {
        return Err(CredentialError::TooLarge {
            field: "account ID",
            len: user.len() - 1,
            max: max_user,
        });
    }
    let mut target = wide(target);
    let mut comment = wide(&truncate_utf16(
        &credential.account.display_name,
        CRED_MAX_STRING_LENGTH as usize,
    ));
    let raw = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        Comment: PWSTR(comment.as_mut_ptr()),
        CredentialBlobSize: secret.len() as u32,
        // CredWriteW only reads the blob.
        CredentialBlob: secret.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user.as_mut_ptr()),
        ..Default::default()
    };
    // SAFETY: every pointer in `raw` refers to a live, null-terminated (or sized) buffer for the
    // duration of the call; CredWriteW copies them.
    unsafe { CredWriteW(&raw, 0) }.map_err(|error| os_error("save", &error))
}

pub(super) fn delete(target: &str) -> Result<(), CredentialError> {
    let target = HSTRING::from(target);
    // SAFETY: `target` is a valid null-terminated string for the duration of the call.
    match unsafe { CredDeleteW(&target, CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(error) => Err(os_error("remove", &error)),
    }
}

pub(super) fn open_url(url: &str) -> Result<(), AuthorizationError> {
    let url = HSTRING::from(url);
    // SAFETY: the strings are valid and null-terminated for the duration of the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &url,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 mean success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(AuthorizationError::Browser)
    }
}

struct Allocation(*mut CREDENTIALW);

impl Drop for Allocation {
    fn drop(&mut self) {
        // SAFETY: the pointer came from CredReadW and is freed once.
        unsafe { CredFree(self.0.cast()) }
    }
}

/// # Safety
/// `value` is null or points to a null-terminated UTF-16 string.
unsafe fn text(value: PWSTR) -> Option<String> {
    if value.is_null() {
        Some(String::new())
    } else {
        unsafe { value.to_string() }.ok()
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(iter::once(0)).collect()
}

fn truncate_utf16(value: &str, max_units: usize) -> String {
    let mut units = 0;
    value
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= max_units
        })
        .collect()
}

fn os_error(operation: &'static str, error: &windows::core::Error) -> CredentialError {
    CredentialError::Os {
        operation,
        code: error.code().0 as u32,
    }
}
