"""Pretend to be the Android app, for testing Hey for Windows without a phone.

    python tools/fake_phone.py pair "hey://pair?..."    # pair, saves a token
    python tools/fake_phone.py send "https://zoom.us/j/123"
    python tools/fake_phone.py files photo.jpg notes.txt  # one batch
    python tools/fake_phone.py windows                      # what can be watched
    python tools/fake_phone.py watch screen 5 out.h264      # stream for 5 seconds

HEY_PORT=47821 talks to a developer-mode Hey (HEY_DEV=1) instead.
"""

import base64
import hashlib
import json
import os
import socket
import ssl
import struct
import sys
import time
from pathlib import Path
from urllib.parse import parse_qs, urlparse

STATE = Path(__file__).with_name(".fake_phone.json")


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def connect(host: str, port: int, fingerprint: str) -> ssl.SSLSocket:
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE  # we pin the certificate's hash instead
    tls = ctx.wrap_socket(socket.create_connection((host, port), timeout=5))
    cert = tls.getpeercert(binary_form=True)
    if b64(hashlib.sha256(cert).digest()) != fingerprint:
        tls.close()
        raise SystemExit("Certificate doesn't match the pairing code. Refusing to send.")
    return tls


def state() -> dict:
    s = json.loads(STATE.read_text())
    if "HEY_PORT" in os.environ:
        s["port"] = int(os.environ["HEY_PORT"])
    return s


def request(host: str, port: int, fingerprint: str, body: dict, payload: Path | None = None) -> dict:
    with connect(host, port, fingerprint) as tls:
        tls.sendall((json.dumps(body) + "\n").encode())
        if payload:
            with payload.open("rb") as f:
                while chunk := f.read(1 << 16):
                    tls.sendall(chunk)
        reply = tls.makefile().readline()
    return json.loads(reply)


def watch(s: dict, target: str, seconds: float, out: Path) -> None:
    """Streams a window like the phone does and reports frame rate and size."""
    with connect(s["host"], s["port"], s["fp"]) as tls, out.open("wb") as video:
        tls.settimeout(10)
        tls.sendall((json.dumps({"t": "view", "token": s["token"], "target": target}) + "\n").encode())
        stream = tls.makefile("rb")
        print("reply:", stream.readline().decode().strip())
        start, frames, total, sizes = time.time(), 0, 0, []
        first = None
        while time.time() - start < seconds:
            length, kind = struct.unpack(">IB", stream.read(5))
            payload = stream.read(length - 1)
            if kind == 1:
                sizes.append(json.loads(payload))
            elif kind == 2:
                if first is None:
                    first = time.time() - start
                    starts = [i for i in range(len(payload) - 4) if payload[i:i + 4] == b"\x00\x00\x00\x01"]
                    nal_types = sorted({payload[i + 4] & 0x1F for i in starts})
                    print(f"first frame after {first:.2f}s, NAL types {nal_types}")
                frames += 1
                total += len(payload)
                video.write(payload)
        elapsed = time.time() - start
        print(f"sizes: {sizes}")
        print(f"{frames} frames in {elapsed:.1f}s = {frames / elapsed:.1f} fps, "
              f"{total * 8 / elapsed / 1e6:.2f} Mbit/s")


def main() -> None:
    command = sys.argv[1]
    arg = sys.argv[2] if len(sys.argv) > 2 else ""
    if command == "pair":
        q = {k: v[0] for k, v in parse_qs(urlparse(arg).query).items()}
        host = sys.argv[3] if len(sys.argv) > 3 else "127.0.0.1"
        reply = request(host, int(q["port"]), q["fp"],
                        {"t": "pair", "key": q["key"], "device": "Fake Phone"})
        print(reply)
        if reply.get("ok"):
            STATE.write_text(json.dumps({"host": host, "port": int(q["port"]),
                                         "fp": q["fp"], "token": reply["token"]}))
    elif command == "send":
        s = state()
        print(request(s["host"], s["port"], s["fp"], {"t": "text", "token": s["token"], "text": arg}))
    elif command == "files":
        s = state()
        paths = [Path(a) for a in sys.argv[2:]]
        for i, path in enumerate(paths):
            body = {"t": "file", "token": s["token"], "name": path.name,
                    "size": path.stat().st_size, "index": i, "count": len(paths)}
            print(path.name, request(s["host"], s["port"], s["fp"], body, payload=path))
    elif command == "windows":
        s = state()
        reply = request(s["host"], s["port"], s["fp"], {"t": "windows", "token": s["token"]})
        for w in reply.get("screens", []) + reply.get("windows", []):
            print(f'{w["id"]:>12}  {w.get("app", ""):<14} {w["title"][:60]}')
    elif command == "watch":
        watch(state(), arg, float(sys.argv[3]), Path(sys.argv[4]))


if __name__ == "__main__":
    main()
