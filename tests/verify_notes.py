#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 N2 verify: Notes on OmniUI over J's Obsidian vault — READ-FORWARD (J: "Obsidian stays the brain"; "never
silently corrupt a real note").
1. Vault untouched: sha256 of every note before = after; strace of the app (window + all-notes render): 0 write-mode opens,
   renames, deletes or mkdirs under the vault; other files that changed must sit inside a symlinked live folder (fleet daemons).
2. Every note rendered once (`omni-ui inkcheck notes`): no crash, no drawn ink past the window edge, ms per note.
3. Window (OMNI_UI_TEST: no keyboard focus taken): AT-SPI reads the home list; an AT-SPI click opens a note; links followed
   to depth 5 through the intent socket along real resolved links; then "back" from key (Escape), text, voice (espeak-ng ->
   whisper.cpp), switch (row 3, col 0) and AT-SPI click -> one action each, walking the history back exactly.
Output: counts and booleans only — no note names or text.
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
PACK = HERE.parent / "access-pack"
BIN = HERE / "target/release/omni-ui"
VAULT = Path.home() / "Documents/Obsidian"
sys.path.insert(0, str(PACK)); sys.path.insert(0, str(HERE / "tests"))
from verify_s3 import a11y_enabled, new_record, set_a11y, send, wait, walk  # noqa: E402


def snapshot():
    notes, files, seen = {}, {}, set()
    for dp, dns, fns in os.walk(VAULT, followlinks=True):
        real = os.path.realpath(dp)
        if real in seen:
            dns[:] = []
            continue
        seen.add(real)
        dns[:] = [d for d in dns if not d.startswith(".")]
        for f in fns:
            p = os.path.join(dp, f)
            try:
                st = os.stat(p)
            except OSError:
                continue
            if f.lower().endswith(".md"):
                notes[p] = hashlib.sha256(open(p, "rb").read()).hexdigest()
            else:
                files[p] = (st.st_size, st.st_mtime_ns)
    return notes, files


TRACE = ["strace", "-f", "-qq", "-e", "trace=open,openat,creat,truncate,rename,renameat,renameat2,unlink,unlinkat,mkdir,mkdirat"]


def vault_writes(trace: Path) -> list:
    """Syscalls in an strace log that could change the vault: write-mode opens, renames, deletes, mkdirs under the vault or
    under the real path of any symlinked folder inside it."""
    roots = [str(VAULT)] + [os.path.realpath(p) for p in VAULT.iterdir() if p.is_symlink()]
    bad = []
    for line in trace.read_text(errors="replace").splitlines():
        if not any(r in line for r in roots):
            continue
        if ("open" in line and any(f in line for f in ("O_WRONLY", "O_RDWR", "O_CREAT", "O_TRUNC"))) or \
           any(line.split("(", 1)[0].strip().endswith(c) for c in ("creat", "truncate", "rename", "renameat", "renameat2", "unlink", "unlinkat", "mkdir", "mkdirat")):
            bad.append(line[:200])
    return bad


def main() -> int:
    from access import bus, speech
    res = {}
    before = snapshot()
    res["snapshot"] = {"notes_hashed": len(before[0]), "attachments_stat": len(before[1])}

    tmp = Path(tempfile.mkdtemp(prefix="omni-notes-"))
    ink = json.loads(subprocess.run([*TRACE, "-o", str(tmp / "ink.trace"), str(BIN), "inkcheck", "notes", str(VAULT)], env=dict(os.environ, OMNI_UI_QUIET="1"),
                                    capture_output=True, text=True, check=True, timeout=3600).stdout)
    rows = ink["rows"]
    ms = sorted(r["ms"] for r in rows)
    res["render_all"] = {"notes": ink["notes"], "past_edge": sum(r["max_right"] > ink["width"] for r in rows),
                         "worst_ink": max(r["max_right"] for r in rows), "median_ms": ms[len(ms) // 2], "p95_ms": ms[int(len(ms) * 0.95)],
                         "max_ms": ms[-1], "total_s": ink["total_s"], "max_widgets_per_part": max(r["widgets"] for r in rows),
                         "notes_in_parts": sum(r["parts"] > 1 for r in rows), "most_parts": max(r["parts"] for r in rows)}
    ok_render = ink["notes"] == len(before[0]) and res["render_all"]["past_edge"] == 0

    # a chain of 5 real resolved links to follow (from the resolver's own dump)
    dump = json.loads(subprocess.run([str(BIN), "vault-check", str(VAULT), "--dump"], capture_output=True, text=True, check=True).stdout)["links_dump"]
    out = {}
    for src, kind, tgt, got, amb in dump:
        if got.endswith(".md") and got != src:
            out.setdefault(src, []).append(got)
    chain = None
    for start in sorted(out):
        c, cur = [start], start
        while len(c) < 6 and cur in out:
            nxt = next((x for x in out[cur] if x not in c), None)
            if not nxt:
                break
            c.append(nxt); cur = nxt
        if len(c) == 6:
            chain = c
            break
    if not chain:
        raise SystemExit("no 6-note link chain in the vault")

    sock, log = tmp / "ui.sock", tmp / "actions.jsonl"
    was = a11y_enabled()
    ui = None
    try:
        set_a11y(True)
        ui = subprocess.Popen([*TRACE, "-o", str(tmp / "ui.trace"), str(BIN), "run", "notes", str(VAULT)],
                              env=dict(os.environ, OMNI_UI_QUIET="1", OMNI_UI_TEST="1", OMNI_UI_SOCK=str(sock), OMNI_UI_LOG=str(log)),
                              stderr=open(tmp / "ui.err", "w"))
        if not wait(lambda: sock.exists(), 60):
            raise SystemExit("notes window did not start: " + (tmp / "ui.err").read_text())
        from verify_reader import find_window
        win = wait(lambda: find_window("Notes — vault"), 20, 0.3)
        if not win:
            raise SystemExit("AT-SPI: no Notes window")
        links = [n for _, r, nm, n in walk(win) if r == "link"]
        res["atspi_home_links"] = len(links)
        ai = links[0].get_action_iface()
        ai.do_action([ai.get_action_name(k) for k in range(ai.get_n_actions())].index("click"))
        st = wait(lambda: (lambda s: s if s["current"] else None)(send(sock, {"query": "state"})["state"]), 10)
        res["atspi_click_opened_a_note"] = bool(st)
        nodes = walk(win)
        res["atspi_note_roles"] = sorted({r for _, r, _, _ in nodes})
        send(sock, {"verb": "back", "arg": "", "source": "reset"})   # home again
        for rel in chain:
            send(sock, {"verb": "open", "arg": rel, "source": "follow"})
        st = send(sock, {"query": "state"})["state"]
        res["followed_depth"] = st["depth"]
        acts = {"key": send(sock, {"key": "Escape"}), "text": send(sock, json.loads(bus.intent("text", "back").to_json()))}
        pcm, rate = speech.synth("Back.", rate_wpm=150)
        it = bus.intent("voice", speech.resample(pcm, rate))
        res["voice_transcript"] = it.meta.get("transcript")
        acts["voice"] = send(sock, json.loads(it.to_json()))
        acts["switch"] = send(sock, json.loads(bus.intent("switch", {"presses": [2.7, 2.9]}).to_json()))   # row 3, col 0 = back
        btn = next(n for _, r, nm, n in walk(win) if r == "push button" and nm == "Back")
        bi = btn.get_action_iface()
        n0 = sum(1 for _ in open(log))
        bi.do_action([bi.get_action_name(k) for k in range(bi.get_n_actions())].index("click"))
        acts["at-spi"] = {"action": new_record(log, n0)}
        canon = {k: (v["action"]["action"] if v.get("action") else None) for k, v in acts.items()}
        cur = [v["action"].get("current") if v.get("action") else None for v in acts.values()]
        sources = {v["action"].get("source") for v in acts.values() if v.get("action")}
        res["canonical"] = canon
        res["history_walked_back_exactly"] = cur == list(reversed(chain[:-1]))
        ok_window = (res["atspi_home_links"] > 0 and res["atspi_click_opened_a_note"] and res["followed_depth"] >= 5
                     and all(c == ["back", ""] for c in canon.values()) and res["history_walked_back_exactly"]
                     and sources == {"key", "text", "voice", "switch", "at-spi"})
    finally:
        if ui:
            ui.terminate()
        set_a11y(was)
        res["a11y_restored"] = a11y_enabled() == was
    after = snapshot()
    if ui:
        try:
            ui.wait(timeout=10)
        except Exception:
            pass
    writes = vault_writes(tmp / "ink.trace") + vault_writes(tmp / "ui.trace")
    changed = [p for p in set(before[1]) | set(after[1]) if before[1].get(p) != after[1].get(p)]
    live = [os.path.relpath(p, VAULT) for p in VAULT.iterdir() if p.is_symlink()]
    res["vault_untouched"] = {"notes_same": before[0] == after[0], "app_write_syscalls_under_vault": len(writes), "first_writes": writes[:3],
                              "other_files_changed": len(changed),
                              "changed_inside_symlinked_live_folders": sum(any(os.path.relpath(p, VAULT).startswith(l + "/") for l in live) for p in changed),
                              "symlinked_live_folders": live}
    ok_untouched = before[0] == after[0] and not writes and res["vault_untouched"]["changed_inside_symlinked_live_folders"] == len(changed)
    res["pass"] = bool(ok_render and ok_window and ok_untouched)
    (HERE / "results/verify_notes.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
