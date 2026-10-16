//! Remote viewing: streams one window (or the whole screen) to the phone as
//! live H.264 video.
//!
//! The phone asks for `{"t":"windows"}` to list what it can watch, then opens
//! `{"t":"view","target":"<id>"}`. After an `{"ok":true}` line the PC sends
//! binary messages until the phone hangs up: a 4-byte big-endian length, a
//! 1-byte kind, then the payload.
//!
//!   kind 0  keep-alive (empty)
//!   kind 1  size change: JSON {"width":W,"height":H,"encoder":"..."};
//!           a keyframe follows
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

/// How big, how often and how sharp: the graphics card can do full HD at
/// 60 fps; the processor-only encoder is kept lighter.
#[derive(Clone, Copy)]
struct Quality {
    max_side: usize,
    interval: Duration,
    fps: u32,
    bitrate: u32,
}

const HARDWARE: Quality = Quality {
    max_side: 1920,
    interval: Duration::from_millis(16),
    fps: 60,
    bitrate: 12_000_000,
};
const SOFTWARE: Quality = Quality {
    max_side: 1280,
    interval: Duration::from_millis(33),
    fps: 30,
    bitrate: 4_000_000,
};

/// Whether this PC has a hardware H.264 encoder that works (checked once).
fn hardware_available() -> bool {
    static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *AVAILABLE.get_or_init(|| crate::hwenc::HwEncoder::new(320, 240, 30, 1_000_000).is_ok())
}

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
    /// Frames are scaled down to fit this many pixels on their longest side.
    max_side: usize,
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
        let raw = shrink(pixels, width, height, self.mailbox.max_side);
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

/// Scales a BGRA picture down so its longest side fits `max_side` (averaging
/// the pixels it merges, so text stays readable) and makes both sides even,
/// which H.264 needs.
fn shrink(bgra: &[u8], width: usize, height: usize, max_side: usize) -> RawFrame {
    let scale = (width.max(height) as f32 / max_side as f32).max(1.0);
    if scale == 1.0 {
        // Already small enough: just trim to even sizes.
        let (out_w, out_h) = ((width & !1).max(2), (height & !1).max(2));
        let mut out = Vec::with_capacity(out_w * out_h * 4);
        for y in 0..out_h.min(height) {
            out.extend_from_slice(&bgra[y * width * 4..y * width * 4 + out_w.min(width) * 4]);
        }
        out.resize(out_w * out_h * 4, 0);
        return RawFrame {
            width: out_w,
            height: out_h,
            bgra: out,
        };
    }
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
pub fn serve(
    reader: &mut BufReader<&mut Tls>,
    target: &str,
    device: &str,
    shared: &crate::server::Shared,
    events: &std::sync::mpsc::Sender<crate::server::Event>,
) -> io::Result<()> {
    let quality = if hardware_available() {
        HARDWARE
    } else {
        SOFTWARE
    };
    let mailbox = Arc::new(Mailbox {
        max_side: quality.max_side,
        ..Default::default()
    });
    let started = if target == "screen" {
        let monitor = Monitor::primary().map_err(io::Error::other)?;
        Capturer::start_free_threaded(settings(monitor, &mailbox, quality))
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
        Capturer::start_free_threaded(settings(window, &mailbox, quality))
    };
    let control = started.map_err(|e| io::Error::other(e.to_string()))?;

    let input_target = match target.parse::<isize>() {
        Ok(hwnd) => crate::input::Target::Window(hwnd),
        Err(_) => crate::input::Target::Screen,
    };
    let session = crate::input::open_session(input_target);
    let what = match input_target {
        crate::input::Target::Window(hwnd) => Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void)
            .title()
            .unwrap_or_default(),
        crate::input::Target::Screen => "your screen".into(),
    };
    let _ = events.send(crate::server::Event::Viewing {
        device: device.to_string(),
        what,
    });
    let allowed = shared.config.lock().unwrap().allow_control;
    let reply = json!({"ok": true, "session": session.to_string(), "control": allowed});

    let result = stream_frames(reader.get_mut(), &mailbox, quality, &reply);
    crate::input::close_session(session);
    *mailbox.closed.lock().unwrap() = true;
    let _ = control.stop();
    result
}

fn settings<T: TryInto<GraphicsCaptureItemType>>(
    item: T,
    mailbox: &Arc<Mailbox>,
    quality: Quality,
) -> Settings<Arc<Mailbox>, T> {
    Settings::new(
        item,
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Custom(quality.interval),
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        mailbox.clone(),
    )
}

fn stream_frames(
    stream: &mut Tls,
    mailbox: &Mailbox,
    quality: Quality,
    reply: &Value,
) -> io::Result<()> {
    stream.write_all(format!("{reply}\n").as_bytes())?;
    stream.flush()?;
    let mut encoder: Option<(VideoEncoder, usize, usize)> = None;
    let mut first = true;
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
            let fresh = VideoEncoder::new(frame.width, frame.height, quality)?;
            let size = json!({
                "width": frame.width,
                "height": frame.height,
                "encoder": fresh.name(),
            });
            send(stream, 1, size.to_string().as_bytes())?;
            encoder = Some((fresh, frame.width, frame.height));
            first = true;
        }
        let (enc, _, _) = encoder.as_mut().unwrap();
        let bitstream = enc.encode(&frame, first)?;
        first = false;
        if !bitstream.is_empty() {
            send(stream, 2, &bitstream)?;
        }
    }
}

/// The graphics card's encoder when there is one, otherwise OpenH264 on the
/// processor.
enum VideoEncoder {
    Hardware(crate::hwenc::HwEncoder),
    Software(Box<Encoder>),
}

impl VideoEncoder {
    fn new(width: usize, height: usize, quality: Quality) -> io::Result<VideoEncoder> {
        if hardware_available()
            && let Ok(encoder) = crate::hwenc::HwEncoder::new(
                width as u32,
                height as u32,
                quality.fps,
                quality.bitrate,
            )
        {
            return Ok(VideoEncoder::Hardware(encoder));
        }
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(SOFTWARE.bitrate))
            .max_frame_rate(FrameRate::from_hz(SOFTWARE.fps as f32))
            .intra_frame_period(IntraFramePeriod::from_num_frames(SOFTWARE.fps * 4))
            .skip_frames(false);
        Encoder::with_api_config(OpenH264API::from_source(), config)
            .map(|e| VideoEncoder::Software(Box::new(e)))
            .map_err(io::Error::other)
    }

    fn name(&self) -> String {
        match self {
            VideoEncoder::Hardware(e) => e.name.clone(),
            VideoEncoder::Software(_) => "OpenH264 (processor)".into(),
        }
    }

    fn encode(&mut self, frame: &RawFrame, keyframe: bool) -> io::Result<Vec<u8>> {
        match self {
            VideoEncoder::Hardware(e) => {
                let nv12 = crate::hwenc::bgra_to_nv12(&frame.bgra, frame.width, frame.height);
                e.encode(&nv12, keyframe).map_err(io::Error::other)
            }
            VideoEncoder::Software(e) => {
                let yuv = YUVBuffer::from_bgra8_source(BgraSliceU8::new(
                    &frame.bgra,
                    (frame.width, frame.height),
                ));
                Ok(e.encode(&yuv).map_err(io::Error::other)?.to_vec())
            }
        }
    }
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
        let f = shrink(&vec![255u8; 2561 * 1441 * 4], 2561, 1441, 1280);
        assert!(f.width <= 1280 && f.height <= 1280);
        assert_eq!((f.width % 2, f.height % 2), (0, 0));
        let small = shrink(&vec![0u8; 301 * 201 * 4], 301, 201, 1280);
        assert_eq!((small.width, small.height), (300, 200));
    }

    #[test]
    fn names_apps() {
        assert_eq!(app_name("chrome.exe"), "Chrome");
        assert_eq!(app_name("Discord.exe"), "Discord");
    }
}
