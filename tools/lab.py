#!/usr/bin/env -S uv run --script
"""Drive the lab: a world lived through headless, held behind a socket.

The lab (src/lab/, the `lab` skill) runs a district as fast as the machine
goes, samples its numbers every world hour, and stops at an anomaly with the
world saved where it stood. Served, it waits there for commands. This is the
client: every command is one connection, so each is one shell call.

    uv run tools/lab.py start --seed 7          # build, serve in the background, wait for it
    uv run tools/lab.py start --load x.savegame # ...carry on from a snapshot instead
    uv run tools/lab.py run 14d                 # let it go (returns at once)
    uv run tools/lab.py wait                    # until it stops: the end, an anomaly, a pause
    uv run tools/lab.py status
    uv run tools/lab.py metrics steel scrap by=1d
    uv run tools/lab.py units courier
    uv run tools/lab.py unit "Anna"
    uv run tools/lab.py trace Anna 2h
    uv run tools/lab.py help                    # every command
    uv run tools/lab.py stop

    uv run tools/lab.py batch --seed 7 --for 14d   # `lab run`: through, then the report;
                                                   # exits 1 if anything fired

`start` and `batch` take the lab's own options (`--seed`, `--map`, `--load`,
`--for`, `--every`, `--watches`, `--no-watches`, `--out`) and build the release
profile unless given --debug. CARGO_TARGET_DIR is respected. The port is 7071,
or --port / CROWD2X_LAB_PORT; the server's own output goes to
lab-runs/server.log.
"""

import os
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RUNS = ROOT / "lab-runs"
DEFAULT_PORT = 7071
# The server may still be compiling when `start` begins waiting for it.
START_TIMEOUT = 900


def port_from(args):
    """Take --port N out of args, falling back to the environment."""
    port = int(os.environ.get("CROWD2X_LAB_PORT", DEFAULT_PORT))
    if "--port" in args:
        at = args.index("--port")
        port = int(args[at + 1])
        del args[at : at + 2]
    return port


def send(port, line, timeout=None):
    """One command, its whole answer."""
    with socket.create_connection(("127.0.0.1", port), timeout=10) as conn:
        conn.settimeout(timeout)
        conn.sendall(line.encode("utf-8") + b"\n")
        chunks = []
        while True:
            chunk = conn.recv(65536)
            if not chunk:
                break
            chunks.append(chunk)
    return b"".join(chunks).decode("utf-8", errors="replace")


def answering(port):
    try:
        send(port, "status", timeout=5)
        return True
    except OSError:
        return False


def cargo(profile_args, lab_args):
    return ["cargo", "run", "--quiet", *profile_args, "--", "lab", *lab_args]


def profile(args):
    if "--debug" in args:
        args.remove("--debug")
        return []
    return ["--release"]


def start(args, port):
    if answering(port):
        print(f"a lab is already serving on {port}; `stop` it first")
        return 1
    build = profile(args)
    RUNS.mkdir(exist_ok=True)
    log_path = RUNS / "server.log"
    log = open(log_path, "w", encoding="utf-8")
    command = cargo(build, ["serve", "--port", str(port), *args])
    kwargs = {}
    if os.name == "nt":
        kwargs["creationflags"] = subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP
    else:
        kwargs["start_new_session"] = True
    process = subprocess.Popen(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, **kwargs)
    print(f"starting: {' '.join(command)}  (log: {log_path})")
    deadline = time.time() + START_TIMEOUT
    while time.time() < deadline:
        if process.poll() is not None:
            print(f"the lab exited with {process.returncode} before serving:")
            print(log_path.read_text(encoding="utf-8", errors="replace")[-4000:])
            return 1
        if answering(port):
            print(send(port, "status"))
            return 0
        time.sleep(1)
    print(f"the lab did not answer on {port} within {START_TIMEOUT}s; see {log_path}")
    return 1


def batch(args):
    command = cargo(profile(args), ["run", *args])
    return subprocess.call(command, cwd=ROOT)


def main():
    args = sys.argv[1:]
    port = port_from(args)
    if not args:
        print(__doc__)
        return 2
    verb, rest = args[0], args[1:]
    if verb == "start":
        return start(rest, port)
    if verb == "batch":
        return batch(rest)
    if verb == "stop":
        verb = "quit"
    line = " ".join([verb, *rest])
    try:
        # `wait` holds until the run stops; everything else answers at once,
        # though `tick` and `trace` take as long as the world they live through.
        print(send(port, line, timeout=None), end="")
    except ConnectionRefusedError:
        print(f"no lab is serving on {port}: `uv run tools/lab.py start --seed 7`")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
