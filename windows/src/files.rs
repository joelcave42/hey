//! Files and photos from the phone: streamed into Downloads\Hey, with a
//! thumbnail for the notification and, for a single photo, a copy on the
//! clipboard ready to paste.

use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// Where received files are saved: the folder chosen in settings, or
/// Downloads\Hey. Falls back to the default if the chosen one is gone (an
/// unplugged drive, say).
pub fn folder() -> PathBuf {
    if let Some(custom) = crate::config::custom_save_folder()
        && (custom.is_dir() || fs::create_dir_all(&custom).is_ok())
    {
        return custom;
    }
    default_folder()
}

pub fn default_folder() -> PathBuf {
    let dir = dirs::download_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Downloads"))
        .join("Hey");
    let _ = fs::create_dir_all(&dir);
    dir
}

/// Reads exactly `size` bytes into a new file in Downloads\Hey. The file only
/// appears under its real name once it has fully arrived.
pub fn receive(reader: &mut impl Read, name: &str, size: u64) -> io::Result<PathBuf> {
    let target = unique_path(&folder(), &safe_name(name));
    let partial = target.with_extension(format!(
        "{}.part",
        target.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let result = (|| {
        let mut out = BufWriter::new(File::create(&partial)?);
        let copied = io::copy(&mut reader.take(size), &mut out)?;
        out.flush()?;
        if copied != size {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the phone stopped sending partway through",
            ));
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            fs::rename(&partial, &target)?;
            Ok(target)
        }
        Err(e) => {
            let _ = fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// A file name that is safe to create on Windows, whatever the phone sent.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = cleaned.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    match cleaned.as_str() {
        "" => "file".to_string(),
        _ if reserved => format!("_{cleaned}"),
        _ => cleaned.chars().take(150).collect(),
    }
}

/// `photo.jpg`, then `photo (1).jpg`, `photo (2).jpg`, ...
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (1..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp")
    )
}

pub fn kind(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" => "Photo",
        "mp4" | "mov" | "mkv" | "webm" | "3gp" | "avi" => "Video",
        "mp3" | "m4a" | "wav" | "ogg" | "opus" | "flac" | "aac" => "Audio",
        "pdf" => "PDF",
        _ => "File",
    }
}

/// Saves a small JPEG of a photo for the notification and the Hey window.
/// Windows won't show big images in a notification, and phone photos are big.
pub fn thumbnail_to(path: &Path, out: &Path) -> bool {
    image::open(path)
        .map(|image| image.thumbnail(720, 720).to_rgb8().save(out).is_ok())
        .unwrap_or(false)
}

/// Puts a photo on the clipboard so it can be pasted straight into Discord,
/// Paint, a document...
pub fn copy_image(path: &Path) -> bool {
    let Ok(image) = image::open(path) else {
        return false;
    };
    if u64::from(image.width()) * u64::from(image.height()) > 50_000_000 {
        return false;
    }
    let rgba = image.to_rgba8();
    let data = arboard::ImageData {
        width: rgba.width() as usize,
        height: rgba.height() as usize,
        bytes: rgba.into_raw().into(),
    };
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_image(data))
        .is_ok()
}

/// What's on the PC's clipboard, for the Clipboard button.
pub enum Clipboard {
    /// Files copied in File Explorer.
    Files(Vec<PathBuf>),
    Text(String),
    /// A picture (like a Win+Shift+S snip), saved as a PNG to send.
    Image(PathBuf),
    Empty,
}

/// Reads the clipboard: copied files first, then text, then a picture. Text
/// comes before pictures because apps like Excel copy both, and the text is
/// what you meant.
pub fn read_clipboard() -> Clipboard {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return Clipboard::Empty;
    };
    if let Ok(files) = clipboard.get().file_list() {
        let files: Vec<_> = files.into_iter().filter(|p| p.is_file()).collect();
        if !files.is_empty() {
            return Clipboard::Files(files);
        }
    }
    if let Ok(text) = clipboard.get_text()
        && !text.trim().is_empty()
    {
        return Clipboard::Text(text.trim().to_string());
    }
    if let Ok(image) = clipboard.get_image()
        && let Some(path) = save_clipboard_image(image)
    {
        return Clipboard::Image(path);
    }
    Clipboard::Empty
}

/// Saves a clipboard picture as "Screenshot 2026-10-15 19.52.08.png" in
/// Hey's own folder, clearing out ones more than a week old.
fn save_clipboard_image(image: arboard::ImageData) -> Option<PathBuf> {
    let dir = crate::config::dir().join("outgoing");
    fs::create_dir_all(&dir).ok()?;
    let week = std::time::Duration::from_secs(7 * 24 * 60 * 60);
    for old in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let stale = old
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > week);
        if stale {
            let _ = fs::remove_file(old.path());
        }
    }
    let buffer = image::RgbaImage::from_raw(
        image.width as u32,
        image.height as u32,
        image.bytes.into_owned(),
    )?;
    let path = dir.join(format!("Screenshot {}.png", local_timestamp()));
    buffer.save(&path).ok()?;
    Some(path)
}

fn local_timestamp() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{}-{:02}-{:02} {:02}.{:02}.{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

pub fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{} KB", b >> 10),
        b => format!("{b} bytes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_names_safe() {
        assert_eq!(safe_name("IMG_2041.jpg"), "IMG_2041.jpg");
        assert_eq!(safe_name("../../Windows/evil.exe"), "evil.exe");
        assert_eq!(safe_name(r"C:\x\y.txt"), "y.txt");
        assert_eq!(safe_name("what?.pdf"), "what_.pdf");
        assert_eq!(safe_name("CON.txt"), "_CON.txt");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name(""), "file");
    }

    #[test]
    fn receives_exact_bytes_and_numbers_duplicates() {
        let dir = std::env::temp_dir().join(format!("hey-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = unique_path(&dir, "a.txt");
        fs::write(&a, "x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt").file_name().unwrap(), "a (1).txt");
        fs::remove_dir_all(&dir).unwrap();
    }
}
