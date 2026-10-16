<p align="center"><img src="docs/icon.png" width="96" alt="Hey icon"></p>

<h1 align="center">Hey</h1>

<p align="center"><a href="https://github.com/joelcave42/hey/actions/workflows/ci.yml"><img src="https://github.com/joelcave42/hey/actions/workflows/ci.yml/badge.svg" alt="CI"></a> <a href="https://github.com/joelcave42/hey/releases/latest"><img src="https://img.shields.io/github/v/release/joelcave42/hey" alt="Latest release"></a></p>

<p align="center"><b>Send links, text, photos and files between your Android phone and your Windows PC. One tap, no account, no cloud.</b></p>

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
- **Photos and files on the PC:** saved to `Downloads\Hey` (or any folder you
  pick in the Hey window), with a preview in
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
- **Screenshots in one click:** snip with Win+Shift+S, then hit **Clipboard**
  in the Hey window (or **Send clipboard to phone** in the tray). Copied files
  and text work the same way, and the phone's Clipboard button sends copied
  pictures too.
- **PC → phone:** type or paste in the Hey window's send box, drop files on
  the window, or right-click any file in Explorer → **Send to phone with Hey**
  (on Windows 11 it's under **Show more options**). Links and text land on the
  phone's clipboard with a notification; files go to `Download/Hey`.
- **Always ready:** with **Receive from PC** on, the phone keeps a quiet
  connection to your PC (with a small notification), reconnects on its own,
  and starts again after a restart.
- **Away from home:** install [Tailscale](https://tailscale.com/download)
  (free) on your PC and phone and sign in to the same account. Hey then
  reaches your PC over mobile data or any Wi-Fi, with no servers of ours in
  between. The phone learns the PC's Tailscale address on its own.
- **See your PC:** tap **See your PC** in the phone app, pick any window (or
  the whole screen), and watch it live. The PC sends H.264 video only when
  something changes, and skips stale frames instead of lagging.
- **The Hey window on your PC:** click the tray icon to see everything you've
  received, with photo thumbnails, search and filters, and Open / Copy / Show
  in folder on each item. Your phones and settings are in the sidebar, and
  pairing happens right in the window.

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
- **No servers.** Phone and PC talk directly over your local network, or over
  Tailscale when you're away. If the PC's IP address changes, the phone finds
  it again with mDNS, and the PC tells the phone its current addresses every
  time they connect.

## Install

Download both from the [latest release](https://github.com/joelcave42/hey/releases/latest). They're free, with no account and no ads.

**Windows 10 or 11:** run `Hey-Setup-<version>.exe`. It installs just for you
(no admin needed), adds Hey to the Start menu, and can start Hey when you sign
in. When Windows Firewall asks, allow Hey on **private networks**.

**Android 10 or later:** download `Hey-<version>.apk` on your phone and open
it. Android asks you to allow installing from your browser the first time.
Then open Hey, tap **Scan pairing code**, and scan the code in the Hey window
on your PC.

**Away from home (optional):** install [Tailscale](https://tailscale.com/download)
on both and sign in to the same account.

## Build from source

Windows needs [Rust](https://rustup.rs):

```bash
cd windows
cargo build --release
```

Android needs the Android SDK (set `ANDROID_HOME`) and Java 17 or later:

```bash
cd android
./gradlew assembleDebug
```

The installer is built with [Inno Setup](https://jrsoftware.org/isinfo.php)
from `installer/hey.iss`. Pushing a `v*` tag builds both downloads on GitHub
Actions and attaches them to the release.

## Project layout

| Path | What's there |
| --- | --- |
| `windows/src/server.rs` | TLS listener, pairing and request handling |
| `windows/src/net.rs` | This PC's addresses, including Tailscale's |
| `windows/src/view.rs` | Remote viewing: capture, H.264 encoding, streaming |
| `windows/src/push.rs` | PC → phone over the phone's always-open connection |
| `windows/src/control.rs` | Lets Explorer's Send to phone reach the running Hey |
| `windows/src/actions.rs` | Spots codes, links, addresses, emails and phone numbers |
| `windows/src/files.rs` | Saves incoming files, thumbnails, photo to clipboard |
| `windows/src/pairing.rs` | Pairing links and QR codes |
| `windows/src/window.rs` | The Hey window (WebView2) |
| `windows/src/feed.rs` | Everything received, saved for the window |
| `windows/ui/index.html` | The window's interface |
| `windows/src/winutil.rs` | Notifications, registry, opening links |
| `windows/src/main.rs` | Tray icon, window events and actions |
| `android/.../ui/` | Jetpack Compose screens: home, pairing, theme |
| `android/.../History.kt` | The Recent list, kept on the phone |
| `android/.../HeyClient.kt` | Pinned TLS client and mDNS discovery |
| `android/.../Send.kt` | Share target and clipboard sender |
| `android/.../ViewerActivity.kt` | Plays the PC's live video with the hardware decoder |
| `android/.../ReceiveService.kt` | Always ready to receive from the PC |
| `android/.../SendService.kt` | Sends photos and files with a progress bar |
| `android/.../HeyTileService.kt` | Quick settings tile |
| `installer/hey.iss` | Windows installer |
| `.github/workflows/` | Checks on every push; release builds on every tag |
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
- [x] PC → phone: send box, drag and drop, and **Send to phone** in Explorer's right-click menu
- [x] Send your PC clipboard to the phone
- [ ] Automatic clipboard sync
- [ ] Screenshot a region and send it to the phone with one hotkey
- [x] Drag and drop onto the Hey window
- [ ] Pair without a QR code: tap your PC on the phone and confirm a matching code

**v0.4: phone ↔ PC awareness**
- [ ] 2FA codes from texts pop up on the PC, already copied
- [ ] Phone notifications on the PC
- [ ] Reply to texts from the PC
- [ ] Phone battery in the tray, and ring my phone
- [x] Searchable history of everything received, in the Hey window

**v0.5: remote control**
- [ ] Media remote: play/pause, skip and volume
- [ ] Phone as trackpad and keyboard
- [ ] Lock or sleep the PC from the phone
- [ ] Several phones and PCs, and choosing where to send

**v1.0: ship it**
- [x] Works away from home Wi-Fi (through Tailscale, still no servers)
- [x] Windows installer and APK on GitHub Releases
- [ ] Auto-updates
- [ ] Home screen widget and Samsung Edge panel

**Someday**
- [x] Stream a PC window to the phone (view only)
- [ ] Hardware video encoding on the graphics card
- [ ] Control the PC from the phone: tap, scroll, type
- [ ] Pinch to zoom and landscape viewing

## License

MIT
