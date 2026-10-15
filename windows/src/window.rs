//! The Hey window: a native window showing ui/index.html in WebView2. The
//! page asks for things with `ipc.postMessage`; Hey sends it fresh state by
//! calling `hey.update(...)`.

use std::borrow::Cow;
use tao::dpi::LogicalSize;
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Icon, Theme, Window, WindowBuilder};
use wry::http::{Request, Response};
use wry::{WebContext, WebView, WebViewBuilder};

const PAGE: &str = include_str!("../ui/index.html");

#[cfg(windows)]
const START_URL: &str = "http://hey.localhost/";
#[cfg(not(windows))]
const START_URL: &str = "hey://localhost/";

pub struct HeyWindow {
    pub window: Window,
    pub webview: WebView,
    // Keeps WebView2's data in %APPDATA%\Hey rather than next to hey.exe.
    _context: WebContext,
}

impl HeyWindow {
    pub fn new<T>(
        target: &EventLoopWindowTarget<T>,
        visible: bool,
        on_message: impl Fn(String) + 'static,
    ) -> Result<HeyWindow, Box<dyn std::error::Error>> {
        let dark = crate::winutil::prefers_dark();
        let window = WindowBuilder::new()
            .with_title("Hey")
            .with_inner_size(LogicalSize::new(1040.0, 720.0))
            .with_min_inner_size(LogicalSize::new(760.0, 520.0))
            .with_window_icon(Icon::from_rgba(crate::icon::rgba(64), 64, 64).ok())
            .with_theme(Some(if dark { Theme::Dark } else { Theme::Light }))
            .with_visible(visible)
            .build(target)?;

        let mut context = WebContext::new(Some(crate::config::dir().join("webview")));
        let icon = crate::icon::png(128);
        let background = if dark {
            (15, 16, 21, 255)
        } else {
            (245, 245, 250, 255)
        };
        let webview = WebViewBuilder::new_with_web_context(&mut context)
            .with_custom_protocol("hey".into(), move |_, request| serve(&request, &icon))
            .with_url(START_URL)
            .with_background_color(background)
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |request: Request<String>| on_message(request.into_body()))
            .build(&window)?;

        Ok(HeyWindow {
            window,
            webview,
            _context: context,
        })
    }

    pub fn show(&self) {
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.set_focus();
    }

    /// Sends the whole state to the page, which redraws.
    pub fn update(&self, state: &serde_json::Value) {
        let _ = self
            .webview
            .evaluate_script(&format!("window.hey && hey.update({state})"));
    }

    pub fn toast(&self, text: &str) {
        let text = serde_json::Value::from(text);
        let _ = self
            .webview
            .evaluate_script(&format!("window.hey && hey.toast({text})"));
    }
}

fn serve(request: &Request<Vec<u8>>, icon: &[u8]) -> Response<Cow<'static, [u8]>> {
    let path = request.uri().path();
    let found = match path {
        "/" | "/index.html" => Some(("text/html; charset=utf-8", Cow::Borrowed(PAGE.as_bytes()))),
        "/icon.png" => Some(("image/png", Cow::Owned(icon.to_vec()))),
        _ => path
            .strip_prefix("/thumb/")
            .and_then(|id| id.parse::<u64>().ok())
            .and_then(|id| std::fs::read(crate::feed::thumb_path(id)).ok())
            .map(|bytes| ("image/jpeg", Cow::Owned(bytes))),
    };
    match found {
        Some((kind, body)) => Response::builder()
            .header("Content-Type", kind)
            .header("Cache-Control", "no-store")
            .body(body)
            .unwrap(),
        None => Response::builder()
            .status(404)
            .body(Cow::Borrowed(&b""[..]))
            .unwrap(),
    }
}
