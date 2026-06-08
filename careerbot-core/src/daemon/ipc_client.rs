//! Tiny hand-rolled HTTP/1.1 client over a Unix domain socket.
//!
//! We only ever talk to our own daemon on the same machine, so a
//! one-shot request/response sufficies — no chunked encoding, no
//! keep-alive, no TLS. This keeps the dep graph free of
//! hyper-util/UDS feature flags.

use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Result of a successful request: HTTP status code + body.
pub type Response = (u16, String);

/// Send `method path` with optional headers to the daemon socket and
/// return its status code and body. Empty body on request side.
pub async fn request(
    socket: &Path,
    method: &str,
    path: &str,
    extra_headers: &[(&str, &str)],
) -> std::io::Result<Response> {
    let mut stream = UnixStream::connect(socket).await?;

    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nConnection: close\r\n"
    );
    for (k, v) in extra_headers {
        head.push_str(k);
        head.push_str(": ");
        head.push_str(v);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    // Don't half-close the write side — hyper's HTTP/1 parser on the
    // server side reads headers + body and responds, then closes
    // because we asked for Connection: close. Half-closing here can
    // race with hyper's read of the request body (it reads `\r\n\r\n`
    // and the configured Content-Length bytes after).

    let mut buf = Vec::with_capacity(4096);
    stream.read_to_end(&mut buf).await?;

    parse_response(&buf)
}

fn parse_response(buf: &[u8]) -> std::io::Result<Response> {
    let text = String::from_utf8_lossy(buf);
    let (head, body) = text.split_once("\r\n\r\n").ok_or_else(|| {
        std::io::Error::other(format!(
            "malformed daemon response: no header/body split; got {} bytes: {:?}",
            buf.len(),
            &text[..text.len().min(200)]
        ))
    })?;

    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| std::io::Error::other("malformed daemon response: empty"))?;

    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| std::io::Error::other("malformed daemon response: no status code"))?;

    Ok((status, body.to_string()))
}

/// Liveness check used by `careerbot status` — `Ok(true)` if the socket
/// exists and accepts a connection, `Ok(false)` if it isn't reachable,
/// `Err` only for unexpected IO failures.
pub async fn is_running(socket: &Path) -> std::io::Result<bool> {
    if !socket.exists() {
        return Ok(false);
    }
    match UnixStream::connect(socket).await {
        Ok(_) => Ok(true),
        Err(e)
            if e.kind() == std::io::ErrorKind::ConnectionRefused
                || e.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_handles_simple_ok() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";
        let (status, body) = parse_response(raw).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, "hello");
    }

    #[test]
    fn parse_response_handles_empty_body() {
        let raw = b"HTTP/1.1 202 Accepted\r\n\r\n";
        let (status, body) = parse_response(raw).unwrap();
        assert_eq!(status, 202);
        assert_eq!(body, "");
    }

    #[test]
    fn parse_response_errors_on_malformed() {
        let raw = b"not a response";
        assert!(parse_response(raw).is_err());
    }
}
