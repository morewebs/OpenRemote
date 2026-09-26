"""Idle daemon memory measurement against the <25 MB design target.

Starts the real daemon with no sessions on a scratch data directory, waits
for startup to settle, then samples the process working set once per second.
Reports min/median/max in MB and exits 0 (this is a measurement, not a gate —
the number is reported honestly whatever it is).

Usage (from the repository root):

    python scripts/measure_idle.py [daemon-exe] [seconds]

Defaults: daemon ``bin/openremote.exe``, 30 seconds of sampling.
"""
import os
import subprocess
import sys
import threading
import time
import urllib.request
import statistics

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
DAEMON = sys.argv[1] if len(sys.argv) > 1 else os.path.join("bin", "openremote.exe")
SECONDS = int(sys.argv[2]) if len(sys.argv) > 2 else 30

BASE = "http://127.0.0.1:4099"
TOKEN = "measuretoken"
WORKSPACE = os.path.join(ROOT, ".openremote", "measure-workspace")
DATA = os.path.join(ROOT, ".openremote", "measure-data")


def memory_mb(pid):
    """Return (working set, private bytes) in MB via PowerShell."""
    out = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         f'"{{0}} {{1}}" -f (Get-Process -Id {pid}).WorkingSet64,'
         f' (Get-Process -Id {pid}).PrivateMemorySize64'],
        capture_output=True, text=True, timeout=10).stdout.strip()
    ws, priv = out.split()
    return int(ws) / (1024 * 1024), int(priv) / (1024 * 1024)


def main():
    os.makedirs(WORKSPACE, exist_ok=True)
    os.makedirs(DATA, exist_ok=True)
    proc = subprocess.Popen(
        [os.path.abspath(DAEMON), "serve",
         "--addr", "127.0.0.1:4099", "--root", WORKSPACE, "--data", DATA, "--token", TOKEN],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    log = []
    threading.Thread(target=lambda: [log.append(l) for l in proc.stdout], daemon=True).start()

    try:
        for _ in range(60):
            time.sleep(0.5)
            try:
                with urllib.request.urlopen(BASE + "/health", timeout=3) as res:
                    if res.status == 200:
                        break
            except Exception:
                pass
        else:
            print("DAEMON FAILED TO START")
            print("".join(log[-40:]))
            return 1

        # Let startup allocation settle before sampling.
        time.sleep(5)

        ws_samples, priv_samples = [], []
        for _ in range(SECONDS):
            try:
                ws, priv = memory_mb(proc.pid)
                ws_samples.append(ws)
                priv_samples.append(priv)
            except Exception as exc:
                print("sample failed:", exc)
            time.sleep(1)

        if not ws_samples:
            print("NO SAMPLES COLLECTED")
            return 1

        print(f"samples: {len(ws_samples)} over {SECONDS}s (idle, no sessions)")
        print(f"working set  MB: min {min(ws_samples):.1f}  median {statistics.median(ws_samples):.1f}  max {max(ws_samples):.1f}")
        print(f"private bytes MB: min {min(priv_samples):.1f}  median {statistics.median(priv_samples):.1f}  max {max(priv_samples):.1f}")
        print(f"TARGET: median working set < 25 MB -> {'MET' if statistics.median(ws_samples) < 25 else 'NOT MET'}")
        return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=15)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    sys.exit(main())
