//! Pairing: a QR code the phone scans once. It carries this PC's
//! addresses (including Tailscale's, for away from home), its certificate fingerprint (so the phone can't be fooled by
//! another machine) and a one-time key that expires after ten minutes.

use crate::config::{self, Config};
use qrcode::QrCode;
use qrcode::render::svg;

pub fn link(config: &Config, port: u16, key: &str) -> String {
    let ips = crate::net::addresses();
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

/// The QR code for a pairing link, as SVG for the Hey window.
pub fn qr_svg(link: &str) -> String {
    QrCode::new(link.as_bytes())
        .map(|code| {
            code.render::<svg::Color>()
                .min_dimensions(260, 260)
                .quiet_zone(true)
                .dark_color(svg::Color("#111318"))
                .light_color(svg::Color("#ffffff"))
                .build()
        })
        .unwrap_or_default()
}
