//! `probierz intake serve` through the real binary on a home that has never
//! held `~/.probierz`, the bearer token given in `PROBIERZ_INTAKE_TOKEN`.
//!
//! The intake creates its directory, writes the address it listens on to
//! `~/.probierz/intake-url`, and answers over HTTP: a wrong bearer is
//! Unauthorized, a POST that declares no Content-Length is a Bad Request
//! naming it. A token variable that is set but empty is refused at
//! `intake.token` naming it before anything binds. Each run's home lies
//! under the crate's target directory and is kept as evidence.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const PROBIERZ: &str = env!("CARGO_BIN_EXE_probierz");

/// The port that asks the system to assign any free one:
/// https://man7.org/linux/man-pages/man7/ip.7.html
const ANY_PORT: u16 = 0;

fn fresh_home(name: &str) -> PathBuf {
    let home = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "intake-serve-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&home).expect("create the isolated home");
    home
}

/// A loopback address no listener holds: the system names a free port for
/// a listener that asks for any, and it is released for the intake.
fn free_loopback() -> String {
    let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, ANY_PORT))
        .expect("the system names a free loopback port");
    probe
        .local_addr()
        .expect("the probe has an address")
        .to_string()
}

struct Serving(Child);

impl Drop for Serving {
    fn drop(&mut self) {
        if let Err(error) = self.0.kill() {
            eprintln!("the intake could not be stopped: {error}");
        }
        if let Err(error) = self.0.wait() {
            eprintln!("the intake's exit could not be read: {error}");
        }
    }
}

/// The intake's answer to one request: the status line and the body.
fn exchange(address: &str, head: &str, body: &str) -> (String, String) {
    let mut stream = TcpStream::connect(address).expect("connect to the intake");
    stream
        .write_all(format!("{head}\r\n{body}").as_bytes())
        .expect("send the request");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("read the answer");
    let Some((head, body)) = answer.split_once("\r\n\r\n") else {
        panic!("the answer has no head: {answer:?}");
    };
    let status = head.lines().next().expect("a status line").to_string();
    (status, body.to_string())
}

#[test]
fn a_fresh_home_with_an_environment_token_gets_its_address_file() {
    let home = fresh_home("environment-token");
    let bind = free_loopback();
    let mut child = Command::new(PROBIERZ)
        .args(["intake", "serve", "--bind", &bind])
        .env("HOME", &home)
        .env("PROBIERZ_INTAKE_TOKEN", "journey-token")
        .stderr(Stdio::piped())
        .spawn()
        .expect("start probierz intake serve");
    let stderr = child.stderr.take().expect("the intake's stderr");
    let serving = Serving(child);
    let mut said = String::new();
    for line in BufReader::new(stderr).lines() {
        let line = line.expect("read the intake's stderr");
        said.push_str(&line);
        said.push('\n');
        if line.contains("listening on") {
            break;
        }
    }
    assert!(
        said.contains("listening on"),
        "the intake never listened: {said}"
    );

    let written = match std::fs::read_to_string(home.join(".probierz/intake-url")) {
        Ok(written) => written,
        Err(error) => panic!("intake-url was not written: {error}; the intake said {said}"),
    };
    assert_eq!(written, format!("http://{bind}"));

    let envelope = "{}";
    let (status, body) = exchange(
        &bind,
        &format!(
            "POST /v1/failures HTTP/1.1\r\nauthorization: Bearer wrong\r\ncontent-length: {}\r\n",
            envelope.len()
        ),
        envelope,
    );
    assert!(status.ends_with("Unauthorized"), "{status} {body}");

    let (status, body) = exchange(
        &bind,
        "POST /v1/failures HTTP/1.1\r\nauthorization: Bearer journey-token\r\n",
        "",
    );
    assert!(status.ends_with("Bad Request"), "{status} {body}");
    assert!(body.contains("declares no Content-Length"), "{body}");
    drop(serving);
}

#[test]
fn an_empty_environment_token_is_refused_by_name() {
    let home = fresh_home("empty-token");
    let output = Command::new(PROBIERZ)
        .args(["intake", "serve", "--bind", &free_loopback()])
        .env("HOME", &home)
        .env("PROBIERZ_INTAKE_TOKEN", "")
        .output()
        .expect("run probierz intake serve");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "{said}");
    assert!(said.contains("intake.token"), "{said}");
    assert!(
        said.contains("PROBIERZ_INTAKE_TOKEN is set but empty"),
        "{said}"
    );
}
