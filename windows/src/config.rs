//! Everything Hey remembers between runs: this PC's identity, its TLS key and
//! the phones it is paired with. Stored as JSON in %APPDATA%\Hey.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub name: String,
    /// SHA-256 of the device's secret token, so the token itself never sits on disk.
    pub token_hash: String,
    /// When this phone last talked to the PC, in milliseconds since 1970.
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub pc_id: String,
    pub cert_der: String,
    pub key_der: String,
    #[serde(default)]
    pub devices: Vec<Device>,
    /// Open links in the browser as soon as they arrive.
    #[serde(default)]
    pub auto_open_links: bool,
    /// Where received files go, if not the default Downloads\Hey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_folder: Option<String>,
}

pub fn dir() -> PathBuf {
    let d = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Hey");
    let _ = fs::create_dir_all(&d);
    d
}

fn path() -> PathBuf {
    dir().join("config.json")
}

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(s).unwrap_or_default()
}

pub fn random_token(len: usize) -> String {
    let mut buf = vec![0u8; len];
    getrandom::fill(&mut buf).expect("system random number generator failed");
    b64(&buf)
}

pub fn hash(s: &str) -> String {
    b64(&Sha256::digest(s.as_bytes()))
}

impl Config {
    pub fn load_or_create() -> Config {
        if let Ok(text) = fs::read_to_string(path())
            && let Ok(config) = serde_json::from_str::<Config>(&text)
        {
            return config;
        }
        let key = rcgen::generate_simple_self_signed(vec!["hey.local".to_string()])
            .expect("could not create this PC's certificate");
        let config = Config {
            pc_id: random_token(9),
            cert_der: b64(key.cert.der()),
            key_der: b64(&key.signing_key.serialize_der()),
            devices: Vec::new(),
            auto_open_links: false,
            save_folder: None,
        };
        config.save();
        config
    }

    pub fn save(&self) {
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path(), text);
        }
    }

    /// SHA-256 of the certificate, which the phone pins after scanning the QR code.
    pub fn fingerprint(&self) -> String {
        b64(&Sha256::digest(unb64(&self.cert_der)))
    }
}

/// The custom save folder from the saved settings, read fresh each time so
/// the notification-button helper process sees changes too.
pub fn custom_save_folder() -> Option<PathBuf> {
    #[derive(Deserialize)]
    struct Only {
        save_folder: Option<String>,
    }
    let text = fs::read_to_string(path()).ok()?;
    let only: Only = serde_json::from_str(&text).ok()?;
    only.save_folder.map(PathBuf::from)
}

pub fn pc_name() -> String {
    gethostname::gethostname().to_string_lossy().into_owned()
}
