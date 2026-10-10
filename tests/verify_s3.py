#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 S3 verify: OmniUI v0 Find window.

1. AT-SPI: an assistive-technology client (pyatspi's Atspi 2.0 — the library orca is built on) finds the window and reads the
   roles and names of its widgets from the same packed tree the engine draws.
2. One action from every input: key (the window's key handler via the intent socket), text and voice (access.bus, real audio:
   espeak-ng -> whisper.cpp), switch (access.bus row-column scan), AT-SPI (Atspi Action.do_action "click" on the Find button)
   -> the same action ("find", "council tax refund") and the same top results.

Side effects on this desktop, all undone at the end: org.a11y.Status IsEnabled is set true for the run (what orca does on start;
AccessKit only exposes trees while it is true) and restored; one window opens and closes; a Find executor runs on the bench index.
Usage: python3 projects/omni-ui/tests/verify_s3.py   (exit 0 = pass)
"""
import json
import os
import socket
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
PACK = HERE.parent / "access-pack"
BIN = HERE / "target/release/omni-ui"
BENCH_HOME = Path.home() / ".cache/access-bench/home"
QUERY = "council tax refund"
PLANT = "2026-03-12-council.txt"
sys.path.insert(0, str(PACK))

import gi  # noqa: E402
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def busctl(*a):
    return subprocess.run(["busctl", "--user", *a], capture_output=True, text=True)


def a11y_enabled() -> bool:
    return busctl("get-property", "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled").stdout.strip() == "b true"


def set_a11y(on: bool):
    busctl("set-property", "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Status", "IsEnabled", "b", "true" if on else "false")


def send(sock: Path, obj: dict) -> dict:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(60)
    s.connect(str(sock))
    s.sendall((json.dumps(obj) + "\n").encode())
    f = s.makefile("rb")
    r = json.loads(f.readline())
    f.close(); s.close()
    return r


def wait(pred, secs: float, step: float = 0.1):
    t = time.time()
    while time.time() - t < secs:
        v = pred()
        if v:
            return v
        time.sleep(step)
    return None


def new_record(log, n0, secs=10):
    """The newest action record after line n0, once it parses (the app appends one JSON line per action)."""
    def get():
        L = open(log).read().splitlines()[n0:]
        try:
            return json.loads(L[-1]) if L else None
        except ValueError:
            return None
    return wait(get, secs)


def find_window():
    desk = Atspi.get_desktop(0)
    for i in range(desk.get_child_count()):
        app = desk.get_child_at_index(i)
        if app is None:
            continue
        for j in range(app.get_child_count()):
            w = app.get_child_at_index(j)
            if w is not None and (w.get_name() or "").startswith("Find — OmniUI"):
                return app, w
    return None


def walk(node, depth=0, out=None):
    out = [] if out is None else out
    out.append((depth, node.get_role_name(), node.get_name() or "", node))
    for i in range(node.get_child_count()):
        c = node.get_child_at_index(i)
        if c is not None:
            walk(c, depth + 1, out)
    return out


def main() -> int:
    from access import bus, speech
    result = {"steps": {}}
    tmp = Path(tempfile.mkdtemp(prefix="omni-ui-s3-"))
    ui_sock, log = tmp / "ui.sock", tmp / "actions.jsonl"
    env = dict(os.environ, ACCESS_HOME=str(BENCH_HOME), OMNI_UI_SOCK=str(ui_sock), OMNI_UI_LOG=str(log), OMNI_UI_QUIET="1", OMNI_UI_TEST="1")
    find_sock = BENCH_HOME / "find.sock"
    was = a11y_enabled()
    procs = []
    try:
        if not find_sock.exists() or send_ok(find_sock) is False:
            procs.append(subprocess.Popen([sys.executable, "-c", f"import sys; sys.path.insert(0, {str(PACK)!r}); from access import find; find.serve()"],
                                          env=env, stderr=subprocess.DEVNULL))
            # a stale socket file from a stopped server exists before the new one binds: wait for a real answer
            if not wait(lambda: send_ok(find_sock), 60, 0.3):
                raise SystemExit("Find executor did not answer")
        set_a11y(True)
        t0 = time.time()
        ui = subprocess.Popen([str(BIN), "run"], env=env, stderr=open(tmp / "ui.err", "w"))
        procs.append(ui)
        if not wait(lambda: ui_sock.exists(), 20):
            raise SystemExit("omni-ui window did not start: " + (tmp / "ui.err").read_text())
        result["window_up_ms"] = round((time.time() - t0) * 1000)

        # 1. AT-SPI tree
        Atspi.init() if hasattr(Atspi, "init") else None
        found = wait(find_window, 15, 0.3)
        if not found:
            raise SystemExit("AT-SPI: no window named 'Find — OmniUI' on the accessibility bus")
        app, win = found
        nodes = walk(win)
        result["steps"]["atspi_tree"] = [f"{'  ' * d}{r} {n!r}" for d, r, n, _ in nodes]
        roles = {(r, n) for _, r, n, _ in nodes}
        want = {("frame", "Find — OmniUI"), ("heading", "Find"), ("push button", "Find"), ("push button", "Read aloud")}
        search = [x for x in nodes if x[1] in ("entry", "text", "search", "editbar") or "search" in x[1]]
        ok_tree = want <= roles and bool(search) and search[0][2] == "Search for"
        result["steps"]["atspi_tree_ok"] = ok_tree
        result["app_name"] = app.get_name()

        # 2. one action from every input
        acts = {}
        # key: type into the field (focus starts there), Tab to the Find button, Enter
        send(ui_sock, {"key": "Escape"})
        for ch in QUERY:
            send(ui_sock, {"text": ch})
        send(ui_sock, {"key": "Tab"})
        acts["key"] = send(ui_sock, {"key": "Enter"})["action"]
        # text (typed or dictated sentence) through access.bus
        acts["text"] = send(ui_sock, json.loads(bus.intent("text", f"find {QUERY}").to_json()))["action"]
        # voice: espeak-ng speaks the sentence, whisper.cpp hears it, access.bus parses it
        pcm, rate = speech.synth(f"Find {QUERY}.", rate_wpm=150)
        it = bus.intent("voice", speech.resample(pcm, rate))
        result["voice_transcript"] = it.meta.get("transcript")
        acts["voice"] = send(ui_sock, json.loads(it.to_json()))["action"]
        # switch: row-column scan picks "find"; the argument is the current selection (as for a mouse user's "find this")
        it = bus.intent("switch", {"presses": [0.3, 0.5]}, context={"selection": QUERY})
        acts["switch"] = send(ui_sock, json.loads(it.to_json()))["action"]
        # AT-SPI: clear the field, put the words in through the bus (dictation), then click Find through the accessibility API
        send(ui_sock, {"verb": "type", "arg": QUERY, "source": "dictation"})
        btn = next(n for _, r, nm, n in walk(win) if r == "push button" and nm == "Find")
        act_if = btn.get_action_iface()
        names = [act_if.get_action_name(i) for i in range(act_if.get_n_actions())]
        n0 = sum(1 for _ in open(log))
        act_if.do_action(names.index("click"))
        acts["at-spi"] = new_record(log, n0)
        result["steps"]["actions"] = acts
        canon = {k: tuple(v["action"]) if v else None for k, v in acts.items()}
        tops = {k: (v["top"][0] if v and v["top"] else None) for k, v in acts.items()}
        result["steps"]["canonical"] = {k: list(v) if v else None for k, v in canon.items()}
        result["steps"]["top1"] = tops
        same = len(set(canon.values())) == 1 and canon["key"] == ("find", QUERY)
        same_top = len(set(tops.values())) == 1 and (tops["key"] or "").endswith(PLANT)
        sources = {v["source"] for v in acts.values() if v}

        # 3. the screen reader sees the results (list items read from the same tree)
        time.sleep(0.3)
        links = [nm for _, r, nm, _ in walk(win) if r == "link"]
        result["steps"]["atspi_links"] = links[:5]
        ok_links = bool(links) and links[0] == PLANT

        result["pass"] = bool(ok_tree and same and same_top and ok_links and sources == {"key", "text", "voice", "switch", "at-spi"})
        (HERE / "results").mkdir(exist_ok=True)
        (HERE / "results/verify_s3.json").write_text(json.dumps(result, indent=1, default=str))
        print("\n".join(result["steps"]["atspi_tree"]))
        print(json.dumps({k: result[k] for k in ("window_up_ms", "app_name", "voice_transcript", "pass")}))
        print("canonical:", json.dumps(result["steps"]["canonical"]), "\ntop1 same:", same_top, "· links:", result["steps"]["atspi_links"][:3])
        return 0 if result["pass"] else 1
    finally:
        for p in procs:
            p.terminate()
        set_a11y(was)
        result["a11y_restored"] = a11y_enabled() == was


def send_ok(sock: Path):
    try:
        return "hits" in send(sock, {"q": "x", "k": 1})
    except (OSError, ValueError):
        return False


if __name__ == "__main__":
    sys.exit(main())
