"""The probe add-in's other end (wps-e2e.sh): serves the add-in to WPS,
records what it reports and stands in for the connector's CORS answers.

    python3 server.py <results folder>

Every request goes to requests.log; reports (/report?step=…&ok=…&detail=…)
to reports.jsonl, one JSON object a line.
"""

import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

ADDIN = Path(__file__).resolve().parent
OUT = Path(sys.argv[1])
GIF = bytes.fromhex("47494638396101000100800000000000ffffff21f90401000000002c00000000010001000002024401003b")
TYPES = {".html": "text/html; charset=utf-8", ".js": "text/javascript", ".xml": "text/xml; charset=utf-8"}
PLUGINS = """<?xml version="1.0" encoding="UTF-8"?>
<jsplugins>
  <jspluginonline name="paperloomprobe" type="wps" url="http://127.0.0.1:8977/addin/"/>
</jsplugins>
"""


class Handler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        with open(OUT / "requests.log", "a", encoding="utf-8") as log:
            log.write(f"{self.command} {self.path} origin={self.headers.get('Origin')} "
                      f"ua={self.headers.get('User-Agent')}\n")

    def send(self, status, body=b"", kind="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", self.headers.get("Origin") or "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Authorization, Content-Type")
        if self.headers.get("Access-Control-Request-Private-Network"):
            self.send_header("Access-Control-Allow-Private-Network", "true")
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_OPTIONS(self):
        self.send(204)

    def do_GET(self):
        url = urlparse(self.path)
        if url.path == "/report":
            fields = {k: v[0] for k, v in parse_qs(url.query).items() if k != "n"}
            line = json.dumps(fields, ensure_ascii=False)
            print(line, flush=True)
            with open(OUT / "reports.jsonl", "a", encoding="utf-8") as reports:
                reports.write(line + "\n")
            self.send(200, GIF, "image/gif")
        elif url.path == "/ping":
            self.send(200, b'{"app": "probe"}')
        elif url.path == "/jsplugins.xml":
            self.send(200, PLUGINS.encode(), TYPES[".xml"])
        elif url.path.startswith("/addin/"):
            name = url.path.removeprefix("/addin/") or "index.html"
            file = (ADDIN / name).resolve()
            if file.parent != ADDIN or not file.is_file():
                self.send(404, b"{}")
            else:
                self.send(200, file.read_bytes(), TYPES.get(file.suffix, "application/octet-stream"))
        else:
            self.send(404, b"{}")

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length") or 0))
        self.send(200, b"[]")


OUT.mkdir(parents=True, exist_ok=True)
ThreadingHTTPServer(("127.0.0.1", 8977), Handler).serve_forever()
