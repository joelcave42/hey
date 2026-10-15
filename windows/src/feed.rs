//! Everything phones have sent this PC, newest first, for the Hey window.
//! Saved as feed.json next to the config, with photo thumbnails in thumbs\.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const LIMIT: usize = 300;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Item {
    pub id: u64,
    /// text, link, code, photo, video, audio, pdf or file
    pub kind: String,
    pub device: String,
    /// Milliseconds since 1970.
    pub time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default)]
    pub thumb: bool,
    /// Sent from this PC to the phone, rather than received.
    #[serde(default)]
    pub outgoing: bool,
    /// For things sent to the phone: sending, sent or failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Feed {
    pub items: Vec<Item>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn path() -> PathBuf {
    crate::config::dir().join("feed.json")
}

pub fn thumbs_dir() -> PathBuf {
    let dir = crate::config::dir().join("thumbs");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn thumb_path(id: u64) -> PathBuf {
    thumbs_dir().join(format!("{id}.jpg"))
}

impl Feed {
    pub fn load() -> Feed {
        let items = fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Feed { items }
    }

    fn save(&self) {
        if let Ok(text) = serde_json::to_string(&self.items) {
            let _ = fs::write(path(), text);
        }
    }

    fn next_id(&self) -> u64 {
        now_ms().max(self.items.first().map(|i| i.id + 1).unwrap_or(0))
    }

    pub fn add_text(&mut self, device: &str, text: &str, found: &crate::actions::Found) -> u64 {
        let kind = match (&found.code, &found.link) {
            (Some(_), _) => "code",
            (None, Some(_)) => "link",
            _ => "text",
        };
        self.push(Item {
            kind: kind.into(),
            device: device.into(),
            text: Some(text.into()),
            link: found.link.clone(),
            code: found.code.clone(),
            ..Default::default()
        })
    }

    pub fn add_file(&mut self, device: &str, file: &Path) -> u64 {
        let size = fs::metadata(file).map(|m| m.len()).ok();
        let kind = match crate::files::kind(file) {
            "Photo" => "photo",
            "Video" => "video",
            "Audio" => "audio",
            "PDF" => "pdf",
            _ => "file",
        };
        let id = self.push(Item {
            kind: kind.into(),
            device: device.into(),
            path: Some(file.to_string_lossy().into_owned()),
            name: file.file_name().map(|n| n.to_string_lossy().into_owned()),
            size,
            ..Default::default()
        });
        if crate::files::is_image(file) && crate::files::thumbnail_to(file, &thumb_path(id)) {
            if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
                item.thumb = true;
            }
            self.save();
        }
        id
    }

    /// Records something on its way to a phone.
    pub fn add_outgoing_text(&mut self, device: &str, text: &str) -> u64 {
        let found = crate::actions::find(text);
        let id = self.add_text(device, text, &found);
        self.mark_outgoing(id);
        id
    }

    pub fn add_outgoing_file(&mut self, device: &str, file: &Path) -> u64 {
        let id = self.add_file(device, file);
        self.mark_outgoing(id);
        id
    }

    fn mark_outgoing(&mut self, id: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.outgoing = true;
            item.status = Some("sending".into());
        }
        self.save();
    }

    pub fn set_delivered(&mut self, id: u64, error: Option<String>) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.status = Some(if error.is_some() { "failed" } else { "sent" }.into());
            item.error = error;
        }
        self.save();
    }

    fn push(&mut self, mut item: Item) -> u64 {
        item.id = self.next_id();
        item.time = now_ms();
        let id = item.id;
        self.items.insert(0, item);
        while self.items.len() > LIMIT {
            if let Some(old) = self.items.pop() {
                let _ = fs::remove_file(thumb_path(old.id));
            }
        }
        self.save();
        id
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Removes an item from the list. Received files stay in Downloads\Hey.
    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
        let _ = fs::remove_file(thumb_path(id));
        self.save();
    }

    pub fn clear(&mut self) {
        for item in self.items.drain(..) {
            let _ = fs::remove_file(thumb_path(item.id));
        }
        self.save();
    }
}
