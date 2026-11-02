//! HTTPS GETs through WinHTTP, which is part of Windows and of Wine, so a
//! launcher needs no TLS stack of its own. Ported from OR's `WinHttp`
//! (its HD-pack download): ranged requests for resuming, redirects
//! followed by WinHTTP itself (GitHub release downloads go to its file
//! servers), and the body streamed in 256 KiB chunks.
//!
//! On Windows `WinHttp` implements `rea_launcher::download::Fetch`, so
//! `download::install(&pin, root, &free, &WinHttp::new(agent), &job)`
//! works as is. The URL and status handling is pure and tested on the host.

/// `https://host/path` split into host and path (`/` when empty);
/// anything but `https` is refused.
pub fn split_https(url: &str) -> Result<(&str, &str), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("not an https address: {url}"))?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    if host.is_empty() {
        return Err(format!("no host in {url}"));
    }
    Ok((host, if path.is_empty() { "/" } else { path }))
}

/// The request headers for a download resumed at byte `from`.
pub fn range_header(from: u64) -> Option<String> {
    (from > 0).then(|| format!("Range: bytes={from}-"))
}

/// Whether the body starts at `from` (true) or at byte 0 (false), from
/// the HTTP status; any other status is an error.
pub fn body_starts_at_from(status: u32, from: u64) -> Result<bool, String> {
    match status {
        206 => Ok(true),
        200 => Ok(from == 0),
        code => Err(format!("the server answered {code}")),
    }
}

/// The WinHTTP client; `agent` is the User-Agent (e.g.
/// `OutRunLauncher/1.2.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinHttp {
    pub agent: String,
}

impl WinHttp {
    pub fn new(agent: impl Into<String>) -> WinHttp {
        WinHttp {
            agent: agent.into(),
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Networking::WinHttp::{
        INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    };

    use super::WinHttp;
    use crate::wide::wide;

    /// A WinHTTP handle, closed on drop.
    struct Internet(*mut c_void);

    impl Drop for Internet {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle WinHTTP returned, closed once.
                unsafe {
                    WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn failed(what: &str) -> String {
        format!("{what} failed: {}", crate::wide::last_error())
    }

    impl WinHttp {
        /// Streams `url` from byte `from` (a range request above 0).
        /// `start` is told whether the body begins at `from` (true) or at
        /// byte 0 because the server ignored the range (false); `chunk`
        /// receives the body and returns false to stop (then the result is
        /// `Err("stopped")`).
        pub fn fetch(
            &self,
            url: &str,
            from: u64,
            start: &mut dyn FnMut(bool),
            chunk: &mut dyn FnMut(&[u8]) -> bool,
        ) -> Result<(), String> {
            let (host, path) = super::split_https(url)?;
            let headers = super::range_header(from).map(wide);
            // SAFETY: every handle is owned by an `Internet` that outlives
            // the calls using it; strings are NUL-terminated and live across
            // each call; the read buffer's length is passed with it.
            unsafe {
                let session = Internet(WinHttpOpen(
                    wide(&self.agent).as_ptr(),
                    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                    null(),
                    null(),
                    0,
                ));
                if session.0.is_null() {
                    return Err(failed("WinHttpOpen"));
                }
                WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 60_000);
                let connection = Internet(WinHttpConnect(
                    session.0,
                    wide(host).as_ptr(),
                    INTERNET_DEFAULT_HTTPS_PORT,
                    0,
                ));
                if connection.0.is_null() {
                    return Err(failed("connecting"));
                }
                let request = Internet(WinHttpOpenRequest(
                    connection.0,
                    wide("GET").as_ptr(),
                    wide(path).as_ptr(),
                    null(),
                    null(),
                    null(),
                    WINHTTP_FLAG_SECURE,
                ));
                if request.0.is_null() {
                    return Err(failed("WinHttpOpenRequest"));
                }
                let (header_ptr, header_len) = match &headers {
                    // The length excludes the terminating NUL.
                    Some(headers) => (headers.as_ptr(), (headers.len() - 1) as u32),
                    None => (null(), 0),
                };
                if WinHttpSendRequest(request.0, header_ptr, header_len, null(), 0, 0, 0) == 0 {
                    return Err(failed("sending the request"));
                }
                if WinHttpReceiveResponse(request.0, null_mut()) == 0 {
                    return Err(failed("receiving the response"));
                }
                let mut status = 0u32;
                let mut size = 4u32;
                if WinHttpQueryHeaders(
                    request.0,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    null(),
                    (&raw mut status).cast(),
                    &mut size,
                    null_mut(),
                ) == 0
                {
                    return Err(failed("reading the status"));
                }
                start(super::body_starts_at_from(status, from)?);
                let mut buffer = vec![0u8; 256 * 1024];
                loop {
                    let mut read = 0u32;
                    if WinHttpReadData(
                        request.0,
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as u32,
                        &mut read,
                    ) == 0
                    {
                        return Err(failed("reading"));
                    }
                    if read == 0 {
                        return Ok(());
                    }
                    if !chunk(&buffer[..read as usize]) {
                        return Err("stopped".into());
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
impl rea_launcher::download::Fetch for WinHttp {
    fn fetch(
        &self,
        url: &str,
        from: u64,
        start: &mut dyn FnMut(bool),
        chunk: &mut dyn FnMut(&[u8]) -> bool,
    ) -> Result<(), String> {
        WinHttp::fetch(self, url, from, start, chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_split_into_host_and_path() {
        assert_eq!(
            split_https("https://github.com/a/b/releases/download/v1/x.zip"),
            Ok(("github.com", "/a/b/releases/download/v1/x.zip"))
        );
        assert_eq!(split_https("https://example.org"), Ok(("example.org", "/")));
        assert!(split_https("http://example.org/x").is_err());
        assert!(split_https("https:///x").is_err());
    }

    #[test]
    fn resuming_asks_for_a_range_and_reads_the_status() {
        assert_eq!(range_header(0), None);
        assert_eq!(range_header(1024).as_deref(), Some("Range: bytes=1024-"));
        assert_eq!(body_starts_at_from(206, 1024), Ok(true));
        // The server ignored the range: the body starts over.
        assert_eq!(body_starts_at_from(200, 1024), Ok(false));
        assert_eq!(body_starts_at_from(200, 0), Ok(true));
        assert_eq!(
            body_starts_at_from(404, 0),
            Err("the server answered 404".into())
        );
    }

    /// Windows/Wine: the trait is wired, and a refused URL fails before any
    /// network use (the tests run offline).
    #[cfg(windows)]
    #[test]
    fn win_http_is_a_core_fetch() {
        use rea_launcher::download::Fetch;
        let fetch: &dyn Fetch = &WinHttp::new("rea-test/0");
        let error = fetch
            .fetch("http://example.org/x", 0, &mut |_| {}, &mut |_| true)
            .unwrap_err();
        assert!(error.contains("not an https address"), "{error}");
    }
}
