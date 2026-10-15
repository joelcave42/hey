//! PC → phone. A paired phone keeps one connection open (`{"t":"listen"}`)
//! from a background service, and the PC pushes things down it:
//!
//!   {"t":"text","id":N,"text":"..."}              -> {"ok":true}
//!   {"t":"file","id":N,"name":"a.pdf","size":S}   followed by S bytes -> {"ok":true}
//!   {"t":"ping"}                                  -> {"t":"pong"}
//!
//! The phone connects out to the PC, so nothing listens on the phone and the
//! connection is pinned to this PC's certificate like every other one.

use crate::server::{Event, Shared};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

const HEARTBEAT: Duration = Duration::from_secs(25);
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

pub enum Outgoing {
    Text(String),
    File(PathBuf),
}

pub struct Job {
    /// The feed item this is, so the window can show Sent or Failed.
    pub id: u64,
    pub what: Outgoing,
}

/// A connected phone: where to hand it jobs, and which connection that is.
pub struct Phone {
    pub jobs: Sender<Job>,
    connection: u64,
}

/// Serves one phone's listen connection until it drops.
pub fn serve(
    reader: &mut BufReader<&mut Tls>,
    device: String,
    shared: &Shared,
    events: &Sender<Event>,
) -> io::Result<()> {
    let (jobs, inbox) = mpsc::channel::<Job>();
    let connection = crate::feed::now_ms();
    shared
        .phones
        .lock()
        .unwrap()
        .insert(device.clone(), Phone { jobs, connection });
    let _ = events.send(Event::Seen);

    let result = (|| -> io::Result<()> {
        write_line(
            reader,
            &json!({"ok": true, "name": crate::config::pc_name()}),
        )?;
        reader
            .get_ref()
            .sock
            .set_read_timeout(Some(REPLY_TIMEOUT))?;
        loop {
            match inbox.recv_timeout(HEARTBEAT) {
                Ok(job) => {
                    let outcome = deliver(reader, &job);
                    let failed = outcome.as_ref().err().map(|e| e.to_string());
                    let _ = events.send(Event::Delivered {
                        id: job.id,
                        error: failed.clone(),
                    });
                    // A broken connection ends the session; a refused item doesn't.
                    if let Err(e) = outcome
                        && e.kind() != io::ErrorKind::Other
                    {
                        return Err(e);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    write_line(reader, &json!({"t": "ping"}))?;
                    read_reply(reader)?;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
    })();

    // Only forget this phone if a newer connection hasn't replaced it.
    let mut phones = shared.phones.lock().unwrap();
    if phones.get(&device).map(|p| p.connection) == Some(connection) {
        phones.remove(&device);
        drop(phones);
        let _ = events.send(Event::Seen);
    }
    result
}

fn deliver(reader: &mut BufReader<&mut Tls>, job: &Job) -> io::Result<()> {
    match &job.what {
        Outgoing::Text(text) => {
            write_line(reader, &json!({"t": "text", "id": job.id, "text": text}))?;
        }
        Outgoing::File(path) => {
            // Problems with the file fail this item but keep the phone connected.
            let cant_open = |e: io::Error| io::Error::other(format!("Couldn't open the file: {e}"));
            let mut file = File::open(path).map_err(cant_open)?;
            let size = file.metadata().map_err(cant_open)?.len();
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".into());
            write_line(
                reader,
                &json!({"t": "file", "id": job.id, "name": name, "size": size}),
            )?;
            let stream = reader.get_mut();
            let copied = io::copy(&mut (&mut file).take(size), stream)?;
            stream.flush()?;
            if copied != size {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the file changed while it was being sent",
                ));
            }
        }
    }
    let reply = read_reply(reader)?;
    if reply["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(io::Error::other(
            reply["error"]
                .as_str()
                .unwrap_or("Your phone said no.")
                .to_string(),
        ))
    }
}

fn write_line(reader: &mut BufReader<&mut Tls>, value: &Value) -> io::Result<()> {
    let stream = reader.get_mut();
    stream.write_all(format!("{value}\n").as_bytes())?;
    stream.flush()
}

fn read_reply<R: BufRead>(reader: &mut R) -> io::Result<Value> {
    let mut line = String::new();
    if reader.by_ref().take(1 << 16).read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "the phone disconnected",
        ));
    }
    serde_json::from_str(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
