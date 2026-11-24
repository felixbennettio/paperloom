//! Files only the current user can read: the session and the app's
//! secrets. On Windows their contents are sealed with DPAPI, so only this
//! Windows user on this computer can open them; elsewhere the file itself
//! is readable only by the user (Android keeps it in the app's own folder).

use std::path::Path;

/// Writes a file only the current user can read.
pub(crate) fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let sealed = dpapi::seal(text)
            .ok_or_else(|| std::io::Error::other("Windows could not protect the file's contents."))?;
        std::fs::write(path, sealed)
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file =
            std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        file.write_all(text.as_bytes())
    }
    #[cfg(not(any(windows, unix)))]
    {
        std::fs::write(path, text)
    }
}

/// Reads a file written by `write_private` (or, from before sealing, as
/// plain text).
pub(crate) fn read_private(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    #[cfg(windows)]
    if let Some(sealed) = text.strip_prefix(dpapi::MARK) {
        return dpapi::open(sealed);
    }
    Some(text)
}

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    /// Starts a sealed file: the rest is the sealed bytes in hex.
    pub(super) const MARK: &str = "dpapi:";

    /// Seals (or opens) bytes for the current Windows user.
    fn run(data: &[u8], seal: bool) -> Option<Vec<u8>> {
        let input =
            CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr().cast_mut() };
        let mut output = CRYPT_INTEGER_BLOB::default();
        // SAFETY: `input` points at `data` for the length of the call, and
        // DPAPI only reads it; it allocates the output, freed below.
        let done = unsafe {
            if seal {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if done == 0 || output.pbData.is_null() {
            return None;
        }
        // SAFETY: on success DPAPI hands back `cbData` bytes at `pbData`,
        // which are copied out before LocalFree releases them.
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        unsafe { LocalFree(output.pbData.cast()) };
        Some(bytes)
    }

    pub(super) fn seal(text: &str) -> Option<String> {
        let sealed = run(text.as_bytes(), true)?;
        Some(format!("{MARK}{}", sealed.iter().map(|b| format!("{b:02x}")).collect::<String>()))
    }

    pub(super) fn open(hex: &str) -> Option<String> {
        let hex = hex.trim();
        if !hex.len().is_multiple_of(2) {
            return None;
        }
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
            .collect::<Option<_>>()?;
        String::from_utf8(run(&bytes, false)?).ok()
    }
}
