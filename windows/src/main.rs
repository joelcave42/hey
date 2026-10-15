#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Hey for Windows: receives links, text, photos and files from paired
//! Android phones. It lives in the tray; clicking the icon opens the Hey
//! window with everything received. Text and single photos are copied to the
//! clipboard and everything gets a notification.

mod actions;
mod config;
mod feed;
mod files;
mod icon;
mod pairing;
mod server;
mod url;
mod window;
mod winutil;

use feed::Feed;
use serde_json::{Value, json};
use server::{Event, Shared};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;
use tao::event::{Event as WinEvent, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Threading::CreateMutexW;
use windows::core::w;
use winutil::Toast;

const PAIRING_MINUTES: u64 = 10;

/// Everything that wakes the event loop up.
enum UserEvent {
    Server(Event),
    Menu(MenuEvent),
    Tray(TrayIconEvent),
    Page(String),
}

struct App {
    shared: Arc<Shared>,
    feed: Feed,
    port: u16,
    /// The pairing link on screen, and when it expires (ms since 1970).
    pairing: Option<(String, u64)>,
    /// Files from a batch a phone is still sending: (path, feed id).
    batches: HashMap<String, Vec<(PathBuf, u64)>>,
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
        // Hey is already running: bring its window up instead.
        winutil::show_running_window();
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

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

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
    let forward = proxy.clone();
    std::thread::spawn(move || {
        for event in events {
            let _ = forward.send_event(UserEvent::Server(event));
        }
    });
    let forward = proxy.clone();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = forward.send_event(UserEvent::Menu(e));
    }));
    let forward = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = forward.send_event(UserEvent::Tray(e));
    }));

    let menu = Menu::new();
    let open = MenuItem::new("Open Hey", true, None);
    let pair = MenuItem::new("Pair a phone…", true, None);
    let quit = MenuItem::new("Quit Hey", true, None);
    let _ = menu.append_items(&[&open, &pair, &PredefinedMenuItem::separator(), &quit]);
    let mut tray = Some(
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Hey")
            .with_icon(Icon::from_rgba(icon::rgba(32), 32, 32).expect("tray icon"))
            .build()
            .expect("could not create the tray icon"),
    );

    let forward = proxy.clone();
    let hey = window::HeyWindow::new(&event_loop, first_run, move |message| {
        let _ = forward.send_event(UserEvent::Page(message));
    })
    .expect("could not create the Hey window");

    let mut app = App {
        shared,
        feed: Feed::load(),
        port,
        pairing: None,
        batches: HashMap::new(),
    };
    if first_run {
        app.start_pairing();
    }

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            WinEvent::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Closing the window keeps Hey running in the tray.
                hey.window.set_visible(false);
                app.cancel_pairing();
            }
            WinEvent::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            })) => hey.show(),
            WinEvent::UserEvent(UserEvent::Menu(e)) => {
                if e.id == *open.id() {
                    hey.show();
                } else if e.id == *pair.id() {
                    app.start_pairing();
                    hey.show();
                    hey.update(&app.state());
                } else if e.id == *quit.id() {
                    tray.take();
                    *flow = ControlFlow::Exit;
                }
            }
            WinEvent::UserEvent(UserEvent::Server(event)) => {
                if let Some(message) = app.on_server(event) {
                    hey.toast(&message);
                }
                hey.update(&app.state());
            }
            WinEvent::UserEvent(UserEvent::Page(message)) => {
                if let Some(reply) = app.on_page(&message) {
                    hey.toast(&reply);
                }
                hey.update(&app.state());
            }
            _ => {}
        }
    });
}

impl App {
    fn start_pairing(&mut self) {
        let key = config::random_token(18);
        *self.shared.pending_pair.lock().unwrap() = Some((key.clone(), Instant::now()));
        let link = pairing::link(&self.shared.config.lock().unwrap(), self.port, &key);
        let expires = feed::now_ms() + PAIRING_MINUTES * 60 * 1000;
        self.pairing = Some((link, expires));
    }

    fn cancel_pairing(&mut self) {
        self.pairing = None;
        *self.shared.pending_pair.lock().unwrap() = None;
    }

    /// Everything the page shows.
    fn state(&self) -> Value {
        let config = self.shared.config.lock().unwrap();
        let mut devices: Vec<_> = config
            .devices
            .iter()
            .map(|d| json!({"name": d.name, "last_seen": d.last_seen}))
            .collect();
        devices.sort_by_key(|d| std::cmp::Reverse(d["last_seen"].as_u64()));
        json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pc": config::pc_name(),
            "devices": devices,
            "settings": {
                "auto_open": config.auto_open_links,
                "autostart": winutil::starts_with_windows(),
                "folder": files::folder().to_string_lossy(),
            },
            "items": self.feed.items,
            "pairing": self.pairing.as_ref().map(|(link, expires)| json!({
                "qr": pairing::qr_svg(link),
                "link": link,
                "expires": expires,
            })),
        })
    }

    /// Handles something a phone sent. Returns a message for the window.
    fn on_server(&mut self, event: Event) -> Option<String> {
        match event {
            Event::Received { device, text } => {
                let auto_open = self.shared.config.lock().unwrap().auto_open_links;
                let found = actions::find(&text);
                self.feed.add_text(&device, &text, &found);
                notify_text_received(&device, &text, &found, auto_open);
                None
            }
            Event::File {
                device,
                path,
                index,
                count,
            } => {
                let id = self.feed.add_file(&device, &path);
                self.batches
                    .entry(device.clone())
                    .or_default()
                    .push((path, id));
                if index + 1 >= count {
                    let batch = self.batches.remove(&device).unwrap_or_default();
                    notify_files(&device, &batch);
                }
                None
            }
            Event::Paired { device } => {
                self.pairing = None;
                winutil::notify_text(
                    &format!("Paired with {device}"),
                    "Share anything to Hey on your phone and it shows up here.",
                );
                Some(format!("Paired with {device}"))
            }
            Event::Seen => None,
        }
    }

    /// Handles a click in the window. Returns a message to show there.
    fn on_page(&mut self, message: &str) -> Option<String> {
        let request: Value = serde_json::from_str(message).ok()?;
        let cmd = request["cmd"].as_str().unwrap_or("");
        let item = request["id"]
            .as_u64()
            .and_then(|id| self.feed.get(id))
            .cloned();
        match (cmd, item) {
            ("ready", _) => None,
            ("pair", _) => {
                self.start_pairing();
                None
            }
            ("cancelPair", _) => {
                self.cancel_pairing();
                None
            }
            ("copyPairLink", _) => {
                let (link, _) = self.pairing.as_ref()?;
                copy(link);
                Some("Pairing link copied. Paste it in Hey on your phone.".into())
            }
            ("open", Some(item)) => {
                match (&item.link, &item.path) {
                    (Some(link), _) => winutil::open(link),
                    (None, Some(path)) if Path::new(path).exists() => winutil::open(path),
                    (None, Some(_)) => {
                        return Some("That file isn't in Downloads\\Hey anymore.".into());
                    }
                    _ => {}
                }
                None
            }
            ("copy", Some(item)) => match (&item.text, &item.path) {
                (Some(text), _) => {
                    copy(text);
                    Some("Copied".into())
                }
                (None, Some(path)) if files::copy_image(Path::new(path)) => {
                    Some("Photo copied. Paste it anywhere.".into())
                }
                _ => Some("Couldn't copy that.".into()),
            },
            ("copyCode", Some(item)) => {
                copy(item.code.as_deref()?);
                Some("Code copied".into())
            }
            ("reveal", Some(item)) => {
                winutil::reveal(Path::new(item.path.as_deref()?));
                None
            }
            ("clear", _) => {
                self.feed.clear();
                None
            }
            ("remove", Some(item)) => {
                self.feed.remove(item.id);
                None
            }
            ("openFolder", _) => {
                winutil::open(&files::folder().to_string_lossy());
                None
            }
            ("openUrl", _) => {
                let url = request["url"].as_str()?;
                if url.starts_with("https://github.com/joelcave42/") {
                    winutil::open(url);
                }
                None
            }
            ("set", _) => {
                let on = request["value"].as_bool().unwrap_or(false);
                match request["key"].as_str()? {
                    "auto_open" => {
                        let mut config = self.shared.config.lock().unwrap();
                        config.auto_open_links = on;
                        config.save();
                    }
                    "autostart" => winutil::set_start_with_windows(on),
                    _ => {}
                }
                None
            }
            ("forget", _) => {
                let name = request["name"].as_str()?;
                let mut config = self.shared.config.lock().unwrap();
                config.devices.retain(|d| d.name != name);
                config.save();
                Some(format!("Forgot {name}"))
            }
            _ => None,
        }
    }
}

/// Copies text from the phone and shows it with buttons that fit what it is.
/// A sign-in code puts just the code on the clipboard.
fn notify_text_received(device: &str, text: &str, found: &actions::Found, auto_open: bool) {
    let copy_link = |s: &str| format!("hey-action:copy?text={}", url::encode(s));
    copy(found.code.as_deref().unwrap_or(text));

    let link = found.link.as_deref();
    let opened = auto_open && link.is_some() && found.code.is_none();
    if let (true, Some(link)) = (opened, link) {
        winutil::open(link);
    }

    let mut buttons = Vec::new();
    if let Some(code) = &found.code
        && text.trim() != code
        && text.len() < 1500
    {
        buttons.push(("Copy full message".to_string(), copy_link(text)));
    }
    if let Some(link) = link {
        buttons.push(("Open link".to_string(), link.to_string()));
    }
    if let Some(address) = &found.address {
        buttons.push((
            "Open in Maps".to_string(),
            format!(
                "https://www.google.com/maps/search/?api=1&query={}",
                url::encode(address)
            ),
        ));
    }
    if let Some(email) = &found.email {
        buttons.push(("Email".to_string(), format!("mailto:{email}")));
    }
    if let Some(phone) = &found.phone
        && text.trim() != phone
    {
        buttons.push(("Copy number".to_string(), copy_link(phone)));
    }

    let (title, note) = match (&found.code, link, opened) {
        (Some(code), _, _) => (
            format!("Code {code} from {device}"),
            "Code copied. Paste it where you're signing in.",
        ),
        (None, Some(_), true) => (
            format!("Opened a link from {device}"),
            "Copied to your clipboard.",
        ),
        (None, Some(_), false) => (format!("Link from {device}"), "Copied to your clipboard."),
        (None, None, _) => (format!("From {device}"), "Copied to your clipboard."),
    };
    winutil::notify(&Toast {
        title,
        body: format!("{}\n{note}", preview(text, 180)),
        launch: link.map(str::to_string),
        buttons,
        ..Default::default()
    });
}

/// One notification for whatever the phone just sent: a single file gets a
/// preview and Open buttons, a batch gets a summary.
fn notify_files(device: &str, batch: &[(PathBuf, u64)]) {
    let Some((last, last_id)) = batch.last() else {
        return;
    };
    let image = Some(feed::thumb_path(*last_id)).filter(|p| files::is_image(last) && p.exists());
    let folder = action("folder", None);

    let toast = if let [(path, _)] = batch {
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
        let kinds: Vec<&str> = batch.iter().map(|(p, _)| files::kind(p)).collect();
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
        ("show", _) => winutil::show_running_window(),
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
