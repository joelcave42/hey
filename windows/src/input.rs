//! Controlling the PC from the phone while watching it.
//!
//! When a view starts, the PC gives it a session number. The phone opens a
//! second connection, `{"t":"input","session":N}`, and sends one JSON line
//! per action. Positions are fractions (0 to 1) of the picture it's
//! watching, so they map onto the window wherever it is on the PC:
//!
//!   {"e":"tap","x":0.5,"y":0.5}        left click
//!   {"e":"double","x":..,"y":..}       double click
//!   {"e":"right","x":..,"y":..}        right click
//!   {"e":"down"|"move"|"up","x":..,"y":..}  drag with the left button
//!   {"e":"scroll","x":..,"y":..,"dy":-1}    mouse wheel notches (+ is up)
//!   {"e":"text","text":"hi"}            type text
//!   {"e":"key","key":"Enter"}           Enter, Backspace, Tab, Escape, arrows...

use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

/// One notch of a mouse wheel.
const WHEEL_DELTA: u32 = 120;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, IsIconic, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

/// What a view session is showing, so input lands in the right place.
#[derive(Clone, Copy)]
pub enum Target {
    Window(isize),
    Screen,
}

static SESSIONS: Mutex<Option<HashMap<u64, Target>>> = Mutex::new(None);

pub fn open_session(target: Target) -> u64 {
    let id = crate::config::random_token(8)
        .bytes()
        .fold(0u64, |a, b| a.wrapping_mul(31).wrapping_add(b as u64));
    SESSIONS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(id, target);
    id
}

pub fn close_session(id: u64) {
    if let Some(map) = SESSIONS.lock().unwrap().as_mut() {
        map.remove(&id);
    }
}

pub fn session(id: u64) -> Option<Target> {
    SESSIONS.lock().unwrap().as_ref()?.get(&id).copied()
}

/// Carries out one action from the phone.
pub fn handle(target: Target, event: &Value) {
    let point = || {
        let x = event["x"].as_f64()?.clamp(0.0, 1.0);
        let y = event["y"].as_f64()?.clamp(0.0, 1.0);
        Some(to_screen(target, x, y))
    };
    match event["e"].as_str().unwrap_or("") {
        "tap" => {
            if let Some(p) = point() {
                focus(target);
                click(p, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 1);
            }
        }
        "double" => {
            if let Some(p) = point() {
                focus(target);
                click(p, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, 2);
            }
        }
        "right" => {
            if let Some(p) = point() {
                focus(target);
                click(p, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, 1);
            }
        }
        "down" => {
            if let Some(p) = point() {
                focus(target);
                send_mouse(&[
                    mouse(p, MOUSEEVENTF_MOVE, 0),
                    mouse(p, MOUSEEVENTF_LEFTDOWN, 0),
                ]);
            }
        }
        "move" => {
            if let Some(p) = point() {
                send_mouse(&[mouse(p, MOUSEEVENTF_MOVE, 0)]);
            }
        }
        "up" => {
            if let Some(p) = point() {
                send_mouse(&[
                    mouse(p, MOUSEEVENTF_MOVE, 0),
                    mouse(p, MOUSEEVENTF_LEFTUP, 0),
                ]);
            }
        }
        "scroll" => {
            if let Some(p) = point() {
                let mut inputs = vec![mouse(p, MOUSEEVENTF_MOVE, 0)];
                let dy = (event["dy"].as_f64().unwrap_or(0.0) * WHEEL_DELTA as f64) as i32;
                let dx = (event["dx"].as_f64().unwrap_or(0.0) * WHEEL_DELTA as f64) as i32;
                if dy != 0 {
                    inputs.push(mouse(p, MOUSEEVENTF_WHEEL, dy));
                }
                if dx != 0 {
                    inputs.push(mouse(p, MOUSEEVENTF_HWHEEL, dx));
                }
                send_mouse(&inputs);
            }
        }
        "text" => {
            focus(target);
            type_text(event["text"].as_str().unwrap_or(""));
        }
        "key" => {
            focus(target);
            if let Some(key) = virtual_key(event["key"].as_str().unwrap_or("")) {
                send_keys(&[key_input(key, false), key_input(key, true)]);
            }
        }
        _ => {}
    }
}

/// Where a fraction of the picture is on the PC's screen, in pixels.
fn to_screen(target: Target, x: f64, y: f64) -> (i32, i32) {
    let rect = match target {
        Target::Window(hwnd) => window_rect(hwnd),
        Target::Screen => unsafe {
            RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            }
        },
    };
    let w = (rect.right - rect.left).max(1) as f64;
    let h = (rect.bottom - rect.top).max(1) as f64;
    (
        rect.left + (x * (w - 1.0)).round() as i32,
        rect.top + (y * (h - 1.0)).round() as i32,
    )
}

/// The part of a window that capture shows: its visible frame, without the
/// invisible resize borders.
fn window_rect(hwnd: isize) -> RECT {
    let mut rect = RECT::default();
    unsafe {
        let _ = DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );
    }
    rect
}

/// Brings the watched window to the front, so clicks and keys reach it
/// rather than whatever is covering it.
fn focus(target: Target) {
    let Target::Window(hwnd) = target else { return };
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        // Windows only lets the app that last had input change the foreground
        // window; a harmless Alt tap counts as input.
        send_keys(&[key_input(VK_MENU, false), key_input(VK_MENU, true)]);
        let _ = SetForegroundWindow(hwnd);
    }
}

fn click(p: (i32, i32), down: MOUSE_EVENT_FLAGS, up: MOUSE_EVENT_FLAGS, times: usize) {
    let mut inputs = vec![mouse(p, MOUSEEVENTF_MOVE, 0)];
    for _ in 0..times {
        inputs.push(mouse(p, down, 0));
        inputs.push(mouse(p, up, 0));
    }
    send_mouse(&inputs);
}

/// A mouse action at an absolute screen position.
fn mouse(p: (i32, i32), flags: MOUSE_EVENT_FLAGS, data: i32) -> INPUT {
    let (vx, vy, vw, vh) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        )
    };
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ((p.0 - vx) as i64 * 65535 / (vw - 1) as i64) as i32,
                dy: ((p.1 - vy) as i64 * 65535 / (vh - 1) as i64) as i32,
                mouseData: data as u32,
                dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_mouse(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn key_input(key: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_keys(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Types text as Unicode characters, so any language and emoji work.
fn type_text(text: &str) {
    let mut inputs = Vec::new();
    for unit in text.encode_utf16() {
        if unit == '\n' as u16 {
            inputs.push(key_input(VK_RETURN, false));
            inputs.push(key_input(VK_RETURN, true));
            continue;
        }
        for up in [false, true] {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: unit,
                        dwFlags: KEYEVENTF_UNICODE
                            | if up {
                                KEYEVENTF_KEYUP
                            } else {
                                KEYBD_EVENT_FLAGS(0)
                            },
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }
    }
    send_keys(&inputs);
}

fn virtual_key(name: &str) -> Option<VIRTUAL_KEY> {
    Some(match name {
        "Enter" => VK_RETURN,
        "Backspace" => VK_BACK,
        "Tab" => VK_TAB,
        "Escape" => VK_ESCAPE,
        "Delete" => VK_DELETE,
        "Left" => VK_LEFT,
        "Right" => VK_RIGHT,
        "Up" => VK_UP,
        "Down" => VK_DOWN,
        "Home" => VK_HOME,
        "End" => VK_END,
        "PageUp" => VK_PRIOR,
        "PageDown" => VK_NEXT,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_open_and_close() {
        let id = open_session(Target::Screen);
        assert!(session(id).is_some());
        close_session(id);
        assert!(session(id).is_none());
    }

    #[test]
    fn knows_common_keys() {
        assert_eq!(virtual_key("Enter"), Some(VK_RETURN));
        assert_eq!(virtual_key("nope"), None);
    }
}
