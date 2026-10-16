//! The TLS listener phones send to. One connection carries one JSON request
//! line (plus, for a file, its bytes) and gets one JSON reply line.
//!
//! Requests:
//!   {"t":"pair","key":"<from QR>","device":"Pixel 8"}  -> {"ok":true,"token":"...","name":"PC"}
//!   {"t":"text","token":"...","text":"..."}            -> {"ok":true}
//!   {"t":"file","token":"...","name":"IMG_1.jpg","size":N,"index":0,"count":1}
//!       followed by exactly N bytes                    -> {"ok":true}
//!   {"t":"ping","token":"..."}                         -> {"ok":true,"name":"PC","ips":[...]}
//!   {"t":"listen","token":"..."}  keeps the connection open for PC → phone (see push.rs)

use crate::config::{self, Config, Device};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const PORT: u16 = 47820;
const MAX_REQUEST: u64 = 1 << 20;
const PAIRING_WINDOW: Duration = Duration::from_secs(10 * 60);

pub enum Event {
    Received {
        device: String,
        text: String,
    },
    Paired {
        device: String,
    },
    /// A paired phone checked in, connected or disconnected, so its status
    /// in the window should update.
    Seen,
    /// Something sent to a phone arrived (`error` is None) or didn't.
    Delivered {
        id: u64,
        error: Option<String>,
    },
    /// A file finished arriving. `index` and `count` say where it sits in a
    /// batch the phone sent together, so the PC can show one notification.
    File {
        device: String,
        path: PathBuf,
        index: u64,
        count: u64,
    },
}

pub struct Shared {
    pub config: Mutex<Config>,
    /// The one-time key shown in the current pairing QR code, if any.
    pub pending_pair: Mutex<Option<(String, Instant)>>,
    /// Phones connected right now, ready to receive, by name.
    pub phones: Mutex<std::collections::HashMap<String, crate::push::Phone>>,
}

pub fn start(shared: Arc<Shared>, events: Sender<Event>) -> std::io::Result<u16> {
    let listener =
        TcpListener::bind(("0.0.0.0", PORT)).or_else(|_| TcpListener::bind(("0.0.0.0", 0)))?;
    let port = listener.local_addr()?.port();
    let tls = Arc::new(tls_config(&shared.config.lock().unwrap()));

    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (tls, shared, events) = (tls.clone(), shared.clone(), events.clone());
            thread::spawn(move || {
                if let Err(e) = handle(stream, tls, &shared, &events) {
                    eprintln!("connection failed: {e}");
                }
            });
        }
    });
    Ok(port)
}

fn tls_config(config: &Config) -> ServerConfig {
    let cert = CertificateDer::from(config::unb64(&config.cert_der));
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(config::unb64(&config.key_der)));
    ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("TLS versions")
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .expect("this PC's certificate is invalid")
}

fn handle(
    stream: TcpStream,
    tls: Arc<ServerConfig>,
    shared: &Shared,
    events: &Sender<Event>,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut tls_stream = StreamOwned::new(ServerConnection::new(tls)?, stream);
    let mut reader = BufReader::new(&mut tls_stream);

    let mut line = String::new();
    reader.by_ref().take(MAX_REQUEST).read_line(&mut line)?;
    let request: Value = serde_json::from_str(&line)?;
    if request.get("t").and_then(Value::as_str) == Some("listen") {
        let Some(device) = device_for(&request, shared) else {
            let stream = reader.into_inner();
            stream.write_all(format!("{}\n", not_paired()).as_bytes())?;
            return Ok(stream.flush()?);
        };
        return Ok(crate::push::serve(&mut reader, device, shared, events)?);
    }
    let reply = if request.get("t").and_then(Value::as_str) == Some("file") {
        receive_file(&request, &mut reader, shared, events)
    } else {
        respond(&request, shared, events)
    };

    let stream = reader.into_inner();
    stream.write_all(format!("{reply}\n").as_bytes())?;
    stream.flush()?;
    stream.conn.send_close_notify();
    let _ = stream.flush();
    Ok(())
}

/// The paired phone a request comes from, noting that it was just seen.
fn device_for(request: &Value, shared: &Shared) -> Option<String> {
    let token = request.get("token").and_then(Value::as_str).unwrap_or("");
    let hash = config::hash(token);
    let mut config = shared.config.lock().unwrap();
    let device = config.devices.iter_mut().find(|d| d.token_hash == hash)?;
    device.last_seen = crate::feed::now_ms();
    let name = device.name.clone();
    config.save();
    Some(name)
}

fn not_paired() -> Value {
    json!({"ok": false, "error": "This phone isn't paired with this PC anymore. Pair it again."})
}

fn receive_file(
    request: &Value,
    reader: &mut impl Read,
    shared: &Shared,
    events: &Sender<Event>,
) -> Value {
    let Some(device) = device_for(request, shared) else {
        return not_paired();
    };
    let number = |name: &str| request.get(name).and_then(Value::as_u64).unwrap_or(0);
    let name = request
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("file");
    match crate::files::receive(reader, name, number("size")) {
        Ok(path) => {
            let _ = events.send(Event::File {
                device,
                path,
                index: number("index"),
                count: number("count").max(1),
            });
            json!({"ok": true})
        }
        Err(e) => json!({"ok": false, "error": format!("Your PC couldn't save {name}: {e}")}),
    }
}

fn respond(request: &Value, shared: &Shared, events: &Sender<Event>) -> Value {
    let field = |name: &str| request.get(name).and_then(Value::as_str).unwrap_or("");
    let name = config::pc_name();

    match field("t") {
        "pair" => {
            let mut pending = shared.pending_pair.lock().unwrap();
            let valid = matches!(&*pending,
                Some((key, at)) if key == field("key") && at.elapsed() < PAIRING_WINDOW);
            if !valid {
                return json!({"ok": false, "error": "This pairing code has expired. Choose Pair a phone on your PC again."});
            }
            *pending = None;

            let device = clean_device_name(field("device"));
            let token = config::random_token(32);
            let mut config = shared.config.lock().unwrap();
            config.devices.retain(|d| d.name != device);
            config.devices.push(Device {
                name: device.clone(),
                token_hash: config::hash(&token),
                last_seen: crate::feed::now_ms(),
            });
            config.save();
            let _ = events.send(Event::Paired { device });
            json!({"ok": true, "token": token, "name": name})
        }
        t @ ("text" | "ping") => {
            let Some(device) = device_for(request, shared) else {
                return not_paired();
            };
            if t == "text" {
                let text = field("text").to_string();
                if text.is_empty() {
                    return json!({"ok": false, "error": "Nothing to send."});
                }
                let _ = events.send(Event::Received { device, text });
            } else {
                let _ = events.send(Event::Seen);
            }
            // The phone keeps its list of this PC's addresses up to date from
            // this, so it learns a new Tailscale address without re-pairing.
            json!({"ok": true, "name": name, "ips": crate::net::addresses()})
        }
        _ => json!({"ok": false, "error": "Unknown request. Update Hey on your phone."}),
    }
}

fn clean_device_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect();
    if name.trim().is_empty() {
        "Phone".to_string()
    } else {
        name.trim().to_string()
    }
}

/// Advertise this PC on the local network so paired phones can find it after
/// its IP address changes.
pub fn advertise(pc_id: &str, port: u16) -> Option<mdns_sd::ServiceDaemon> {
    let daemon = mdns_sd::ServiceDaemon::new().ok()?;
    let host: String = config::pc_name()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let info = mdns_sd::ServiceInfo::new(
        "_hey._tcp.local.",
        &format!("hey-{pc_id}"),
        &format!("{host}.local."),
        "",
        port,
        &[("id", pc_id), ("name", config::pc_name().as_str())][..],
    )
    .ok()?
    .enable_addr_auto();
    daemon.register(info).ok()?;
    Some(daemon)
}
