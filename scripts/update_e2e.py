"""Offline end-to-end self-update test: old daemon -> fake release feed -> new daemon.

Builds two daemon binaries (0.9.0 and 0.10.0), serves a GitHub-style release
feed on loopback, runs the old daemon with `serve` (full supervisor path),
asks it to update via POST /api/v1/update, and asserts the daemon comes back
running the new version with the old binary preserved as <exe>.old.

A second scenario feeds a wrong SHA256 and asserts the daemon refuses the
update and keeps running the old version.

No network access beyond loopback. Usage (from the repository root):

    python scripts/update_e2e.py
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SCRATCH = os.path.join(ROOT, ".openremote", "update-e2e")
ADDR = "127.0.0.1:4101"
TOKEN = "updatetoken"
ASSET = "openremote-windows-amd64.exe" if os.name == "nt" else "openremote-linux-amd64"


class Feed(BaseHTTPRequestHandler):
    new_binary = b""
    mangle = False

    def do_GET(self):
        if self.path == "/latest":
            body = json.dumps({
                "tag_name": "v0.10.0",
                "assets": [
                    {"name": ASSET, "browser_download_url": f"http://127.0.0.1:{self.server.server_port}/binary"},
                    {"name": "SHA256SUMS.txt", "browser_download_url": f"http://127.0.0.1:{self.server.server_port}/sums"},
                ],
            }).encode()
            self._respond(body, "application/json")
        elif self.path == "/binary":
            self._respond(self.new_binary, "application/octet-stream")
        elif self.path == "/sums":
            digest = hashlib.sha256(self.new_binary).hexdigest()
            if self.mangle:
                digest = "0000" + digest[4:]
            self._respond(f"{digest}  {ASSET}\n".encode(), "text/plain")
        else:
            self.send_error(404)

    def _respond(self, body, ctype):
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def call(method, path, body=None, timeout=10):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        f"http://{ADDR}{path}", data=data, method=method,
        headers={"Authorization": "Bearer " + TOKEN} if path != "/health" else {})
    if data:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout) as res:
        raw = res.read()
    return json.loads(raw) if raw.strip() else None


def build(tag, output):
    env = dict(os.environ, CGO_ENABLED="0")
    subprocess.run(
        ["go", "build", "-trimpath", "-ldflags", f"-X main.Version={tag}", "-o", output, "./cmd/openremote"],
        cwd=ROOT, env=env, check=True)


def health_version():
    for _ in range(90):
        try:
            h = call("GET", "/health")
            return h.get("version", "")
        except Exception:
            time.sleep(1)
    return ""


def wait_health(expected, deadline_s=90):
    end = time.time() + deadline_s
    while time.time() < end:
        try:
            h = call("GET", "/health")
            if h.get("version") == expected:
                return True
        except Exception:
            pass
        time.sleep(1)
    return False


def wait_file_writable(path, timeout_s=20):
    """Wait out the old worker's shutdown grace period before reusing the exe path."""
    end = time.time() + timeout_s
    while time.time() < end:
        try:
            with open(path, "ab"):
                return True
        except PermissionError:
            time.sleep(1)
    return False


def run_scenario(mangle):
    data = os.path.join(SCRATCH, "data-mismatch" if mangle else "data-ok")
    shutil.rmtree(data, ignore_errors=True)
    os.makedirs(data, exist_ok=True)
    exe = os.path.join(SCRATCH, "openremote-old.exe" if os.name == "nt" else "openremote-old")

    # A pristine old binary per scenario (the previous scenario's was swapped).
    if not wait_file_writable(exe):
        print("previous daemon still holds the binary; aborting")
        return False
    shutil.copyfile(os.path.join(SCRATCH, "old-src.exe" if os.name == "nt" else "old-src"), exe)
    for stale in (exe + ".old",):
        if os.path.exists(stale):
            os.remove(stale)

    Feed.mangle = mangle
    proc = subprocess.Popen(
        [exe, "serve", "--addr", ADDR, "--data", data, "--token", TOKEN,
         "--update-url", f"http://127.0.0.1:{FEED_PORT}/latest"],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log = []
    threading.Thread(target=lambda: [log.append(l) for l in proc.stdout], daemon=True).start()
    try:
        if not wait_health("0.9.0", 30):
            print("DAEMON FAILED TO START")
            print("".join(log[-30:]))
            return False

        call("POST", "/api/v1/update")

        if mangle:
            # Refusal: daemon stays alive on 0.9.0 and reports the reason.
            end = time.time() + 60
            error = ""
            while time.time() < end:
                st = call("GET", "/api/v1/update")
                if st.get("applying") is False and st.get("applyError"):
                    error = st["applyError"]
                    break
                time.sleep(1)
            ok = "checksum mismatch" in error and wait_health("0.9.0", 5) \
                and not os.path.exists(exe + ".old")
            print(f"mismatch scenario: {'PASS' if ok else 'FAIL'} ({error or 'no error surfaced'})")
            return ok

        # Success: the supervisor restarts the daemon on the new version.
        if not wait_health("0.10.0", 90):
            print("DAEMON NEVER CAME BACK ON 0.10.0")
            print("".join(log[-30:]))
            return False
        ok = os.path.exists(exe + ".old")
        print(f"update scenario: {'PASS' if ok else 'FAIL'} "
              f"(daemon now {health_version()}, .old present: {ok})")
        return ok
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=15)
        except Exception:
            proc.kill()


def main():
    os.makedirs(SCRATCH, exist_ok=True)
    old_src = os.path.join(SCRATCH, "old-src.exe" if os.name == "nt" else "old-src")
    new_bin = os.path.join(SCRATCH, "new-src.exe" if os.name == "nt" else "new-src")
    print("building test binaries (go build, ~1 min)...")
    build("0.9.0", old_src)
    build("0.10.0", new_bin)
    Feed.new_binary = open(new_bin, "rb").read()

    global FEED_PORT
    server = ThreadingHTTPServer(("127.0.0.1", 0), Feed)
    FEED_PORT = server.server_port
    threading.Thread(target=server.serve_forever, daemon=True).start()

    try:
        ok = run_scenario(mangle=False) and run_scenario(mangle=True)
        print("RESULT:", "PASS" if ok else "FAIL")
        return 0 if ok else 1
    finally:
        server.shutdown()


if __name__ == "__main__":
    sys.exit(main())
