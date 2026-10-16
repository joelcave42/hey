//! Remote viewing: streams one window (or the whole screen) to the phone as
//! live H.264 video.
//!
//! The phone asks for `{"t":"windows"}` to list what it can watch, then opens
//! `{"t":"view","target":"<id>"}`. After an `{"ok":true}` line the PC sends
//! binary messages until the phone hangs up: a 4-byte big-endian length, a
//! 1-byte kind, then the payload.
//!
//!   kind 0  keep-alive (empty)
//!   kind 1  size change: JSON {"width":W,"height":H}; a keyframe follows
//!   kind 2  one H.264 access unit, Annex B
//!
//! Frames are captured only when the window changes. Capture keeps just the
//! newest frame, so if the network falls behind Hey skips stale frames
//! rather than queueing them, and the picture stays live.

use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType,
};
use openh264::formats::{BgraSliceU8, YUVBuffer};
use serde_json::{Value, json};
use std::io::{self, BufReader, Write};
use std::net::TcpStream;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

/// The largest picture sent, in pixels. Phones are about this wide anyway,
/// and it keeps encoding light.
const MAX_WIDTH: usize = 1280;
const MAX_HEIGHT: usize = 1280;
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Windows the phone can watch, plus the whole screen.
pub fn list() -> Value {
    let own_process = std::process::id();
    let mut windows: Vec<Value> = Window::enumerate()
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.process_id().ok() != Some(own_process))
        .filter_map(|w| {
            let title = w.title().ok()?.trim().to_string();
            if title.is_empty() || w.width().ok()? < 120 || w.height().ok()? < 80 {
                return None;
            }
            let app = w
                .process_name()
                .ok()
                .map(|p| app_name(&p))
                .unwrap_or_default();
            Some(json!({
                "id": (w.as_raw_hwnd() as usize).to_string(),
                "title": title,
                "app": app,
            }))
        })
        .collect();
    windows.sort_by_key(|w| w["app"].as_str().unwrap_or("").to_lowercase());
    json!({
        "ok": true,
        "windows": windows,
        "screens": [{"id": "screen", "title": "Whole screen"}],
    })
}

/// "chrome.exe" -> "Chrome"
fn app_name(process: &str) -> String {
    let stem = process.trim_end_matches(".exe").trim_end_matches(".EXE");
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The newest captured frame, waiting to be encoded.
#[derive(Default)]
struct Mailbox {
    frame: Mutex<Option<RawFrame>>,
    ready: Condvar,
    closed: Mutex<bool>,
}

struct RawFrame {
    width: usize,
    height: usize,
    bgra: Vec<u8>,
}

struct Capturer {
    mailbox: Arc<Mailbox>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Capturer {
    type Flags = Arc<Mailbox>;
    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Capturer {
            mailbox: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if *self.mailbox.closed.lock().unwrap() {
            control.stop();
            return Ok(());
        }
        let buffer = frame.buffer()?;
        let (width, height) = (buffer.width() as usize, buffer.height() as usize);
        let pixels = buffer.as_nopadding_buffer(&mut self.scratch);
        let raw = shrink(pixels, width, height);
        *self.mailbox.frame.lock().unwrap() = Some(raw);
        self.mailbox.ready.notify_one();
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        *self.mailbox.closed.lock().unwrap() = true;
        self.mailbox.ready.notify_one();
        Ok(())
    }
}

/// Scales a BGRA picture down to fit MAX_WIDTH x MAX_HEIGHT (averaging the
/// pixels it merges, so text stays readable) and makes both sides even,
/// which H.264 needs.
fn shrink(bgra: &[u8], width: usize, height: usize) -> RawFrame {
    let scale = (width as f32 / MAX_WIDTH as f32)
        .max(height as f32 / MAX_HEIGHT as f32)
        .max(1.0);
    let out_w = (((width as f32 / scale) as usize) & !1).max(2);
    let out_h = (((height as f32 / scale) as usize) & !1).max(2);
    let mut out = vec![0u8; out_w * out_h * 4];
    for y in 0..out_h {
        let y0 = y * height / out_h;
        let y1 = ((y + 1) * height / out_h).max(y0 + 1).min(height);
        for x in 0..out_w {
            let x0 = x * width / out_w;
            let x1 = ((x + 1) * width / out_w).max(x0 + 1).min(width);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                let row = sy * width * 4;
                for sx in x0..x1 {
                    let i = row + sx * 4;
                    for c in 0..4 {
                        sum[c] += bgra[i + c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * out_w + x) * 4;
            for c in 0..4 {
                out[o + c] = (sum[c] / n) as u8;
            }
        }
    }
    RawFrame {
        width: out_w,
        height: out_h,
        bgra: out,
    }
}

/// Streams the chosen window to the phone until it hangs up or the window
/// closes.
pub fn serve(reader: &mut BufReader<&mut Tls>, target: &str) -> io::Result<()> {
    let mailbox = Arc::new(Mailbox::default());
    let started = if target == "screen" {
        let monitor = Monitor::primary().map_err(io::Error::other)?;
        Capturer::start_free_threaded(settings(monitor, &mailbox))
    } else {
        let hwnd: usize = target
            .parse()
            .map_err(|_| io::Error::other("unknown window"))?;
        let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
        if !window.is_valid() {
            let stream = reader.get_mut();
            stream.write_all(
                format!(
                    "{}\n",
                    json!({"ok": false, "error": "That window has closed."})
                )
                .as_bytes(),
            )?;
            return stream.flush();
        }
        Capturer::start_free_threaded(settings(window, &mailbox))
    };
    let control = started.map_err(|e| io::Error::other(e.to_string()))?;

    let result = stream_frames(reader.get_mut(), &mailbox);
    *mailbox.closed.lock().unwrap() = true;
    let _ = control.stop();
    result
}

fn settings<T: TryInto<GraphicsCaptureItemType>>(
    item: T,
    mailbox: &Arc<Mailbox>,
) -> Settings<Arc<Mailbox>, T> {
    Settings::new(
        item,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(FRAME_INTERVAL),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        mailbox.clone(),
    )
}

fn stream_frames(stream: &mut Tls, mailbox: &Mailbox) -> io::Result<()> {
    stream.write_all(format!("{}\n", json!({"ok": true})).as_bytes())?;
    stream.flush()?;
    let mut encoder: Option<(Encoder, usize, usize)> = None;
    loop {
        let frame = {
            let mut slot = mailbox.frame.lock().unwrap();
            if slot.is_none() {
                slot = mailbox
                    .ready
                    .wait_timeout(slot, Duration::from_secs(2))
                    .unwrap()
                    .0;
            }
            slot.take()
        };
        if *mailbox.closed.lock().unwrap() {
            return Ok(());
        }
        let Some(frame) = frame else {
            // Nothing changed on screen; check the phone is still there.
            send(stream, 0, &[])?;
            continue;
        };

        if encoder.as_ref().map(|(_, w, h)| (*w, *h)) != Some((frame.width, frame.height)) {
            encoder = Some((new_encoder()?, frame.width, frame.height));
            let size = json!({"width": frame.width, "height": frame.height}).to_string();
            send(stream, 1, size.as_bytes())?;
        }
        let (enc, w, h) = encoder.as_mut().unwrap();
        let yuv = YUVBuffer::from_bgra8_source(BgraSliceU8::new(&frame.bgra, (*w, *h)));
        let bitstream = enc.encode(&yuv).map_err(io::Error::other)?.to_vec();
        if !bitstream.is_empty() {
            send(stream, 2, &bitstream)?;
        }
    }
}

fn new_encoder() -> io::Result<Encoder> {
    let config = EncoderConfig::new()
        .usage_type(UsageType::ScreenContentRealTime)
        .rate_control_mode(RateControlMode::Bitrate)
        .bitrate(BitRate::from_bps(4_000_000))
        .max_frame_rate(FrameRate::from_hz(30.0))
        .intra_frame_period(IntraFramePeriod::from_num_frames(120))
        .skip_frames(false);
    Encoder::with_api_config(OpenH264API::from_source(), config).map_err(io::Error::other)
}

fn send(stream: &mut Tls, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut header = [0u8; 5];
    header[..4].copy_from_slice(&((payload.len() + 1) as u32).to_be_bytes());
    header[4] = kind;
    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinks_to_even_sizes_within_limits() {
        let f = shrink(&vec![255u8; 2561 * 1441 * 4], 2561, 1441);
        assert!(f.width <= MAX_WIDTH && f.height <= MAX_HEIGHT);
        assert_eq!((f.width % 2, f.height % 2), (0, 0));
        let small = shrink(&vec![0u8; 301 * 201 * 4], 301, 201);
        assert_eq!((small.width, small.height), (300, 200));
    }

    #[test]
    fn names_apps() {
        assert_eq!(app_name("chrome.exe"), "Chrome");
        assert_eq!(app_name("Discord.exe"), "Discord");
    }
}
