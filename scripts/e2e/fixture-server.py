#!/usr/bin/env python3
"""Local deterministic HTTP fixture for the desktop E2E smoke test (MAT-135).

Serves /fixture.bin: 4 MiB of a fixed byte pattern. Supports HEAD,
full GET, and single-range GET -> 206 (the download engine errors out
if a server ignores its Range header).
"""

import hashlib
import re
import sys
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SIZE = 4 * 1024 * 1024
BODY = bytes(range(256)) * (SIZE // 256)
SHA256 = hashlib.sha256(BODY).hexdigest()
RANGE_RE = re.compile(r"bytes=(\d+)-(\d*)$")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _headers(self, status, length, extra=()):
        self.send_response(status)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        for k, v in extra:
            self.send_header(k, v)
        self.end_headers()

    def do_HEAD(self):
        if self.path != "/fixture.bin":
            self.send_error(404)
            return
        self._headers(200, SIZE)

    def do_GET(self):
        if self.path != "/fixture.bin":
            self.send_error(404)
            return
        m = RANGE_RE.match(self.headers.get("Range", ""))
        if m:
            start = int(m.group(1))
            end = int(m.group(2)) if m.group(2) else SIZE - 1
            if start >= SIZE or end < start:
                self.send_error(416)
                return
            end = min(end, SIZE - 1)
            chunk = BODY[start : end + 1]
            self._headers(
                206, len(chunk), [("Content-Range", f"bytes {start}-{end}/{SIZE}")]
            )
            self.wfile.write(chunk)
        else:
            self._headers(200, SIZE)
            self.wfile.write(BODY)


def serve(port):
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    return server


def self_test():
    server = serve(0)
    port = server.server_address[1]
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{port}/fixture.bin"

    head = urllib.request.urlopen(urllib.request.Request(base, method="HEAD"))
    assert head.status == 200 and int(head.headers["Content-Length"]) == SIZE
    assert head.headers["Accept-Ranges"] == "bytes"

    full = urllib.request.urlopen(base).read()
    assert hashlib.sha256(full).hexdigest() == SHA256

    req = urllib.request.Request(base, headers={"Range": "bytes=100-259"})
    part = urllib.request.urlopen(req)
    assert part.status == 206
    assert part.headers["Content-Range"] == f"bytes 100-259/{SIZE}"
    assert part.read() == BODY[100:260]

    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/nope")
        raise AssertionError("expected 404")
    except urllib.error.HTTPError as e:
        assert e.code == 404

    server.shutdown()
    print(f"self-test OK (sha256={SHA256})")


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        self_test()
    else:
        srv = serve(int(sys.argv[1]))
        print(f"fixture server on 127.0.0.1:{srv.server_address[1]}", flush=True)
        srv.serve_forever()
