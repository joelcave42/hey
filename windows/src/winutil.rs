//! Small Windows helpers: notifications, opening links, and the registry.

use std::path::{Path, PathBuf};
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyW, RegDeleteKeyValueW,
    RegGetValueW, RegSetValueExW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

/// Notifications from an app without an installer need an app ID registered
/// under HKCU, which also gives them Hey's name and icon.
pub const APP_ID: &str = "joelcave42.Hey";
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

pub fn register_app_id(icon: &Path) {
    let key = format!(r"Software\Classes\AppUserModelId\{APP_ID}");
    set_string(&key, "DisplayName", "Hey");
    set_string(&key, "IconUri", &icon.to_string_lossy());
}

fn set_string(key: &str, name: &str, value: &str) {
    unsafe {
        let mut hkey = HKEY::default();
        if RegCreateKeyW(HKEY_CURRENT_USER, &HSTRING::from(key), &mut hkey) != ERROR_SUCCESS {
            return;
        }
        let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let _ = RegSetValueExW(hkey, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
    }
}

pub fn starts_with_windows() -> bool {
    let mut size = 0u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            w!("Hey"),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        ) == ERROR_SUCCESS
    }
}

pub fn set_start_with_windows(on: bool) {
    if on {
        if let Ok(exe) = std::env::current_exe() {
            set_string(
                r"Software\Microsoft\Windows\CurrentVersion\Run",
                "Hey",
                &format!("\"{}\"", exe.display()),
            );
        }
    } else {
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Hey"));
        }
    }
}

/// Opens a URL or file with its default app.
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}

/// A notification. Buttons and clicks open a URI: a web link, or a
/// `hey-action:` link that Windows hands back to Hey (see `register_actions`).
#[derive(Default)]
pub struct Toast {
    pub title: String,
    pub body: String,
    /// A local image shown large in the notification.
    pub image: Option<PathBuf>,
    /// What clicking the notification itself opens.
    pub launch: Option<String>,
    /// Up to five (label, URI) buttons.
    pub buttons: Vec<(String, String)>,
}

pub fn notify(toast: &Toast) {
    let launch = toast
        .launch
        .as_ref()
        .map(|uri| format!(r#" activationType="protocol" launch="{}""#, xml(uri)))
        .unwrap_or_default();
    let image = toast
        .image
        .as_ref()
        .map(|p| {
            format!(
                r#"<image placement="hero" src="{}"/>"#,
                xml(&p.to_string_lossy())
            )
        })
        .unwrap_or_default();
    let buttons: String = toast
        .buttons
        .iter()
        .take(5)
        .map(|(label, uri)| {
            format!(
                r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                xml(label),
                xml(uri)
            )
        })
        .collect();
    let actions = if buttons.is_empty() {
        String::new()
    } else {
        format!("<actions>{buttons}</actions>")
    };
    let body = if toast.body.is_empty() {
        String::new()
    } else {
        format!("<text>{}</text>", xml(&toast.body))
    };
    let doc = format!(
        r#"<toast{launch}><visual><binding template="ToastGeneric"><text>{}</text>{body}{image}</binding></visual>{actions}</toast>"#,
        xml(&toast.title)
    );
    let show = || -> windows::core::Result<()> {
        let xml_doc = XmlDocument::new()?;
        xml_doc.LoadXml(&HSTRING::from(doc))?;
        let notification = ToastNotification::CreateToastNotification(&xml_doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
            .Show(&notification)
    };
    if let Err(e) = show() {
        eprintln!("notification failed: {e}");
    }
}

/// Shorthand for a plain notification with just a title and body.
pub fn notify_text(title: &str, body: &str) {
    notify(&Toast {
        title: title.into(),
        body: body.into(),
        ..Default::default()
    });
}

/// Registers the `hey-action:` link type, so notification buttons can ask
/// Hey to copy something or show a file. Windows starts `hey.exe --action
/// "<link>"`, which does the job and exits.
pub fn register_actions() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let key = r"Software\Classes\hey-action";
    set_string(key, "", "URL:Hey action");
    set_string(key, "URL Protocol", "");
    set_string(
        &format!(r"{key}\shell\open\command"),
        "",
        &format!("\"{}\" --action \"%1\"", exe.display()),
    );
}

/// Shows a file selected in File Explorer.
pub fn reveal(path: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", path.display()))
        .spawn();
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The first http(s) link in some text, so "Join Zoom: https://..." still
/// gets an Open button.
pub fn find_link(text: &str) -> Option<&str> {
    let start = text.find("https://").or_else(|| text.find("http://"))?;
    let rest = &text[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let link = rest[..end].trim_end_matches(['.', ',', ')', '>', '"', '\'']);
    (link.len() > "https://".len()).then_some(link)
}

#[cfg(test)]
mod tests {
    use super::find_link;

    #[test]
    fn finds_links_inside_text() {
        assert_eq!(
            find_link("Join Zoom Meeting https://us02web.zoom.us/j/123?pwd=abc."),
            Some("https://us02web.zoom.us/j/123?pwd=abc")
        );
        assert_eq!(find_link("http://example.com"), Some("http://example.com"));
        assert_eq!(find_link("no link here"), None);
    }
}
