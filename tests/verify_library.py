#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 step L verify: Executive Library on OmniUI (the launcher's Library / Executive Reader, native, read-only).
1. Essence = the launcher's own extractEssence (opencode-launcher ui/src/lib/markdownRenderer.js), run by node on every corpus
   document: byte-identical for every document.
2. Corpus = an independent Python walk of the same shelves (roots, recursion, .md, dot-folders skipped): same documents.
3. Every document, essence and full text, every part: no drawn ink past the window (`omni-ui inkcheck library`).
4. Window (OMNI_UI_TEST): AT-SPI reads the shelf list; an AT-SPI click opens a document; the Full / Essence toggle; 6 documents
   opened then "back" from key (Escape), text, voice, switch (row 3, col 0), AT-SPI click -> the history walked back exactly.
5. strace of the window and the all-documents render: 0 write-mode opens / renames / deletes under any shelf root.
Output: counts and booleans only.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
PACK = HERE.parent / "access-pack"
BIN = HERE / "target/release/omni-ui"
RENDERER = Path.home() / "projects/opencode-launcher/ui/src/lib/markdownRenderer.js"
sys.path.insert(0, str(PACK)); sys.path.insert(0, str(HERE / "tests"))
from verify_s3 import a11y_enabled, new_record, set_a11y, send, wait, walk  # noqa: E402
from verify_reader import find_window  # noqa: E402

H = Path.home()
SHELVES = [(H / "ai-stack/control-center/governance", False), (H / "ai-stack/docs/systems", True), (H / "ai-stack/_Reviews", True),
           (H / "Documents/Obsidian/_Reviews", True), (H / "Documents/Obsidian/200-Business/Business-Proposals", True),
           (H / "ai-stack/control-center", False)]
TRACE = ["strace", "-f", "-qq", "-e", "trace=open,openat,creat,truncate,rename,renameat,renameat2,unlink,unlinkat,mkdir,mkdirat"]


def py_corpus():
    out = []
    for root, rec in SHELVES:
        def go(d, depth):
            try:
                ents = list(os.scandir(d))
            except OSError:
                return
            for e in ents:
                if e.name.startswith("."):
                    continue
                if e.is_dir():
                    if rec and depth < 6:
                        go(e.path, depth + 1)
                elif e.name.lower().endswith(".md"):
                    out.append(e.path)
        go(str(root), 0)
    return out


def writes(trace: Path):
    roots = [str(r) for r, _ in SHELVES] + [os.path.realpath(str(r)) for r, _ in SHELVES]
    bad = []
    for line in trace.read_text(errors="replace").splitlines():
        if not any(r in line for r in roots):
            continue
        if ("open" in line and any(f in line for f in ("O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC"))) or \
           any(line.split("(", 1)[0].strip().endswith(c) for c in ("creat", "truncate", "rename", "renameat", "renameat2", "unlink", "unlinkat", "mkdir", "mkdirat")):
            bad.append(line[:160])
    return bad


def main() -> int:
    res = {}
    tmp = Path(tempfile.mkdtemp(prefix="omni-library-"))
    # 1. essence vs the launcher's own function
    ours = json.loads(subprocess.run([str(BIN), "essence-dump"], capture_output=True, text=True, check=True, timeout=600).stdout)
    mjs = tmp / "markdownRenderer.mjs"
    shutil.copy(RENDERER, mjs)  # read-only copy so node loads it as an ES module
    (tmp / "paths.json").write_text(json.dumps(sorted(ours)))
    node = f"""
import {{ extractEssence }} from '{mjs}';
import fs from 'fs';
const paths = JSON.parse(fs.readFileSync('{tmp / "paths.json"}', 'utf8'));
const out = {{}};
for (const p of paths) out[p] = extractEssence(fs.readFileSync(p, 'utf8'));
fs.writeFileSync('{tmp / "theirs.json"}', JSON.stringify(out));
"""
    subprocess.run(["node", "--input-type=module", "-e", node], check=True, timeout=600)
    theirs = json.loads((tmp / "theirs.json").read_text())
    same = sum(ours[p] == theirs[p] for p in ours)
    res["essence"] = {"docs": len(ours), "identical": same, "different": len(ours) - same,
                      "sections_found": sum("Isolating Problem" in v for v in ours.values()), "summary_slice": sum("Summary Slice" in v for v in ours.values())}
    ok_essence = same == len(ours) and len(ours) > 0

    # 2. corpus
    py = sorted(py_corpus())
    res["corpus"] = {"ours": len(ours), "python_walk": len(py), "same_documents": sorted(ours) == py}

    # 3. render every document
    ink = json.loads(subprocess.run([*TRACE, "-o", str(tmp / "ink.trace"), str(BIN), "inkcheck", "library"], env=dict(os.environ, OMNI_UI_QUIET="1"),
                                    capture_output=True, text=True, check=True, timeout=3600).stdout)
    rows = ink["rows"]
    ms = sorted(r["ms"] for r in rows)
    res["render_all"] = {"docs": ink["docs"], "views": len(rows), "past_edge": sum(r["max_right"] > ink["width"] for r in rows),
                         "median_ms": ms[len(ms) // 2], "max_ms": ms[-1], "most_parts": max(r["parts"] for r in rows), "total_s": ink["total_s"]}
    ok_render = res["render_all"]["past_edge"] == 0

    # 4. window
    from access import bus, speech
    sock, log = tmp / "ui.sock", tmp / "actions.jsonl"
    was = a11y_enabled()
    ui = None
    try:
        set_a11y(True)
        ui = subprocess.Popen([*TRACE, "-o", str(tmp / "ui.trace"), str(BIN), "run", "library"],
                              env=dict(os.environ, OMNI_UI_QUIET="1", OMNI_UI_TEST="1", OMNI_UI_SOCK=str(sock), OMNI_UI_LOG=str(log)),
                              stderr=open(tmp / "ui.err", "w"))
        if not wait(lambda: sock.exists(), 60):
            raise SystemExit("library window did not start: " + (tmp / "ui.err").read_text())
        win = wait(lambda: find_window("Library — Executive"), 20, 0.3)
        if not win:
            raise SystemExit("AT-SPI: no Library window")
        links = [n for _, r, nm, n in walk(win) if r == "link"]
        res["atspi_links"] = len(links)
        ai = links[0].get_action_iface()
        ai.do_action([ai.get_action_name(k) for k in range(ai.get_n_actions())].index("click"))
        st = wait(lambda: (lambda s: s if s["open"] else None)(send(sock, {"query": "state"})["state"]), 10)
        res["atspi_click_opened_a_doc"] = bool(st)
        res["essence_default"] = bool(st) and st["full"] is False
        r = send(sock, {"verb": "read", "arg": "full", "source": "probe"})
        res["full_toggle"] = r["state"]["full"] is True
        send(sock, {"verb": "back", "arg": "", "source": "reset"})
        chain = sorted(ours)[:6]
        for p in chain:
            send(sock, {"verb": "open", "arg": p, "source": "follow"})
        acts = {"key": send(sock, {"key": "Escape"}), "text": send(sock, json.loads(bus.intent("text", "back").to_json()))}
        pcm, rate = speech.synth("Back.", rate_wpm=150)
        it = bus.intent("voice", speech.resample(pcm, rate))
        res["voice_transcript"] = it.meta.get("transcript")
        acts["voice"] = send(sock, json.loads(it.to_json()))
        acts["switch"] = send(sock, json.loads(bus.intent("switch", {"presses": [2.7, 2.9]}).to_json()))
        btn = next(n for _, r2, nm, n in walk(win) if r2 == "push button" and nm == "Back")
        bi = btn.get_action_iface()
        n0 = sum(1 for _ in open(log))
        bi.do_action([bi.get_action_name(k) for k in range(bi.get_n_actions())].index("click"))
        acts["at-spi"] = {"action": new_record(log, n0)}
        canon = {k: (v["action"]["action"] if v.get("action") else None) for k, v in acts.items()}
        cur = [v["action"].get("open") if v.get("action") else None for v in acts.values()]
        res["canonical"] = canon
        res["history_walked_back_exactly"] = cur == list(reversed(chain[:-1]))
        sources = {v["action"].get("source") for v in acts.values() if v.get("action")}
        ok_window = (res["atspi_links"] > 0 and res["atspi_click_opened_a_doc"] and res["essence_default"] and res["full_toggle"]
                     and all(c == ["back", ""] for c in canon.values()) and res["history_walked_back_exactly"]
                     and sources == {"key", "text", "voice", "switch", "at-spi"})
    finally:
        if ui:
            ui.terminate()
            try:
                ui.wait(timeout=10)
            except Exception:
                pass
        set_a11y(was)
        res["a11y_restored"] = a11y_enabled() == was
    w = writes(tmp / "ink.trace") + writes(tmp / "ui.trace")
    res["writes_under_shelves"] = len(w)
    res["first_writes"] = w[:3]
    res["pass"] = bool(ok_essence and res["corpus"]["same_documents"] and ok_render and ok_window and not w)
    (HERE / "results/verify_library.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
