//! The app's only network access: HTTPS GET requests through WinHTTP, the Windows HTTP
//! stack (system TLS and proxy settings), to an allow-list of GitHub hosts.
//!
//! - `api.github.com`, under `/repos/MathisVkg/cargoline/` only;
//! - `github.com`, under `/MathisVkg/cargoline/releases/download/` only;
//! - the release-asset hosts that github.com redirects downloads to ([`ASSET_HOSTS`]).
//!
//! HTTPS only. Redirects are followed by hand (at most [`MAX_REDIRECTS`]) and every target is
//! checked again. The test feed (`CARGO_LINE_UPDATE_FEED=http://127.0.0.1:<port>`) additionally
//! allows that one loopback address, and only while the variable is set.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Url;

/// The releases list. `/releases/latest` would skip pre-releases.
pub const API_URL: &str = "https://api.github.com/repos/MathisVkg/cargoline/releases?per_page=20";
/// Test feed override: replaces [`API_URL`]; only `http://127.0.0.1:<port>` is accepted.
pub const FEED_ENV: &str = "CARGO_LINE_UPDATE_FEED";
const API_PREFIX: &str = "/repos/MathisVkg/cargoline/";
const DOWNLOAD_PREFIX: &str = "/MathisVkg/cargoline/releases/download/";
/// Where github.com redirects release downloads (GitHub's current download hosts).
pub const ASSET_HOSTS: &[&str] = &[
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];
pub const MAX_REDIRECTS: usize = 5;
const CONNECT_TIMEOUT_MS: i32 = 10_000;
const TRANSFER_TIMEOUT_MS: i32 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("address not allowed: {0}")]
    Refused(String),
    #[error("GitHub is busy, try later")]
    Busy,
    #[error("GitHub answered {0}")]
    Status(u32),
    #[error("the answer is too large")]
    TooLarge,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Connection(String),
}

/// A URL that passed the allow-list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub secure: bool,
    pub host: String,
    pub port: u16,
    /// Path and query, as sent in the request line.
    pub target: String,
}

/// The test feed URL, if `value` is exactly `http://127.0.0.1:<port>` (with an optional path).
/// Anything else is ignored.
pub fn test_feed(value: Option<&str>) -> Option<Url> {
    let url = Url::parse(value?.trim()).ok()?;
    let ok = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.username().is_empty()
        && url.password().is_none();
    ok.then_some(url)
}

/// The feed of this process: the test feed if `CARGO_LINE_UPDATE_FEED` is valid.
pub fn feed_override() -> Option<Url> {
    test_feed(std::env::var(FEED_ENV).ok().as_deref())
}

/// Checks `url` against the allow-list. `feed` is the active test feed, if any.
pub fn check_url(url: &str, feed: Option<&Url>) -> Result<Endpoint, NetError> {
    let refused = || NetError::Refused(crate::update::feed::short(url));
    let parsed = Url::parse(url).map_err(|_| refused())?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(refused());
    }
    let host = parsed.host_str().ok_or_else(refused)?.to_ascii_lowercase();
    let target = match parsed.query() {
        Some(q) => format!("{}?{}", parsed.path(), q),
        None => parsed.path().to_string(),
    };
    if let Some(feed) = feed {
        if parsed.scheme() == "http"
            && host == "127.0.0.1"
            && parsed.port().is_some()
            && parsed.port() == feed.port()
        {
            return Ok(Endpoint {
                secure: false,
                host,
                port: parsed.port().unwrap_or(80),
                target,
            });
        }
    }
    if parsed.scheme() != "https" || parsed.port().is_some_and(|p| p != 443) {
        return Err(refused());
    }
    let allowed = match host.as_str() {
        "api.github.com" => parsed.path().starts_with(API_PREFIX),
        "github.com" => parsed.path().starts_with(DOWNLOAD_PREFIX),
        h => ASSET_HOSTS.contains(&h),
    };
    if !allowed {
        return Err(refused());
    }
    Ok(Endpoint {
        secure: true,
        host,
        port: 443,
        target,
    })
}

/// Receives the body in chunks, with the announced total size if any.
pub type Sink<'a> = dyn FnMut(&[u8], Option<u64>) -> Result<(), NetError> + 'a;

/// Options of one GET request.
pub struct Get<'a> {
    pub user_agent: &'a str,
    pub accept: &'a str,
    /// Larger bodies are refused (also when the server announces a larger Content-Length).
    pub max_bytes: u64,
    pub cancel: Option<&'a AtomicBool>,
}

/// GETs `url` (following allowed redirects) and passes the body to `sink` in chunks, with the
/// announced total size if any. Returns the number of bytes received.
pub fn get(
    url: &str,
    feed: Option<&Url>,
    opts: &Get<'_>,
    sink: &mut Sink<'_>,
) -> Result<u64, NetError> {
    let mut current = url.to_string();
    for _ in 0..=MAX_REDIRECTS {
        let endpoint = check_url(&current, feed)?;
        match platform::request(&endpoint, opts, sink)? {
            Outcome::Done(bytes) => return Ok(bytes),
            Outcome::Redirect(location) => {
                let base = Url::parse(&current).map_err(|_| NetError::Refused(current.clone()))?;
                current = base
                    .join(&location)
                    .map_err(|_| NetError::Refused(crate::update::feed::short(&location)))?
                    .to_string();
            }
        }
    }
    Err(NetError::Connection("too many redirects".into()))
}

/// GETs a small body into memory.
pub fn get_bytes(url: &str, feed: Option<&Url>, opts: &Get<'_>) -> Result<Vec<u8>, NetError> {
    let mut body = Vec::new();
    get(url, feed, opts, &mut |chunk, _| {
        body.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(body)
}

pub(crate) enum Outcome {
    Done(u64),
    Redirect(String),
}

pub(crate) fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}

/// A short text for a WinHTTP error code.
pub fn winhttp_reason(code: u32) -> String {
    match code {
        12002 => "the connection timed out".into(),
        12007 => "no internet connection (name not resolved)".into(),
        12029 | 12030 => "couldn't connect".into(),
        12175 | 12045 | 12044 => "the secure connection failed".into(),
        12017 => "cancelled".into(),
        c => format!("network error {c}"),
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::*;

    struct Handle(*mut c_void);
    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle returned by WinHttpOpen/Connect/OpenRequest, closed once.
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    fn last_error() -> NetError {
        let code = windows::core::Error::from_thread().code().0 as u32 & 0xFFFF;
        NetError::Connection(winhttp_reason(code))
    }

    fn check(handle: *mut c_void) -> Result<Handle, NetError> {
        if handle.is_null() {
            Err(last_error())
        } else {
            Ok(Handle(handle))
        }
    }

    fn query_number(request: &Handle, info: u32) -> Option<u32> {
        let mut value: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        // SAFETY: the buffer is a u32 and `len` its size.
        unsafe {
            WinHttpQueryHeaders(
                request.0,
                info | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some(&mut value as *mut u32 as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
        }
        .ok()
        .map(|_| value)
    }

    fn query_text(request: &Handle, info: u32) -> Option<String> {
        let mut buf = vec![0u16; 4096];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: `len` is the buffer size in bytes.
        unsafe {
            WinHttpQueryHeaders(
                request.0,
                info,
                PCWSTR::null(),
                Some(buf.as_mut_ptr() as *mut c_void),
                &mut len,
                std::ptr::null_mut(),
            )
        }
        .ok()?;
        Some(String::from_utf16_lossy(&buf[..(len as usize / 2)]))
    }

    pub(crate) fn request(
        endpoint: &Endpoint,
        opts: &Get<'_>,
        sink: &mut Sink<'_>,
    ) -> Result<Outcome, NetError> {
        let agent = wide(opts.user_agent);
        let host = wide(&endpoint.host);
        let target = wide(&endpoint.target);
        let verb = wide("GET");
        // SAFETY: every pointer is a NUL-terminated UTF-16 buffer that outlives the call; the
        // handles are closed by `Handle`.
        unsafe {
            let session = check(WinHttpOpen(
                PCWSTR(agent.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            ))?;
            let _ = WinHttpSetTimeouts(
                session.0,
                CONNECT_TIMEOUT_MS,
                CONNECT_TIMEOUT_MS,
                TRANSFER_TIMEOUT_MS,
                TRANSFER_TIMEOUT_MS,
            );
            let connection = check(WinHttpConnect(
                session.0,
                PCWSTR(host.as_ptr()),
                endpoint.port,
                0,
            ))?;
            let flags = if endpoint.secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            };
            let request = check(WinHttpOpenRequest(
                connection.0,
                PCWSTR(verb.as_ptr()),
                PCWSTR(target.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                std::ptr::null(),
                flags,
            ))?;
            // Redirects are followed by hand so each target passes the allow-list.
            let never = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
            WinHttpSetOption(
                Some(request.0 as *const c_void),
                WINHTTP_OPTION_REDIRECT_POLICY,
                Some(&never),
            )
            .map_err(|_| last_error())?;
            let headers: Vec<u16> = format!("Accept: {}\r\n", opts.accept)
                .encode_utf16()
                .collect();
            WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0)
                .map_err(|_| last_error())?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|_| last_error())?;

            let status = query_number(&request, WINHTTP_QUERY_STATUS_CODE).unwrap_or(0);
            match status {
                200 => {}
                301 | 302 | 303 | 307 | 308 => {
                    let location =
                        query_text(&request, WINHTTP_QUERY_LOCATION).ok_or_else(|| {
                            NetError::Connection("a redirect without a target".into())
                        })?;
                    return Ok(Outcome::Redirect(location));
                }
                403 | 429 => return Err(NetError::Busy),
                other => return Err(NetError::Status(other)),
            }
            let total = query_number(&request, WINHTTP_QUERY_CONTENT_LENGTH).map(u64::from);
            if total.is_some_and(|t| t > opts.max_bytes) {
                return Err(NetError::TooLarge);
            }
            let mut received: u64 = 0;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                if is_cancelled(opts.cancel) {
                    return Err(NetError::Cancelled);
                }
                let mut read: u32 = 0;
                WinHttpReadData(
                    request.0,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len() as u32,
                    &mut read,
                )
                .map_err(|_| last_error())?;
                if read == 0 {
                    break;
                }
                received += u64::from(read);
                if received > opts.max_bytes {
                    return Err(NetError::TooLarge);
                }
                sink(&buf[..read as usize], total)?;
            }
            Ok(Outcome::Done(received))
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;
    pub(crate) fn request(
        _endpoint: &Endpoint,
        _opts: &Get<'_>,
        _sink: &mut Sink<'_>,
    ) -> Result<Outcome, NetError> {
        Err(NetError::Connection(
            "updates are only checked on Windows".into(),
        ))
    }
}
