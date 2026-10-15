//! The pairing page: a QR code the phone scans once. It carries this PC's
//! addresses, its certificate fingerprint (so the phone can't be fooled by
//! another machine) and a one-time key that expires after ten minutes.

use crate::config::{self, Config};
use qrcode::QrCode;
use qrcode::render::svg;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

pub fn link(config: &Config, port: u16, key: &str) -> String {
    let ips: Vec<String> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_link_local() => Some(ip.to_string()),
            _ => None,
        })
        .collect();
    format!(
        "hey://pair?v=1&id={}&name={}&port={port}&fp={}&key={key}&ip={}",
        config.pc_id,
        percent(&config::pc_name()),
        config.fingerprint(),
        ips.join(",")
    )
}

fn percent(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Serves the current pairing page to this PC's browser only, on 127.0.0.1.
/// A local web address opens reliably where a file path can't (some browsers
/// break on paths with spaces, like C:\Users\Jane Doe). Returns the URL.
pub fn serve(page: Arc<Mutex<String>>) -> std::io::Result<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let url = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let icon = crate::icon::png(128);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut request = [0u8; 2048];
            let n = stream.read(&mut request).unwrap_or(0);
            let path = String::from_utf8_lossy(&request[..n])
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .to_string();
            let (kind, body) = if path == "/icon.png" {
                ("image/png", icon.clone())
            } else {
                (
                    "text/html; charset=utf-8",
                    page.lock().unwrap().clone().into_bytes(),
                )
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Ok(url)
}

/// Builds the pairing page for a pairing link.
pub fn page(link: &str) -> String {
    let qr = QrCode::new(link.as_bytes())
        .map(|code| {
            code.render::<svg::Color>()
                .min_dimensions(280, 280)
                .quiet_zone(true)
                .dark_color(svg::Color("#111318"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default();
    let name = html(&config::pc_name());
    PAGE.replace("{{QR}}", &qr)
        .replace("{{NAME}}", &name)
        .replace("{{LINK}}", &html(link))
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Pair a phone with Hey</title>
<link rel="icon" href="icon.png">
<style>
:root{--bg:#f4f4f8;--card:#fff;--text:#14151a;--muted:#5d6070;--brand:#5b5bf6;--line:#e4e4ec}
@media (prefers-color-scheme:dark){:root{--bg:#0f1015;--card:#191a22;--text:#f2f2f6;--muted:#9a9cab;--line:#2a2b36}}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--text);
font:16px/1.5 "Segoe UI Variable","Segoe UI",system-ui,sans-serif;padding:24px}
.card{background:var(--card);border:1px solid var(--line);border-radius:20px;padding:36px;max-width:440px;width:100%;
text-align:center;box-shadow:0 10px 40px rgba(20,20,40,.08)}
.logo{width:56px;height:56px;border-radius:14px}
h1{font-size:24px;margin:12px 0 4px}
p{color:var(--muted);margin:0 0 20px}
.qr{background:#fff;border-radius:14px;padding:10px;display:inline-block;line-height:0}
.qr svg{width:280px;height:280px}
ol{text-align:left;color:var(--muted);padding-left:22px;margin:22px 0 0}
li{margin:6px 0}
strong{color:var(--text)}
details{margin-top:18px;text-align:left;color:var(--muted);font-size:13px}
code{display:block;word-break:break-all;background:var(--bg);padding:10px;border-radius:8px;margin-top:8px;user-select:all}
</style></head><body>
<main class="card">
<img class="logo" src="icon.png" alt="">
<h1>Pair a phone</h1>
<p>with <strong>{{NAME}}</strong></p>
<div class="qr">{{QR}}</div>
<ol>
<li>Open <strong>Hey</strong> on your Android phone.</li>
<li>Tap <strong>Scan pairing code</strong> and point it at this code.</li>
<li>That's it. Share anything to <strong>Hey</strong> and it lands here.</li>
</ol>
<details><summary>Can't scan?</summary>Tap <strong>Paste pairing link</strong> in Hey and paste this:<code>{{LINK}}</code></details>
<p style="margin:18px 0 0;font-size:13px">This code works once and expires in 10 minutes.</p>
</main></body></html>"#;
