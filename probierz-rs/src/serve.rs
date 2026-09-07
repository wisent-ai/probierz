//! Loopback API used by Probierz Desktop.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{json, Value};

use crate::adoption;
use crate::failure::{fail, Answer, Failure};

const BODY_LIMIT: usize = 1024 * 1024;
const HEADER_LIMIT: usize = 16 * 1024;

fn response(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(body).unwrap_or_else(|_| b"{}".to_vec());
    bytes.push(b'\n');
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json; charset=utf-8\r\ncontent-length: {}\r\ncache-control: no-store\r\nconnection: close\r\n\r\n",
        bytes.len(),
    )?;
    stream.write_all(&bytes)?;
    stream.flush()
}

fn error_response(
    stream: &mut TcpStream,
    status: u16,
    detail: impl Into<String>,
) -> std::io::Result<()> {
    response(stream, status, &json!({ "error": detail.into() }))
}

fn read_request(stream: &mut TcpStream) -> Result<(String, String, Vec<u8>), String> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 8192];
    let header_end = loop {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("incomplete HTTP request".to_string());
        }
        received.extend_from_slice(&chunk[..count]);
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
        if received.len() > HEADER_LIMIT {
            return Err("request headers exceed 16 KiB".to_string());
        }
    };

    let headers = std::str::from_utf8(&received[..header_end])
        .map_err(|_| "request headers are not valid UTF-8".to_string())?;
    let mut lines = headers.split("\r\n");
    let mut request = lines.next().unwrap_or("").split_whitespace();
    let method = request.next().unwrap_or("").to_string();
    let target = request.next().unwrap_or("");
    let path = target.split('?').next().unwrap_or("").to_string();

    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value
                .trim()
                .parse::<usize>()
                .map_err(|_| "content-length is not a non-negative integer".to_string())?;
        }
    }
    if content_length > BODY_LIMIT {
        return Err("request body exceeds 1 MiB".to_string());
    }

    while received.len().saturating_sub(header_end) < content_length {
        let count = stream.read(&mut chunk).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        received.extend_from_slice(&chunk[..count]);
        if received.len().saturating_sub(header_end) > BODY_LIMIT {
            return Err("request body exceeds 1 MiB".to_string());
        }
    }
    if received.len().saturating_sub(header_end) < content_length {
        return Err("request body ended before content-length bytes arrived".to_string());
    }
    received.truncate(header_end + content_length);
    Ok((method, path, received.split_off(header_end)))
}

fn request_body(bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

fn handle_connection(mut stream: TcpStream, project_root: &Path) -> Result<(), Failure> {
    let (method, path, bytes) = match read_request(&mut stream) {
        Ok(request) => request,
        Err(detail) => {
            error_response(&mut stream, 400, detail)?;
            return Ok(());
        }
    };

    let answer = match (method.as_str(), path.as_str()) {
        ("GET", "/v1/health") => response(
            &mut stream,
            200,
            &json!({
                "ok": true,
                "product": "probierz",
            }),
        ),
        ("GET", "/v1/project-adoptions") => match adoption::list_project_adoptions(project_root) {
            Ok(body) => response(&mut stream, 200, &body),
            Err(error) => error_response(&mut stream, 400, error.detail),
        },
        ("POST", "/v1/project-adoptions") => match request_body(&bytes) {
            Err(detail) => error_response(&mut stream, 400, detail),
            Ok(body) => {
                let Some(source_root) = body
                    .get("sourceRoot")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                else {
                    error_response(&mut stream, 400, "sourceRoot is required")?;
                    return Ok(());
                };
                let replace = body.get("replace").and_then(Value::as_bool) == Some(true);
                match adoption::adopt_project(project_root, Path::new(source_root), replace) {
                    Ok(body) => response(&mut stream, 200, &body),
                    Err(error) => error_response(&mut stream, 400, error.detail),
                }
            }
        },
        _ => error_response(&mut stream, 404, "not found"),
    };
    answer.map_err(|error| fail("serve.response", error.to_string()))
}

pub fn serve(project_root: &Path, port: u16) -> Answer {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        Failure::unavailable(
            "serve.listen",
            format!("could not bind 127.0.0.1:{port}: {error}"),
        )
    })?;
    let actual_port = listener
        .local_addr()
        .map_err(|error| Failure::unavailable("serve.listen", error.to_string()))?
        .port();
    println!(
        "{}",
        serde_json::to_string(&json!({
            "ready": true,
            "host": "127.0.0.1",
            "port": actual_port,
        }))?
    );
    std::io::stdout().flush()?;

    let project_root = PathBuf::from(project_root);
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let project_root = project_root.clone();
                thread::spawn(move || {
                    let _ = handle_connection(stream, &project_root);
                });
            }
            Err(error) => return Err(Failure::unavailable("serve.listen", error.to_string())),
        }
    }
    Ok(())
}
