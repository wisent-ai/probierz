//! A collab relay on loopback that accepts what jeden sends: a POST is
//! acknowledged with a sequence number, anything else reads an empty event
//! page. It lives on its own thread for the length of one journey.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use super::constants::{RELAY_EMPTY_PAGE, RELAY_POSTED};

pub(super) struct Relay {
    port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

fn answer(stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok_and(|read| read > 0) && !header.trim().is_empty() {
        header.clear();
    }
    let body = if request.starts_with("POST") {
        RELAY_POSTED
    } else {
        RELAY_EMPTY_PAGE
    };
    let mut writer = &stream;
    let _ = write!(
        writer,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

impl Relay {
    pub(super) fn start() -> Result<Relay, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("the collab relay stub cannot listen: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let worker = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    answer(stream);
                }
            }
        });
        Ok(Relay {
            port,
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // One connection wakes the accepting thread so it sees the stop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
