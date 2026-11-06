//! Receives a browser redirect on `127.0.0.1` (RFC 8252 §7.3).

use std::io::{self, Read as _, Write as _};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use url::Url;

use super::{AuthAttempt, AuthorizationError, BrowserAuthorization};
use crate::error::ProviderError;
use crate::secret::Secret;

const POLL_INTERVAL: Duration = Duration::from_millis(25);
/// How long one connection may take to send its request, so a slow client cannot hold the
/// receiver past cancellation or its own timeout for long.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_REQUEST_HEAD: usize = 8 * 1024;

const SIGNED_IN_PAGE: &str = "Signed in. You can close this tab and return to Parabolica.";
const FAILED_PAGE: &str = "Sign-in did not finish. Return to Parabolica to try again.";
const STALE_PAGE: &str = "This sign-in link is no longer active. Start again from Parabolica.";

/// A one-use redirect endpoint on an ephemeral loopback port.
#[derive(Debug)]
pub struct LoopbackReceiver {
    listener: TcpListener,
    redirect_uri: Url,
}

impl LoopbackReceiver {
    /// Listens for redirects to `path`, which starts with `/`.
    pub fn bind(path: &str) -> Result<Self, AuthorizationError> {
        let callback = |error: io::Error| AuthorizationError::Callback(error.kind());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(callback)?;
        listener.set_nonblocking(true).map_err(callback)?;
        let port = listener.local_addr().map_err(callback)?.port();
        let redirect_uri = Url::parse(&format!("http://127.0.0.1:{port}"))
            .and_then(|base| base.join(path))
            .map_err(|_| AuthorizationError::Callback(io::ErrorKind::InvalidInput))?;
        Ok(Self {
            listener,
            redirect_uri,
        })
    }

    /// The `redirect_uri` to register with the authorization request.
    pub fn redirect_uri(&self) -> &Url {
        &self.redirect_uri
    }

    /// Waits for the redirect that answers `authorization` and returns its code.
    ///
    /// Requests for other paths, or carrying another request's `state`, get an error page and
    /// are otherwise ignored, so a stale browser tab can neither end nor complete this attempt.
    pub fn receive(
        &self,
        authorization: &BrowserAuthorization,
        attempt: &AuthAttempt,
        timeout: Duration,
    ) -> Result<Secret, ProviderError> {
        let deadline = Instant::now() + timeout;
        loop {
            attempt.check()?;
            if Instant::now() >= deadline {
                return Err(attempt.fail(AuthorizationError::TimedOut));
            }
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(outcome) = self.serve(stream, authorization) {
                        return outcome.map_err(|error| attempt.fail(error));
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(POLL_INTERVAL)
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    return Err(attempt.fail(AuthorizationError::Callback(error.kind())));
                }
            }
        }
    }

    /// Answers one connection. `None` means keep waiting.
    fn serve(
        &self,
        mut stream: TcpStream,
        authorization: &BrowserAuthorization,
    ) -> Option<Result<Secret, AuthorizationError>> {
        let configured = stream.set_nonblocking(false).is_ok()
            && stream.set_write_timeout(Some(CONNECTION_TIMEOUT)).is_ok();
        if !configured {
            return None;
        }
        let head = read_head(&mut stream, Instant::now() + CONNECTION_TIMEOUT)?;
        let mut request_line = head.lines().next()?.split(' ');
        let (method, target) = (request_line.next()?, request_line.next()?);
        if method != "GET" {
            respond(&mut stream, "405 Method Not Allowed", "Not supported.");
            return None;
        }
        // Origin-form only: `//host/...` would resolve to another host.
        let url = (target.starts_with('/') && !target.starts_with("//"))
            .then(|| self.redirect_uri.join(target).ok())
            .flatten();
        let Some(url) = url.filter(|url| url.path() == self.redirect_uri.path()) else {
            respond(&mut stream, "404 Not Found", "Not found.");
            return None;
        };
        match authorization.validate(url.query_pairs()) {
            Err(AuthorizationError::StateMismatch) => {
                respond(&mut stream, "400 Bad Request", STALE_PAGE);
                None
            }
            Ok(code) => {
                respond(&mut stream, "200 OK", SIGNED_IN_PAGE);
                Some(Ok(code))
            }
            Err(error) => {
                respond(&mut stream, "200 OK", FAILED_PAGE);
                Some(Err(error))
            }
        }
    }
}

fn read_head(stream: &mut TcpStream, deadline: Instant) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if head.len() >= MAX_REQUEST_HEAD || remaining.is_zero() {
            return None;
        }
        stream.set_read_timeout(Some(remaining)).ok()?;
        let read = stream.read(&mut buffer).ok().filter(|read| *read > 0)?;
        head.extend_from_slice(&buffer[..read]);
    }
    String::from_utf8(head).ok()
}

fn respond(stream: &mut TcpStream, status: &str, message: &str) {
    let body = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Parabolica</title><p>{message}</p></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    // The browser may already have closed the tab; the outcome stands either way.
    let _ = stream.write_all(response.as_bytes());
}
