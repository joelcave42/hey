<p align="center"><img src="docs/icon.png" width="96" alt="Hey icon"></p>

<h1 align="center">Hey</h1>

<p align="center"><b>Send links and text from your Android phone to your Windows PC. One tap, no account, no cloud.</b></p>

Your friend sends you a Zoom link on your phone, and you need it on your PC.
Normally that means messaging it to yourself on Discord. With Hey you tap
**Share → Hey** and it's already on your PC's clipboard, with a
notification that opens the link.

## What it does

- **Share sheet:** in any app, tap **Share → Hey**. Links, text, photos,
  videos and any other file, one or many at once.
- **Selected text:** select text anywhere and pick **Hey** from the menu.
- **In the app:** big buttons for **Clipboard**, **Photos**, **Files** and
  **Type**, plus a **Recent** list where you can send anything again.
- **Clipboard tile:** swipe down, tap **Send clipboard** in quick settings.
- **Links and text on the PC:** copied to your clipboard and shown in a
  Windows notification. If there's a link (even inside a longer message, like
  a Zoom invite), the notification has an **Open link** button.
- **Photos and files on the PC:** saved to `Downloads\Hey`, with a preview in
  the notification and **Open** / **Show in folder** buttons. A single photo
  also lands on your clipboard, so you can paste a screenshot straight into
  Discord.
- **Big files keep going:** the phone shows a progress bar and keeps sending
  after you leave the share sheet.
- **Smart actions:** a sign-in code ("G-482913 is your verification code")
  puts just the code on your clipboard. Addresses get **Open in Maps**, email
  addresses get **Email**, phone numbers get **Copy number**.
- **Auto-open links:** tick **Open links automatically** in the tray menu and
  links open in your browser the moment they arrive.
- **Tray menu:** the last 8 things you sent are under **Recent**, and
  **Open Hey folder** opens `Downloads\Hey`.

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
| `windows/src/actions.rs` | Spots codes, links, addresses, emails and phone numbers |
| `windows/src/files.rs` | Saves incoming files, thumbnails, photo to clipboard |
| `windows/src/pairing.rs` | QR pairing page |
| `windows/src/winutil.rs` | Notifications, registry, opening links |
| `windows/src/main.rs` | Tray icon, menu and event loop |
| `android/.../ui/` | Jetpack Compose screens: home, pairing, theme |
| `android/.../History.kt` | The Recent list, kept on the phone |
| `android/.../HeyClient.kt` | Pinned TLS client and mDNS discovery |
| `android/.../Send.kt` | Share target and clipboard sender |
| `android/.../SendService.kt` | Sends photos and files with a progress bar |
| `android/.../HeyTileService.kt` | Quick settings tile |
| `tools/fake_phone.py` | Tests the PC app without a phone |

## Roadmap

**v0.1: links and text**
- [x] Phone → PC links and text from the share sheet, text selection and a quick settings tile
- [x] One-time QR pairing with a pinned certificate
- [x] Finds the PC again with mDNS when its IP changes

**v0.2: make the basics feel instant**
- [x] Photos and files → PC, saved to `Downloads\Hey`
- [x] Auto-open links as they arrive
- [x] Smart actions: sign-in codes, addresses, email addresses and phone numbers get the right button

**v0.3: both directions**
- [ ] PC → phone: **Send to phone** in Explorer's right-click menu, and from the tray
- [ ] Clipboard sync
- [ ] Screenshot a region and send it to the phone with one hotkey
- [ ] Drag and drop onto the tray icon
- [ ] Pair without a QR code: tap your PC on the phone and confirm a matching code

**v0.4: phone ↔ PC awareness**
- [ ] 2FA codes from texts pop up on the PC, already copied
- [ ] Phone notifications on the PC
- [ ] Reply to texts from the PC
- [ ] Phone battery in the tray, and ring my phone
- [ ] Searchable history of everything sent

**v0.5: remote control**
- [ ] Media remote: play/pause, skip and volume
- [ ] Phone as trackpad and keyboard
- [ ] Lock or sleep the PC from the phone
- [ ] Several phones and PCs, and choosing where to send

**v1.0: ship it**
- [ ] Works away from home Wi-Fi (through Tailscale, still no servers)
- [ ] Windows installer, APK on GitHub Releases, auto-updates
- [ ] Home screen widget and Samsung Edge panel

**Someday**
- [ ] Stream a PC window to the phone

## License

MIT
