"""Live end-to-end smoke test against a real daemon and agent CLI.

Unlike the Go test suite (fixtures and mocks only), this script starts the
real daemon, creates a real agent session, sends one prompt and verifies the
expected token arrives in the assistant chat stream over SSE. It sends one
real model turn, so the agent CLI must be installed and authenticated, and
the provider account must have working credits for the configured model.

Usage (from the repository root):

    python scripts/live_e2e.py [agent-id] [expected-token] [daemon-exe]

Defaults: agent ``opencode``, token ``OPENCODE_LIVE_OK``, daemon
``bin/openremote.exe``. The workspace is a scratch directory under
``.openremote/`` (git-ignored); provider/model selection comes from the
agent CLI's own configuration in that workspace (for OpenCode set
``model`` in ``opencode.json``, for Pi set ``defaultProvider``/
``defaultModel`` in ``~/.pi/agent/settings.json`` and export the matching
provider API key).

Exit code 0 means the full round trip succeeded.
"""
import json
import os
import subprocess
import sys
import threading
import time
import urllib.request

AGENT = sys.argv[1] if len(sys.argv) > 1 else "opencode"
EXPECT = sys.argv[2] if len(sys.argv) > 2 else "OPENCODE_LIVE_OK"
DAEMON = sys.argv[3] if len(sys.argv) > 3 else os.path.join("bin", "openremote.exe")

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
BASE = "http://127.0.0.1:4098"
TOKEN = "livetoken"
WORKSPACE = os.path.join(ROOT, ".openremote", "qa-workspace")
DATA = os.path.join(ROOT, ".openremote", "qa-data-live")
HEADERS = {"Authorization": "Bearer " + TOKEN}


def call(method, path, body=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=dict(HEADERS))
    if data:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout) as res:
        raw = res.read()
    return json.loads(raw) if raw.strip() else None


def main():
    os.makedirs(WORKSPACE, exist_ok=True)
    os.makedirs(DATA, exist_ok=True)
    proc = subprocess.Popen(
        [os.path.abspath(DAEMON), "serve",
         "--addr", "127.0.0.1:4098", "--root", WORKSPACE, "--data", DATA, "--token", TOKEN],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    log = []
    threading.Thread(target=lambda: [log.append(l) for l in proc.stdout], daemon=True).start()

    try:
        for _ in range(60):
            time.sleep(0.5)
            try:
                if call("GET", "/health", timeout=3):
                    break
            except Exception:
                pass
        else:
            print("DAEMON FAILED TO START")
            print("".join(log[-40:]))
            return 1

        agents = call("GET", "/api/v1/agents")
        ids = [a.get("id") for a in agents] if isinstance(agents, list) else []
        print("agents:", ids)
        if AGENT not in ids:
            print(f"FAIL: {AGENT} agent not registered/available")
            return 1

        session = call("POST", "/api/v1/sessions", {"agentId": AGENT, "cwd": WORKSPACE}, timeout=120)
        sid = session.get("sessionId") or session.get("sessionID")
        print("session:", sid, session.get("status"))

        events = []
        stop = threading.Event()

        def reader():
            req = urllib.request.Request(f"{BASE}/events?sessionId={sid}", headers=HEADERS)
            try:
                with urllib.request.urlopen(req, timeout=180) as res:
                    for raw in res:
                        line = raw.decode("utf-8", "replace").strip()
                        if line.startswith("data:"):
                            try:
                                events.append(json.loads(line[5:].strip()))
                            except Exception:
                                pass
                        if stop.is_set():
                            break
            except Exception as exc:
                events.append({"type": "reader-error", "message": str(exc)})

        threading.Thread(target=reader, daemon=True).start()
        time.sleep(1.0)

        print("prompting...")
        call("POST", f"/api/v1/sessions/{sid}/prompt",
             {"prompt": f"Reply with exactly the token {EXPECT} and nothing else."})

        deadline = time.time() + 150
        while time.time() < deadline:
            time.sleep(1)
            if any(e.get("type") in ("turn.completed", "session.exit") for e in events):
                break
        stop.set()

        assistant = []
        for e in events:
            if e.get("type") == "chat.message" and e.get("kind") == "error":
                assistant.append("[ERROR] " + e.get("text", ""))
            elif e.get("type") == "chat.message" and e.get("role") == "assistant":
                assistant.append(e.get("text", ""))
        print("event types:", sorted({e.get("type") for e in events}))
        print("assistant text:", " | ".join(assistant)[:600])
        if events and not any(EXPECT in a for a in assistant):
            print("--- RAW EVENTS ---")
            for e in events:
                print(json.dumps(e)[:500])
        ok = any(EXPECT in a for a in assistant)
        print("RESULT:", "PASS" if ok else "FAIL")
        call("DELETE", f"/api/v1/sessions/{sid}")
        return 0 if ok else 1
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=15)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())