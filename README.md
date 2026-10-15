<p align="center"><img src="docs/icon.png" width="96" alt="Hey icon"></p>

<h1 align="center">Hey</h1>

<p align="center"><b>Send links and text from your Android phone to your Windows PC. One tap, no account, no cloud.</b></p>

Your friend sends you a Zoom link on your phone, and you need it on your PC.
Normally that means messaging it to yourself on Discord. With Hey you tap
**Share → Send to PC** and it's already on your PC's clipboard, with a
notification that opens the link.

## What it does

- **Share sheet:** in any app, tap **Share → Send to PC**.
- **Selected text:** select text anywhere and pick **Send to PC** from the menu.
- **Clipboard tile:** swipe down, tap **Send clipboard** in quick settings.
- **On the PC:** whatever arrives is copied to your clipboard and shown in a
  Windows notification. If it contains a link (even inside a longer message,
  like a Zoom invite), the notification has an **Open link** button.
- **Tray menu:** the last 8 things you sent are under **Recent**; click one to
  copy it again.

## How it works

```
 Android phone                                 Windows PC
┌──────────────────┐   TLS 1.3, pinned cert   ┌──────────────────────┐
│ Share / tile /   │ ───────────────────────▶ │ Hey tray app (Rust)  │
│ text selection   │   {"t":"text", ...}      │ → clipboard          │
│ (Kotlin)         │ ◀─────────────────────── │ → notification       │
└──────────────────┘   {"ok":true}            └──────────────────────┘
        │              finds the PC by mDNS             │
        └──────────── (_hey._tcp) if its IP changes ────┘
```

- **Pairing.** The PC shows a QR code holding its addresses, its certificate's
  SHA-256 fingerprint and a one-time key that expires after 10 minutes. The
  phone scans it, connects, and trades the key for a long-lived device token.
- **Security.** Every connection is TLS, and the phone only accepts the exact
  certificate from the QR code, so nothing else on the network can pose as
  your PC. The PC only accepts tokens from phones it paired with, and stores
  their SHA-256 hashes, never the tokens themselves.
- **No servers.** Phone and PC talk directly over your local network. If the
  PC's IP address changes, the phone finds it again with mDNS.

## Install

### Windows

Requires [Rust](https://rustup.rs).

```bash
cd windows
cargo build --release
```

Run `windows/target/release/hey.exe`. Hey appears in the system tray and opens
the pairing page. When Windows Firewall asks, allow Hey on **private
networks**. Tick **Start with Windows** in the tray menu to keep it running.

### Android

Requires the Android SDK (set `ANDROID_HOME`) and Java 17 or later.

```bash
cd android
./gradlew assembleDebug
```

Install `android/app/build/outputs/apk/debug/app-debug.apk` on your phone
(Android 8 or later), open Hey and tap **Scan pairing code**.

## Project layout

| Path | What's there |
| --- | --- |
| `windows/src/server.rs` | TLS listener, pairing and request handling |
| `windows/src/pairing.rs` | QR pairing page |
| `windows/src/winutil.rs` | Notifications, registry, opening links |
| `windows/src/main.rs` | Tray icon, menu and event loop |
| `android/.../HeyClient.kt` | Pinned TLS client and mDNS discovery |
| `android/.../Send.kt` | Share target and clipboard sender |
| `android/.../HeyTileService.kt` | Quick settings tile |
| `tools/fake_phone.py` | Tests the PC app without a phone |

## Roadmap

- [x] Phone → PC links and text
- [ ] Files, both ways
- [ ] PC → phone
- [ ] Works away from home Wi-Fi
- [ ] Installer for Windows, APK on GitHub Releases

## License

MIT
