//! The pairing page: a QR code the phone scans once. It carries this PC's
//! addresses, its certificate fingerprint (so the phone can't be fooled by
//! another machine) and a one-time key that expires after ten minutes.

use crate::config::{self, Config};
use qrcode::QrCode;
use qrcode::render::svg;
use std::path::PathBuf;

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

/// Writes the pairing page next to the config and returns its path.
pub fn write_page(link: &str) -> PathBuf {
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
    let page = PAGE
        .replace("{{QR}}", &qr)
        .replace("{{NAME}}", &name)
        .replace("{{LINK}}", &html(link));
    let path = config::dir().join("pair.html");
    let _ = std::fs::write(&path, page);
    path
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
