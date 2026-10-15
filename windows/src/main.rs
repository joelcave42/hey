#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Hey for Windows: lives in the system tray and receives links and text
//! from paired Android phones. Whatever arrives is copied to the clipboard
//! and shown in a notification; links get an Open button.

mod config;
mod icon;
mod pairing;
mod server;
mod winutil;

use server::{Event, Shared};
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

const RECENT_LIMIT: usize = 8;

fn main() {
    let _instance = unsafe { CreateMutexW(None, true, w!(r"Local\HeyTraySingleInstance")) };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    let icon_path = config::dir().join("icon.png");
    let _ = std::fs::write(&icon_path, icon::png(128));
    winutil::register_app_id(&icon_path);

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
            winutil::notify(
                "Hey couldn't start",
                &format!("It couldn't listen for your phone: {e}"),
                None,
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

    let mut recent_items: Vec<(MenuItem, String)> = Vec::new();
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
            } else if let Some((_, text)) = recent_items.iter().find(|(item, _)| *item.id() == id) {
                copy(text);
                if let Some(link) = winutil::find_link(text) {
                    winutil::open(link);
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
                    winutil::notify(
                        &title,
                        &format!("{}\nCopied to your clipboard.", preview(&text, 180)),
                        link,
                    );

                    if recent_items.is_empty() {
                        let _ = recent.remove(&empty);
                    }
                    let item = MenuItem::new(preview(&text, 48), true, None);
                    let _ = recent.prepend(&item);
                    recent_items.insert(0, (item, text));
                    if recent_items.len() > RECENT_LIMIT {
                        let (old, _) = recent_items.pop().unwrap();
                        let _ = recent.remove(&old);
                    }
                }
                Event::Paired { device } => {
                    phones.set_text(phone_count(&shared));
                    winutil::notify(
                        &format!("Paired with {device}"),
                        "Share anything to Hey on your phone and it shows up here.",
                        None,
                    );
                }
            }
        }

        std::thread::sleep(Duration::from_millis(40));
    }
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
