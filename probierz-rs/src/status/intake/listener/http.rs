//! The one HTTP exchange an intake connection carries: its request read
//! whole, and the JSON answer written back with a status RFC 9110 defines.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use serde_json::Value;

use crate::failure::Failure;

/// A status code and the reason phrase RFC 9110 gives it.
#[derive(Clone, Copy)]
pub(crate) struct Status(u16, &'static str);

/// https://www.rfc-editor.org/rfc/rfc9110#section-15.3.3
pub(crate) const ACCEPTED: Status = Status(202, "Accepted");
/// https://www.rfc-editor.org/rfc/rfc9110#section-15.5.1
pub(crate) const BAD_REQUEST: Status = Status(400, "Bad Request");
/// https://www.rfc-editor.org/rfc/rfc9110#section-15.5.2
pub(crate) const UNAUTHORIZED: Status = Status(401, "Unauthorized");
/// https://www.rfc-editor.org/rfc/rfc9110#section-15.5.5
pub(crate) const NOT_FOUND: Status = Status(404, "Not Found");
/// https://www.rfc-editor.org/rfc/rfc9110#section-15.6.1
pub(crate) const INTERNAL_SERVER_ERROR: Status = Status(500, "Internal Server Error");

/// A request as the intake reads it: method, path without its query, the
/// bearer it carries and its body.
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) authorization: Option<String>,
    pub(crate) body: Result<Vec<u8>, String>,
}

/// What reading a connection produced: a request, a client that closed
/// before sending one, or a request whose head is not HTTP.
pub(crate) enum Received {
    Request(Request),
    Closed,
    Malformed(String),
}

/// One line of the head without its line ending; empty at the blank line
/// that ends the head, none when the connection closed.
fn head_line(reader: &mut impl BufRead) -> Result<Option<String>, Failure> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    Ok(Some(line.trim_end_matches(['\r', '\n']).to_string()))
}

/// The body of the length the request declares. A request that declares no
/// Content-Length, one that is not a byte count, and a body that ends before
/// that length are each named, never read as an empty body.
fn body(
    reader: &mut impl Read,
    content_length: Option<String>,
) -> Result<Result<Vec<u8>, String>, Failure> {
    let Some(text) = content_length else {
        return Ok(Err(
            "the request declares no Content-Length, so its body cannot be read whole".to_string(),
        ));
    };
    let declared = match text.parse::<u64>() {
        Ok(declared) => declared,
        Err(error) => {
            return Ok(Err(format!(
                "Content-Length {text:?} is not a byte count: {error}"
            )))
        }
    };
    let mut body = Vec::new();
    reader.take(declared).read_to_end(&mut body)?;
    Ok(match u64::try_from(body.len()) {
        Ok(read) if read == declared => Ok(body),
        Ok(read) => Err(format!(
            "the body ended after {read} of the {declared} bytes its Content-Length declares"
        )),
        Err(error) => Err(format!("the body's length is not a byte count: {error}")),
    })
}

/// Reads the request line, the headers up to the blank line, and the body.
pub(crate) fn receive(stream: &TcpStream) -> Result<Received, Failure> {
    let mut reader = BufReader::new(stream);
    let Some(request_line) = head_line(&mut reader)? else {
        return Ok(Received::Closed);
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(Received::Malformed(format!(
            "the request line {request_line:?} is not METHOD TARGET VERSION"
        )));
    };
    let path = match target.split_once('?') {
        Some((path, _query)) => path,
        None => target,
    };
    let mut authorization = None;
    let mut content_length = None;
    loop {
        let Some(line) = head_line(&mut reader)? else {
            return Ok(Received::Malformed(
                "the connection closed before the request's headers ended".to_string(),
            ));
        };
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Ok(Received::Malformed(format!(
                "the header line {line:?} is not NAME: VALUE"
            )));
        };
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().to_string());
        }
    }
    let body = body(&mut reader, content_length)?;
    Ok(Received::Request(Request {
        method: method.to_string(),
        path: path.to_string(),
        authorization,
        body,
    }))
}

/// Writes `payload` as the JSON answer with `status`.
pub(crate) fn respond(
    stream: &mut TcpStream,
    Status(code, reason): Status,
    payload: &Value,
) -> std::io::Result<()> {
    let text = serde_json::to_string(payload).map_err(std::io::Error::other)?;
    write!(
        stream,
        "HTTP/1.1 {code} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len(),
    )?;
    stream.flush()
}
