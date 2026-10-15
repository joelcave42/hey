"""Pretend to be the Android app, for testing Hey for Windows without a phone.

    python tools/fake_phone.py pair "hey://pair?..."    # pair, saves a token
    python tools/fake_phone.py send "https://zoom.us/j/123"
    python tools/fake_phone.py files photo.jpg notes.txt  # one batch
"""

import base64
import hashlib
import json
import socket
import ssl
import sys
from pathlib import Path
from urllib.parse import parse_qs, urlparse

STATE = Path(__file__).with_name(".fake_phone.json")


def b64(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def request(host: str, port: int, fingerprint: str, body: dict, payload: Path | None = None) -> dict:
    ctx = ssl.create_default_context()
    ctx.check_hostname = False
    ctx.verify_mode = ssl.CERT_NONE  # we pin the certificate's hash instead
    with socket.create_connection((host, port), timeout=5) as raw:
        with ctx.wrap_socket(raw) as tls:
            cert = tls.getpeercert(binary_form=True)
            if b64(hashlib.sha256(cert).digest()) != fingerprint:
                raise SystemExit("Certificate doesn't match the pairing code. Refusing to send.")
            tls.sendall((json.dumps(body) + "\n").encode())
            if payload:
                with payload.open("rb") as f:
                    while chunk := f.read(1 << 16):
                        tls.sendall(chunk)
            reply = tls.makefile().readline()
    return json.loads(reply)


def main() -> None:
    command, arg = sys.argv[1], sys.argv[2]
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
        s = json.loads(STATE.read_text())
        print(request(s["host"], s["port"], s["fp"], {"t": "text", "token": s["token"], "text": arg}))
    elif command == "files":
        s = json.loads(STATE.read_text())
        paths = [Path(a) for a in sys.argv[2:]]
        for i, path in enumerate(paths):
            body = {"t": "file", "token": s["token"], "name": path.name,
                    "size": path.stat().st_size, "index": i, "count": len(paths)}
            print(path.name, request(s["host"], s["port"], s["fp"], body, payload=path))


if __name__ == "__main__":
    main()
