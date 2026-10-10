#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 step 5 verify: Reader on OmniUI.

Input: the 5 degraded scans of projects/access-pack/tests/bench_read.py (same sources, fonts, seed 1004; two at 200 dpi;
skew / blur / uneven light / noise / JPEG 60). For each scan, `omni-ui measure reader <scan>`:
  - the reader shows exactly the words access.read produced (paragraph text equal at every size)
  - OCR word accuracy vs the source text >= 0.95 overall, >= 0.90 worst page (bench_read's bar)
  - reflow: at 20 / 32 / 48 px (and 26, 40) no box passes the window's right edge; page height rises with text size
  - paging: "next" scrolls when the page is taller than the window and the frame changes
Then one window (scan 1): "bigger" from key ('+'), text, voice (espeak-ng -> whisper.cpp), switch (scan row 2, col 2) and
AT-SPI click -> one action; the AT-SPI tree exposes the paragraphs (role paragraph, name = text).
Side effects, undone at the end: org.a11y.Status IsEnabled true during the window part, then restored; one window.
"""
import json
import os
import random
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
PACK = HERE.parent / "access-pack"
BIN = HERE / "target/release/omni-ui"
sys.path.insert(0, str(PACK)); sys.path.insert(0, str(PACK / "tests")); sys.path.insert(0, str(HERE / "tests"))

import bench_read as br  # noqa: E402  (render, degrade, clean_prose, SOURCES, FONTS, AI, wer_words)
from verify_s3 import a11y_enabled, new_record, set_a11y, send, wait, walk  # noqa: E402

import gi  # noqa: E402
gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402


def find_window(prefix):
    desk = Atspi.get_desktop(0)
    for i in range(desk.get_child_count()):
        app = desk.get_child_at_index(i)
        for j in range(app.get_child_count() if app else 0):
            w = app.get_child_at_index(j)
            if w is not None and (w.get_name() or "").startswith(prefix):
                return w
    return None


def main() -> int:
    from access import bus, speech
    res = {"pages": []}
    tmp = Path(tempfile.mkdtemp(prefix="omni-reader-"))
    env = dict(os.environ, ACCESS_HOME=str(tmp / "home"), OMNI_UI_QUIET="1", OMNI_UI_TEST="1")
    rng = random.Random(1004)
    err = tot = 0
    ok_pages = True
    scans = []
    for i, (src, font) in enumerate(zip(br.SOURCES, br.FONTS)):
        ref = br.clean_prose((br.AI / src).read_text())
        f = tmp / f"scan{i + 1}.jpg"
        br.degrade(br.render(ref, font), rng, low_dpi=i in (1, 3)).save(f, quality=60)
        scans.append(f)
        m = json.loads(subprocess.run([str(BIN), "measure", "reader", str(f)], env=env, capture_output=True, text=True, timeout=300).stdout)
        e, n = br.wer_words(ref, m["text"])
        err += e; tot += n
        sz = {s["px"]: s for s in m["sizes"]}
        heights = [s["page_h"] for s in m["sizes"]]
        page = {"page": i + 1, "words": m["words"], "word_acc": round(1 - e / n, 4),
                "text_equal_all_sizes": all(s["tree_text_equal"] for s in m["sizes"]),
                "max_right_by_px": {s["px"]: s["max_right"] for s in m["sizes"]},
                "page_h_by_px": {s["px"]: s["page_h"] for s in m["sizes"]},
                "frame_ms_by_px": {s["px"]: s["frame_ms"] for s in m["sizes"]},
                "reflow_ok": all(s["max_right"] <= s["width"] for s in m["sizes"]),
                "height_rises": all(b > a for a, b in zip(heights, heights[1:])),
                "paging_ok": all((s["next_scroll"] > 0 and s["next_frame_differs"]) for s in m["sizes"] if s["page_h"] > s["view_h"]),
                "taller_than_window_at": [s["px"] for s in m["sizes"] if s["page_h"] > s["view_h"]]}
        ok_pages &= page["text_equal_all_sizes"] and page["reflow_ok"] and page["height_rises"] and page["paging_ok"] and m["words"] > 0
        res["pages"].append(page)
        print(json.dumps(page))
    # control: a token longer than any line (the engine breaks only at ASCII spaces, a logged finding) must still stay
    # inside the window, and the reader's text must keep the token whole (soft breaks are drawing only)
    tok = "https://example.org/" + "a" * 140
    (tmp / "long.txt").write_text(f"A normal sentence before the long token.\n\n{tok}\n\nA normal sentence after it.\n")
    m = json.loads(subprocess.run([str(BIN), "measure", "reader", str(tmp / "long.txt")], env=env, capture_output=True, text=True).stdout)
    res["control_long_token"] = {"max_right_by_px": {s["px"]: s["max_right"] for s in m["sizes"]}, "token_kept": tok in m["text"].split()}
    ok_pages &= all(s["max_right"] <= s["width"] for s in m["sizes"]) and res["control_long_token"]["token_kept"]
    print(json.dumps(res["control_long_token"]))
    acc = 1 - err / tot
    res["overall_word_acc"] = round(acc, 4)
    ok_ocr = acc >= 0.95 and min(p["word_acc"] for p in res["pages"]) >= 0.90

    # one window: the same "bigger" from every input
    was = a11y_enabled()
    sock, log = tmp / "ui.sock", tmp / "actions.jsonl"
    wenv = dict(env, OMNI_UI_SOCK=str(sock), OMNI_UI_LOG=str(log))
    ui = None
    try:
        set_a11y(True)
        ui = subprocess.Popen([str(BIN), "run", "reader", str(scans[0]), "--px", "20"], env=wenv, stderr=open(tmp / "ui.err", "w"))
        if not wait(lambda: sock.exists(), 60):
            raise SystemExit("reader window did not start: " + (tmp / "ui.err").read_text())
        win = wait(lambda: find_window("Reader — scan1.jpg"), 20, 0.3)
        if not win:
            raise SystemExit("AT-SPI: no Reader window")
        nodes = walk(win)
        paras = [n for _, r, n, _ in nodes if r == "paragraph"]
        m0 = json.loads(subprocess.run([str(BIN), "measure", "reader", str(scans[0])], env=env, capture_output=True, text=True).stdout)
        res["atspi_paragraphs"] = len(paras)
        res["atspi_paragraphs_equal_text"] = paras == m0["text"].split("\n\n")
        res["atspi_roles"] = sorted({r for _, r, _, _ in nodes})
        acts = {}
        acts["key"] = send(sock, {"text": "+"})
        acts["text"] = send(sock, json.loads(bus.intent("text", "bigger").to_json()))
        pcm, rate = speech.synth("Bigger.", rate_wpm=150)
        it = bus.intent("voice", speech.resample(pcm, rate))
        res["voice_transcript"] = it.meta.get("transcript")
        acts["voice"] = send(sock, json.loads(it.to_json()))
        acts["switch"] = send(sock, json.loads(bus.intent("switch", {"presses": [1.9, 3.6]}).to_json()))   # row 2, col 2 = bigger
        # AT-SPI: click Bigger after stepping back down so the click has room to act
        send(sock, {"verb": "smaller", "arg": "", "source": "reset"})
        btn = next(n for _, r, nm, n in walk(win) if r == "push button" and nm == "Bigger")
        ai = btn.get_action_iface()
        n0 = sum(1 for _ in open(log))
        ai.do_action([ai.get_action_name(k) for k in range(ai.get_n_actions())].index("click"))
        acts["at-spi"] = {"action": new_record(log, n0)}
        canon = {k: (v["action"]["action"] if v.get("action") else None) for k, v in acts.items()}
        px = {k: (v["action"].get("px") if v.get("action") else None) for k, v in acts.items()}
        sources = {k: (v["action"].get("source") if v.get("action") else None) for k, v in acts.items()}
        res["canonical"], res["px_after"], res["sources"] = canon, px, sources
        ok_intents = all(c == ["bigger", ""] for c in canon.values()) and set(sources.values()) == {"key", "text", "voice", "switch", "at-spi"}
        ok_at = res["atspi_paragraphs"] > 0 and res["atspi_paragraphs_equal_text"]
    finally:
        if ui:
            ui.terminate()
        set_a11y(was)
        res["a11y_restored"] = a11y_enabled() == was
    res["pass"] = bool(ok_pages and ok_ocr and ok_intents and ok_at)
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results/verify_reader.json").write_text(json.dumps(res, indent=1))
    print(json.dumps({k: res[k] for k in ("overall_word_acc", "atspi_paragraphs", "atspi_paragraphs_equal_text", "atspi_roles",
                                          "voice_transcript", "canonical", "px_after", "a11y_restored", "pass")}, indent=1))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
