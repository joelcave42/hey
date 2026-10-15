#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Hey for Windows: lives in the system tray and receives links, text,
//! photos and files from paired Android phones. Text and single photos are
//! copied to the clipboard; everything is shown in a notification.

mod config;
mod files;
mod icon;
mod pairing;
mod server;
mod url;
mod winutil;

use server::{Event, Shared};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};
use windows::core::w;
use winutil::Toast;

const RECENT_LIMIT: usize = 8;

enum Recent {
    Text(String),
    File(PathBuf),
}

fn main() {
    // A notification button was clicked: do that one thing and exit.
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--action" {
        run_action(&args[2]);
        return;
    }

    let _instance = unsafe { CreateMutexW(None, true, w!(r"Local\HeyTraySingleInstance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    let icon_path = config::dir().join("icon.png");
    let _ = std::fs::write(&icon_path, icon::png(128));
    winutil::register_app_id(&icon_path);
    winutil::register_actions();

    let config = config::Config::load_or_create();
    let first_run = config.devices.is_empty();
    let pc_id = config.pc_id.clone();
    let shared = Arc::new(Shared {
        config: Mutex::new(config),
        pending_pair: Mutex::new(None),
    });

    let (events_tx, events) = mpsc::channel();
    let port = match server::start(shared.clone(), events_tx) {
        Ok(port) => port,
        Err(e) => {
            winutil::notify_text(
                "Hey couldn't start",
                &format!("It couldn't listen for your phone: {e}"),
            );
            return;
        }
    };
    let _mdns = server::advertise(&pc_id, port);

    // Tray menu
    let menu = Menu::new();
    let header = MenuItem::new(format!("Hey · {}", config::pc_name()), false, None);
    let pair = MenuItem::new("Pair a phone…", true, None);
    let recent = Submenu::new("Recent", true);
    let open_folder = MenuItem::new("Open Hey folder", true, None);
    let empty = MenuItem::new("Nothing yet", false, None);
    let phones = MenuItem::new(phone_count(&shared), false, None);
    let forget = MenuItem::new("Forget all phones", true, None);
    let autostart = CheckMenuItem::new(
        "Start with Windows",
        true,
        winutil::starts_with_windows(),
        None,
    );
    let quit = MenuItem::new("Quit Hey", true, None);
    let _ = recent.append(&empty);
    let _ = menu.append_items(&[
        &header,
        &PredefinedMenuItem::separator(),
        &pair,
        &recent,
        &open_folder,
        &PredefinedMenuItem::separator(),
        &phones,
        &forget,
        &autostart,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(true)
        .with_tooltip("Hey: share from your phone to this PC")
        .with_icon(Icon::from_rgba(icon::rgba(32), 32, 32).expect("tray icon"))
        .build()
        .expect("could not create the tray icon");

    let pairing_page = Arc::new(Mutex::new(String::new()));
    let pairing_url = pairing::serve(pairing_page.clone()).ok();
    let open_pairing = || {
        let key = config::random_token(18);
        *shared.pending_pair.lock().unwrap() = Some((key.clone(), Instant::now()));
        let config = shared.config.lock().unwrap();
        *pairing_page.lock().unwrap() = pairing::page(&pairing::link(&config, port, &key));
        if let Some(url) = &pairing_url {
            winutil::open(url);
        }
    };

    if first_run {
        open_pairing();
    }

    let mut recent_items: Vec<(MenuItem, Recent)> = Vec::new();
    // Files from a batch the phone is still sending, per device.
    let mut batches: HashMap<String, Vec<PathBuf>> = HashMap::new();
    loop {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = event.id;
            if id == *pair.id() {
                open_pairing();
            } else if id == *open_folder.id() {
                winutil::open(&files::folder().to_string_lossy());
            } else if id == *forget.id() {
                let mut config = shared.config.lock().unwrap();
                config.devices.clear();
                config.save();
                drop(config);
                phones.set_text(phone_count(&shared));
            } else if id == *autostart.id() {
                winutil::set_start_with_windows(autostart.is_checked());
            } else if id == *quit.id() {
                return;
            } else if let Some((_, entry)) = recent_items.iter().find(|(item, _)| *item.id() == id)
            {
                match entry {
                    Recent::Text(text) => {
                        copy(text);
                        if let Some(link) = winutil::find_link(text) {
                            winutil::open(link);
                        }
                    }
                    Recent::File(path) => winutil::reveal(path),
                }
            }
        }

        while let Ok(event) = events.try_recv() {
            match event {
                Event::Received { device, text } => {
                    copy(&text);
                    let link = winutil::find_link(&text);
                    let title = if link.is_some() {
                        format!("Link from {device}")
                    } else {
                        format!("From {device}")
                    };
                    winutil::notify(&Toast {
                        title,
                        body: format!("{}\nCopied to your clipboard.", preview(&text, 180)),
                        launch: link.map(str::to_string),
                        buttons: link
                            .map(|l| vec![("Open link".to_string(), l.to_string())])
                            .unwrap_or_default(),
                        ..Default::default()
                    });
                    add_recent(
                        &recent,
                        &empty,
                        &mut recent_items,
                        preview(&text, 48),
                        Recent::Text(text),
                    );
                }
                Event::File {
                    device,
                    path,
                    index,
                    count,
                } => {
                    let label =
                        format!("{}  {}", files::kind(&path), preview(&file_name(&path), 40));
                    add_recent(
                        &recent,
                        &empty,
                        &mut recent_items,
                        label,
                        Recent::File(path.clone()),
                    );
                    batches.entry(device.clone()).or_default().push(path);
                    if index + 1 >= count {
                        let batch = batches.remove(&device).unwrap_or_default();
                        notify_files(&device, &batch);
                    }
                }
                Event::Paired { device } => {
                    phones.set_text(phone_count(&shared));
                    winutil::notify_text(
                        &format!("Paired with {device}"),
                        "Share anything to Hey on your phone and it shows up here.",
                    );
                }
            }
        }

        std::thread::sleep(Duration::from_millis(40));
    }
}

fn add_recent(
    menu: &Submenu,
    empty: &MenuItem,
    items: &mut Vec<(MenuItem, Recent)>,
    label: String,
    entry: Recent,
) {
    if items.is_empty() {
        let _ = menu.remove(empty);
    }
    let item = MenuItem::new(label, true, None);
    let _ = menu.prepend(&item);
    items.insert(0, (item, entry));
    if items.len() > RECENT_LIMIT {
        let (old, _) = items.pop().unwrap();
        let _ = menu.remove(&old);
    }
}

/// One notification for whatever the phone just sent: a single file gets a
/// preview and Open buttons, a batch gets a summary.
fn notify_files(device: &str, batch: &[PathBuf]) {
    let Some(last) = batch.last() else { return };
    let image = if files::is_image(last) {
        files::thumbnail(last)
    } else {
        None
    };
    let folder = action("folder", None);

    let toast = if let [path] = batch {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let copied = files::is_image(path) && files::copy_image(path);
        let mut body = format!("{} · {}", file_name(path), files::human_size(size));
        if copied {
            body.push_str("\nCopied to your clipboard. Paste it anywhere.");
        }
        Toast {
            title: format!("{} from {device}", files::kind(path)),
            body,
            image,
            launch: Some(action("open", Some(path))),
            buttons: vec![
                ("Open".into(), action("open", Some(path))),
                ("Show in folder".into(), action("reveal", Some(path))),
            ],
        }
    } else {
        let kinds: Vec<&str> = batch.iter().map(|p| files::kind(p)).collect();
        let same = kinds.iter().all(|k| *k == kinds[0]);
        let noun = match kinds[0] {
            "Photo" if same => "photos",
            "Video" if same => "videos",
            _ => "files",
        };
        Toast {
            title: format!("{} {noun} from {device}", batch.len()),
            body: "Saved to Downloads\\Hey.".into(),
            image,
            launch: Some(folder.clone()),
            buttons: vec![("Open folder".into(), folder)],
        }
    };
    winutil::notify(&toast);
}

/// A `hey-action:` link for a notification button.
fn action(name: &str, path: Option<&Path>) -> String {
    match path {
        Some(p) => format!(
            "hey-action:{name}?path={}",
            url::encode(&p.to_string_lossy())
        ),
        None => format!("hey-action:{name}"),
    }
}

/// Runs a `hey-action:` link. Anything can launch these links (a web page
/// could, after the browser asks), so file actions only touch files inside
/// Downloads\\Hey.
fn run_action(link: &str) {
    let name = link
        .strip_prefix("hey-action:")
        .unwrap_or("")
        .split(['?', '/'])
        .next()
        .unwrap_or("");
    let path = url::query_param(link, "path").map(PathBuf::from);
    let inside_hey = |p: &Path| {
        let folder = files::folder().canonicalize().ok();
        let file = p.canonicalize().ok();
        matches!((folder, file), (Some(f), Some(p)) if p.starts_with(&f))
    };
    match (name, path) {
        ("copy", _) => {
            if let Some(text) = url::query_param(link, "text") {
                copy(&text);
            }
        }
        ("open", Some(p)) if inside_hey(&p) => winutil::open(&p.to_string_lossy()),
        ("reveal", Some(p)) if inside_hey(&p) => winutil::reveal(&p),
        ("folder", _) => winutil::open(&files::folder().to_string_lossy()),
        _ => {}
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn copy(text: &str) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_text(text.to_string());
    }
}

fn preview(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max - 1).collect::<String>())
    } else {
        flat
    }
}

fn phone_count(shared: &Shared) -> String {
    match shared.config.lock().unwrap().devices.len() {
        0 => "No phones paired".to_string(),
        1 => "1 phone paired".to_string(),
        n => format!("{n} phones paired"),
    }
}
