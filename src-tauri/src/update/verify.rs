//! Checking a downloaded installer: its SHA-256 must match its line in
//! `SHA256SUMS.txt` and its size the size GitHub lists. This catches a damaged or incomplete
//! download. It does not protect against a compromised GitHub account (code signing would).

use std::io::Read;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("SHA256SUMS.txt has no line for {0}")]
    MissingLine(String),
    #[error("the size doesn't match ({actual} bytes, expected {expected})")]
    Size { actual: u64, expected: u64 },
    #[error("the SHA-256 doesn't match")]
    Hash,
    #[error("the file couldn't be read: {0}")]
    Io(String),
}

/// The lower-case hex SHA-256 for `file_name` in a `SHA256SUMS.txt` text: lines
/// `<64 hex>  <name>` (one or two spaces, or `*` for binary mode), LF or CRLF, any hex case.
pub fn expected_hash(sums: &str, file_name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let line = line.trim_end_matches('\r').trim();
        let (hash, rest) = line.split_once(char::is_whitespace)?;
        let name = rest.trim_start().trim_start_matches('*');
        let valid = hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit());
        (valid && name == file_name).then(|| hash.to_ascii_lowercase())
    })
}

/// Lower-case hex SHA-256 of a file.
pub fn sha256_file(path: &Path) -> Result<String, VerifyError> {
    let mut file = std::fs::File::open(path).map_err(|e| VerifyError::Io(e.to_string()))?;
    let mut hasher = Sha256::new().map_err(VerifyError::Io)?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| VerifyError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]).map_err(VerifyError::Io)?;
    }
    let digest = hasher.finish().map_err(VerifyError::Io)?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Checks size, then hash. On any mismatch the file is deleted.
pub fn verify_file(
    path: &Path,
    expected_size: u64,
    expected_hash: &str,
) -> Result<(), VerifyError> {
    let result = (|| {
        let actual = std::fs::metadata(path)
            .map_err(|e| VerifyError::Io(e.to_string()))?
            .len();
        if actual != expected_size {
            return Err(VerifyError::Size {
                actual,
                expected: expected_size,
            });
        }
        if sha256_file(path)? != expected_hash.to_ascii_lowercase() {
            return Err(VerifyError::Hash);
        }
        Ok(())
    })();
    if matches!(result, Err(VerifyError::Size { .. } | VerifyError::Hash)) {
        let _ = std::fs::remove_file(path);
    }
    result
}

#[cfg(windows)]
struct Sha256(windows::Win32::Security::Cryptography::BCRYPT_HASH_HANDLE);

#[cfg(windows)]
impl Sha256 {
    fn new() -> Result<Self, String> {
        use windows::Win32::Security::Cryptography::*;
        let mut handle = BCRYPT_HASH_HANDLE::default();
        // SAFETY: the SHA-256 pseudo-handle needs no provider; `handle` receives the hash object.
        let status =
            unsafe { BCryptCreateHash(BCRYPT_SHA256_ALG_HANDLE, &mut handle, None, None, 0) };
        status
            .ok()
            .map(|_| Self(handle))
            .map_err(|e| format!("SHA-256 unavailable: {e}"))
    }

    fn update(&mut self, data: &[u8]) -> Result<(), String> {
        use windows::Win32::Security::Cryptography::BCryptHashData;
        // SAFETY: `self.0` is a live hash object; `data` is a valid slice.
        unsafe { BCryptHashData(self.0, data, 0) }
            .ok()
            .map_err(|e| e.to_string())
    }

    fn finish(self) -> Result<[u8; 32], String> {
        use windows::Win32::Security::Cryptography::BCryptFinishHash;
        let mut out = [0u8; 32];
        // SAFETY: SHA-256 output is 32 bytes; the object is destroyed by Drop.
        unsafe { BCryptFinishHash(self.0, &mut out, 0) }
            .ok()
            .map(|_| out)
            .map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
impl Drop for Sha256 {
    fn drop(&mut self) {
        // SAFETY: destroyed once.
        unsafe {
            let _ = windows::Win32::Security::Cryptography::BCryptDestroyHash(self.0);
        }
    }
}

#[cfg(not(windows))]
struct Sha256;
#[cfg(not(windows))]
impl Sha256 {
    fn new() -> Result<Self, String> {
        Err("SHA-256 is only available on Windows".into())
    }
    fn update(&mut self, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn finish(self) -> Result<[u8; 32], String> {
        Ok([0; 32])
    }
}
