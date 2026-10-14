//! The failure intake listener: its bind address and token, one connection, and the serve answer.

use crate::status::*;

pub(crate) fn parse_bind(bind: &str) -> Result<(&str, u16), Failure> {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return Err(Failure::invalid(
            "intake.bind",
            format!("--bind needs host:port, got {bind:?}"),
        ));
    };
    if host.is_empty() || port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Failure::invalid(
            "intake.bind",
            format!("--bind needs host:port, got {bind:?}"),
        ));
    }
    let port_number = port
        .parse::<u32>()
        .ok()
        .filter(|port| (1..=65535).contains(port))
        .ok_or_else(|| {
            Failure::invalid("intake.bind", format!("--bind port out of range: {port}"))
        })?;
    Ok((host, port_number as u16))
}

pub(crate) fn random_token() -> Result<String, Failure> {
    let mut bytes = [0u8; 24];
    rand_core::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|error| {
            Failure::unavailable(
                "intake.token",
                format!("could not generate intake token: {error}"),
            )
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn intake_token() -> Result<(String, bool, PathBuf), Failure> {
    if let Ok(token) = std::env::var("PROBIERZ_INTAKE_TOKEN") {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok((
                token,
                false,
                home_dir().join(".probierz").join("intake-token"),
            ));
        }
    }
    let file = home_dir().join(".probierz").join("intake-token");
    // Only a token file that is not there, or holds nothing, is minted anew.
    // One that exists and cannot be read is refused: replacing it would cut
    // off every client already paired with the token it holds.
    match fs::read_to_string(&file) {
        Ok(existing) => {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                return Ok((existing, false, file));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Failure::config(
                "intake.token",
                format!(
                    "cannot read the intake token {}, so it is not replaced: {error}",
                    file.display()
                ),
            ))
        }
    }
    let parent = file.parent().unwrap_or(Path::new("."));
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        let mut directories = fs::DirBuilder::new();
        directories.recursive(true).mode(0o700);
        directories.create(parent)?;
        let token = random_token()?;
        let mut output = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&file)?;
        output.write_all(format!("{token}\n").as_bytes())?;
        return Ok((token, true, file));
    }
    #[cfg(not(unix))]
    {
        fs::create_dir_all(parent)?;
        let token = random_token()?;
        fs::write(&file, format!("{token}\n"))?;
        Ok((token, true, file))
    }
}

pub(crate) fn response(
    stream: &mut TcpStream,
    status: u16,
    payload: &Value,
) -> std::io::Result<()> {
    let text = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_string());
    let reason = match status {
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
        text.len(),
    )?;
    stream.flush()
}

pub(crate) fn failure_response(
    stream: &mut TcpStream,
    status: u16,
    code: &str,
    detail: &str,
) -> std::io::Result<()> {
    response(stream, status, &failure_envelope(code, detail))
}

pub(crate) fn handle_connection(mut stream: TcpStream, token: &str) -> Result<(), Failure> {
    let mut received = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(());
        }
        received.extend_from_slice(&chunk[..count]);
        if let Some(index) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = String::from_utf8_lossy(&received[..header_end]);
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let route = parts.next().unwrap_or("").split('?').next().unwrap_or("");
    let mut authorization = None;
    let mut content_length: Option<String> = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("authorization") {
            authorization = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.trim().to_string());
        }
    }
    if method != "POST" || route != "/v1/failures" {
        failure_response(
            &mut stream,
            404,
            "not_found",
            "unknown route; the intake endpoint is POST /v1/failures",
        )?;
        return Ok(());
    }
    if !authorized(authorization.as_deref(), token) {
        failure_response(&mut stream, 401, "auth", "missing or wrong bearer token")?;
        return Ok(());
    }
    // The body is the length the request declares. A request that declares
    // none, or a length that is not a byte count, is refused naming it,
    // never read as an empty body that then "is not valid JSON".
    let declared = match content_length.as_deref() {
        Some(text) => text
            .parse::<usize>()
            .map_err(|error| format!("Content-Length {text:?} is not a byte count: {error}")),
        None => Err(
            "the request declares no Content-Length, so its body cannot be read whole".to_string(),
        ),
    };
    let parsed = match declared {
        Ok(content_length) => {
            while received.len().saturating_sub(header_end) < content_length {
                let count = stream.read(&mut chunk)?;
                let read = &chunk[..count];
                if read.is_empty() {
                    break;
                }
                received.extend_from_slice(read);
            }
            let body_bytes = &received[header_end..received.len().min(header_end + content_length)];
            serde_json::from_slice::<Value>(body_bytes)
                .map_err(|error| format!("body is not valid JSON: {error}"))
        }
        Err(problem) => Err(problem),
    };
    let (body, problem) = match parsed {
        Ok(body) => {
            let problem = envelope_problem(&body);
            (body, problem)
        }
        Err(problem) => (Value::Null, Some(problem)),
    };
    if let Some(problem) = problem {
        failure_response(&mut stream, 400, "unknown", &problem)?;
        return Ok(());
    }
    match store_envelope(&body) {
        Ok(()) => response(&mut stream, 202, &json!({ "accepted": true }))?,
        Err(error) => failure_response(
            &mut stream,
            500,
            "infra_down",
            &format!("intake store failed: {}", error.detail),
        )?,
    }
    Ok(())
}

pub fn intake_serve(bind: &str) -> Answer {
    let (host, port) = parse_bind(bind)?;
    super::predecessor::retire();
    let (token, created, token_file) = intake_token()?;
    if created {
        eprintln!(
            "probierz intake: generated a new intake token at {} (mode 0600), shown once:\n{}\nSet PROBIERZ_INTAKE_TOKEN to this value in each desktop app.",
            token_file.display(),
            token,
        );
    }
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| Failure::invalid("intake.bind", error.to_string()))?
        .next()
        .ok_or_else(|| {
            Failure::invalid(
                "intake.bind",
                format!("--bind needs host:port, got {bind:?}"),
            )
        })?;
    let listener = TcpListener::bind(address).map_err(|error| {
        Failure::unavailable("intake.listen", format!("could not bind {bind}: {error}"))
    })?;
    // Clients find the intake where it actually listens: the address the
    // service declaration bound is written beside the token, so no client
    // carries a port of its own.
    let listening = listener.local_addr().map_err(|error| {
        Failure::unavailable(
            "intake.listen",
            format!("the bound {bind} has no local address: {error}"),
        )
    })?;
    let url = format!("http://{listening}");
    let url_file = home_dir().join(".probierz").join("intake-url");
    std::fs::write(&url_file, &url).map_err(|error| {
        Failure::config(
            "intake.address",
            format!(
                "{} could not be written, so no client can find this intake: {error}",
                url_file.display()
            ),
        )
    })?;
    eprintln!(
        "probierz intake: listening on {url}/v1/failures (address in {})",
        url_file.display()
    );
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                let token = token.clone();
                thread::spawn(move || {
                    let _ = handle_connection(stream, &token);
                });
            }
            Err(error) => return Err(Failure::unavailable("intake.listen", error.to_string())),
        }
    }
    Ok(())
}
