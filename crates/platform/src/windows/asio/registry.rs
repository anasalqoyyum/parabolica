use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use parabolica_domain::output::AsioDriver;
use winreg::RegKey;
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

pub fn enumerate() -> Result<Vec<AsioDriver>, String> {
    let machine = RegKey::predef(HKEY_LOCAL_MACHINE);
    let mut drivers = Vec::new();
    for (view, fallback) in [(KEY_WOW64_64KEY, "64-bit"), (KEY_WOW64_32KEY, "x86")] {
        let asio = match machine.open_subkey_with_flags("SOFTWARE\\ASIO", KEY_READ | view) {
            Ok(key) => key,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("ASIO registry {fallback}: {error}")),
        };
        for name in asio.enum_keys() {
            let name = name.map_err(|error| error.to_string())?;
            let key = asio.open_subkey(&name).map_err(|error| error.to_string())?;
            let id: String = key
                .get_value("CLSID")
                .map_err(|error| format!("{name} CLSID: {error}"))?;
            let Some(id) = canonical_id(&id) else {
                continue;
            };
            let path: String = machine
                .open_subkey_with_flags(
                    format!("SOFTWARE\\Classes\\CLSID\\{id}\\InprocServer32"),
                    KEY_READ | view,
                )
                .and_then(|key| key.get_value(""))
                .unwrap_or_default();
            let path = expand_path(path.trim_matches('"'));
            let architecture = pe_machine(Path::new(&path));
            let compatible = architecture.as_deref() == Some(std::env::consts::ARCH);
            let reason = (!compatible).then(|| match &architecture {
                Some(architecture) => format!(
                    "{architecture} driver cannot load in this {} app",
                    std::env::consts::ARCH
                ),
                None => "Driver DLL is missing or its architecture cannot be read".to_owned(),
            });
            drivers.push(AsioDriver {
                stable_id: id,
                name: key.get_value("Description").unwrap_or(name),
                architecture: architecture
                    .unwrap_or_else(|| format!("Unknown ({fallback} registration)")),
                compatible,
                library_path: path,
                unavailable_reason: reason,
            });
        }
    }
    drivers.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.architecture.cmp(&b.architecture))
    });
    drivers.dedup_by(|a, b| a.stable_id == b.stable_id && a.architecture == b.architecture);
    Ok(drivers)
}

fn canonical_id(value: &str) -> Option<String> {
    let hex = value.trim().trim_matches(['{', '}']).replace('-', "");
    if hex.len() != 32 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let hex = hex.to_ascii_lowercase();
    Some(format!(
        "{{{}-{}-{}-{}-{}}}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

fn expand_path(value: &str) -> String {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('%') {
        result.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('%') else {
            result.push_str(&rest[start..]);
            return result;
        };
        result.push_str(
            &std::env::var(&tail[..end]).unwrap_or_else(|_| format!("%{}%", &tail[..end])),
        );
        rest = &tail[end + 1..];
    }
    result.push_str(rest);
    result
}

fn pe_machine(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let offset = u32::from_le_bytes(dos[60..64].try_into().ok()?);
    if offset > 1024 * 1024 {
        return None;
    }
    file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe).ok()?;
    if &pe[..4] != b"PE\0\0" {
        return None;
    }
    Some(
        match u16::from_le_bytes([pe[4], pe[5]]) {
            0x8664 => "x86_64",
            0x014c => "x86",
            0xaa64 => "aarch64",
            _ => return None,
        }
        .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clsids_are_canonical_and_not_endpoint_ids() {
        assert_eq!(
            canonical_id("{6B3BA606-8664-4426-8994-0F1E6FE6199F}").unwrap(),
            "{6b3ba606-8664-4426-8994-0f1e6fe6199f}"
        );
        assert!(canonical_id("{0.0.0.00000000}.{79d5e961-7833-43c0-b061-a8e716780e11}").is_none());
    }
}
