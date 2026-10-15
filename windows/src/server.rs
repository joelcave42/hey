//! The TLS listener phones send to. One connection carries one JSON request
//! line and gets one JSON reply line.
//!
//! Requests:
//!   {"t":"pair","key":"<from QR>","device":"Pixel 8"}  -> {"ok":true,"token":"...","name":"PC"}
//!   {"t":"text","token":"...","text":"..."}            -> {"ok":true}
//!   {"t":"ping","token":"..."}                         -> {"ok":true,"name":"PC"}

use crate::config::{self, Config, Device};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const PORT: u16 = 47820;
const MAX_REQUEST: u64 = 1 << 20;
const PAIRING_WINDOW: Duration = Duration::from_secs(10 * 60);

pub enum Event {
    Received { device: String, text: String },
    Paired { device: String },
}

pub struct Shared {
    pub config: Mutex<Config>,
    /// The one-time key shown in the current pairing QR code, if any.
    pub pending_pair: Mutex<Option<(String, Instant)>>,
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
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut tls_stream = StreamOwned::new(ServerConnection::new(tls)?, stream);

    let mut line = String::new();
    BufReader::new((&mut tls_stream).take(MAX_REQUEST)).read_line(&mut line)?;
    let request: Value = serde_json::from_str(&line)?;
    let reply = respond(&request, shared, events);

    tls_stream.write_all(format!("{reply}\n").as_bytes())?;
    tls_stream.flush()?;
    tls_stream.conn.send_close_notify();
    let _ = tls_stream.flush();
    Ok(())
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
            });
            config.save();
            let _ = events.send(Event::Paired { device });
            json!({"ok": true, "token": token, "name": name})
        }
        t @ ("text" | "ping") => {
            let Some(device) = shared
                .config
                .lock()
                .unwrap()
                .device_for_token(field("token"))
                .map(|d| d.name.clone())
            else {
                return json!({"ok": false, "error": "This phone isn't paired with this PC anymore. Pair it again."});
            };
            if t == "text" {
                let text = field("text").to_string();
                if text.is_empty() {
                    return json!({"ok": false, "error": "Nothing to send."});
                }
                let _ = events.send(Event::Received { device, text });
            }
            json!({"ok": true, "name": name})
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
