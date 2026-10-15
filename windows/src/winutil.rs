//! Small Windows helpers: notifications, opening links, and the registry.

use std::path::Path;
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
pub const APP_ID: &str = "JoelCave.Hey";
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

pub fn notify(title: &str, body: &str, link: Option<&str>) {
    let (launch, actions) = match link {
        Some(url) => (
            format!(r#" activationType="protocol" launch="{}""#, xml(url)),
            format!(
                r#"<actions><action content="Open link" activationType="protocol" arguments="{}"/></actions>"#,
                xml(url)
            ),
        ),
        None => (String::new(), String::new()),
    };
    let toast = format!(
        r#"<toast{launch}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual>{actions}</toast>"#,
        xml(title),
        xml(body)
    );
    let show = || -> windows::core::Result<()> {
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(toast))?;
        let notification = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))?
            .Show(&notification)
    };
    if let Err(e) = show() {
        eprintln!("notification failed: {e}");
    }
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
