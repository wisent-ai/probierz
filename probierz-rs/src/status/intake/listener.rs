//! The failure intake listener: its bind address, one connection, and the serve answer.

mod http;
mod token;

use std::num::NonZeroU16;

use crate::status::*;

use self::http::{
    receive, respond, Received, Status, ACCEPTED, BAD_REQUEST, INTERNAL_SERVER_ERROR, NOT_FOUND,
    UNAUTHORIZED,
};
use self::token::{intake_directory, intake_token};

/// The host and port of `--bind`. The port is a TCP port a listener can
/// name: a number that fits the port field and is not the "any port" zero.
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
    match port.parse::<NonZeroU16>() {
        Ok(port) => Ok((host, port.get())),
        Err(error) => Err(Failure::invalid(
            "intake.bind",
            format!("--bind port out of range: {port} ({error})"),
        )),
    }
}

fn refuse(stream: &mut TcpStream, status: Status, code: &str, detail: &str) -> std::io::Result<()> {
    respond(stream, status, &failure_envelope(code, detail))
}

pub(crate) fn handle_connection(mut stream: TcpStream, token: &str) -> Result<(), Failure> {
    let request = match receive(&stream)? {
        Received::Closed => return Ok(()),
        Received::Malformed(problem) => {
            refuse(&mut stream, BAD_REQUEST, "unknown", &problem)?;
            return Ok(());
        }
        Received::Request(request) => request,
    };
    if request.method != "POST" || request.path != "/v1/failures" {
        refuse(
            &mut stream,
            NOT_FOUND,
            "not_found",
            "unknown route; the intake endpoint is POST /v1/failures",
        )?;
        return Ok(());
    }
    if !authorized(request.authorization.as_deref(), token) {
        refuse(
            &mut stream,
            UNAUTHORIZED,
            "auth",
            "missing or wrong bearer token",
        )?;
        return Ok(());
    }
    let parsed = request.body.and_then(|bytes| {
        serde_json::from_slice::<Value>(&bytes)
            .map_err(|error| format!("body is not valid JSON: {error}"))
    });
    let body = match parsed {
        Ok(body) => body,
        Err(problem) => {
            refuse(&mut stream, BAD_REQUEST, "unknown", &problem)?;
            return Ok(());
        }
    };
    if let Some(problem) = envelope_problem(&body) {
        refuse(&mut stream, BAD_REQUEST, "unknown", &problem)?;
        return Ok(());
    }
    match store_envelope(&body) {
        Ok(()) => respond(&mut stream, ACCEPTED, &json!({ "accepted": true }))?,
        Err(error) => refuse(
            &mut stream,
            INTERNAL_SERVER_ERROR,
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
            "probierz intake: generated a new intake token at {} (owner-only), shown once:\n{}\nSet PROBIERZ_INTAKE_TOKEN to this value in each desktop app.",
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
    let url_file = intake_directory()?.join("intake-url");
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
                    // A connection this intake could not serve is said on its
                    // log with the point and cause, never dropped unseen.
                    if let Err(failure) = handle_connection(stream, &token) {
                        eprintln!(
                            "probierz intake: a connection was not served at {}: {}",
                            failure.point, failure.detail
                        );
                    }
                });
            }
            Err(error) => return Err(Failure::unavailable("intake.listen", error.to_string())),
        }
    }
    Ok(())
}
