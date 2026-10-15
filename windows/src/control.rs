//! Lets a second `hey.exe` talk to the Hey that's already running: hand it
//! files from Explorer's "Send to phone", or ask it to show its window. The
//! running Hey listens on 127.0.0.1 only, and a random key in
//! %APPDATA%\Hey\control.json keeps other programs from using it.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

fn path() -> PathBuf {
    crate::config::dir().join("control.json")
}

/// Starts listening and calls `on_message` with each request (minus the key).
pub fn listen(on_message: impl Fn(Value) + Send + 'static) -> std::io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let key = crate::config::random_token(24);
    let info = json!({"port": listener.local_addr()?.port(), "key": key});
    std::fs::write(path(), info.to_string())?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Ok(mut request) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if request["key"].as_str() != Some(key.as_str()) {
                continue;
            }
            if let Some(map) = request.as_object_mut() {
                map.remove("key");
            }
            on_message(request);
        }
    });
    Ok(())
}

/// Sends files to the running Hey. Returns false if it isn't running.
pub fn send(paths: &[String]) -> bool {
    request(json!({"cmd": "drop", "paths": paths}))
}

/// Asks the running Hey to show its window. Returns false if it isn't running.
pub fn show() -> bool {
    request(json!({"cmd": "show"}))
}

fn request(mut message: Value) -> bool {
    let mut send = || -> Option<()> {
        let info: Value = serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()?;
        let port = info["port"].as_u64()? as u16;
        let mut stream =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))
                .ok()?;
        message["key"] = info["key"].clone();
        stream.write_all(format!("{message}\n").as_bytes()).ok()
    };
    send().is_some()
}
