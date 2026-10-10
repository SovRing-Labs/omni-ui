#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""BROWSER-BODY-0 step 7: peak memory of omni-ui web vs Firefox on the same pages, one page per cold start.
Method: start the browser (fresh process tree; Firefox with a new empty profile, --headless --no-remote), sample every
process in its tree every 0.2 s and sum RSS and PSS (/proc/<pid>/smaps_rollup — PSS splits shared pages fairly between
processes, so a multi-process browser is not over-counted); the score is the peak of each sum over the run.
omni-ui: `omni-ui script web --script` (open URL, hold 5 s) — broker + its sandboxed page process. Firefox: the URL on the
command line, 15 s (cold start + load + settle). Both headless (no window surface). Output: results/browser-body-0/ram.json + ram.md."""
import json, os, shutil, signal, subprocess, sys, tempfile, time
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
OUT = HERE / "results/browser-body-0"
PAGES = [
    "https://en.wikipedia.org/wiki/Ternary_numeral_system",
    "https://en.wikipedia.org/wiki/Moscow",
    "https://doc.rust-lang.org/book/ch08-02-strings.html",
    "https://peps.python.org/pep-0008/",
    "https://html.duckduckgo.com/html/?q=ternary+numeral+system",
    "https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Overview",
    "https://news.ycombinator.com/",
    "https://lwn.net/",
    "https://wiki.archlinux.org/title/Systemd",
    "https://text.npr.org/",
]

def tree(root):
    kids = {}
    for d in os.listdir("/proc"):
        if not d.isdigit(): continue
        try:
            st = open(f"/proc/{d}/stat").read()
            ppid = int(st[st.rindex(")") + 2:].split()[1])
            kids.setdefault(ppid, []).append(int(d))
        except Exception: pass
    out, todo = [], [root]
    while todo:
        p = todo.pop(); out.append(p); todo += kids.get(p, [])
    return out

def mem(pid):
    rss = pss = 0
    try:
        for l in open(f"/proc/{pid}/smaps_rollup"):
            if l.startswith("Rss:"): rss = int(l.split()[1])
            elif l.startswith("Pss:"): pss = int(l.split()[1])
    except Exception: pass
    return rss, pss

def run(cmd, secs, env=None):
    p = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True, env=env)
    peak_rss = peak_pss = 0; nproc = 0; t0 = time.time()
    while time.time() - t0 < secs and p.poll() is None:
        ps = tree(p.pid); r = s = 0
        for q in ps: a, b = mem(q); r += a; s += b
        peak_rss, peak_pss, nproc = max(peak_rss, r), max(peak_pss, s), max(nproc, len(ps))
        time.sleep(0.2)
    try: os.killpg(p.pid, signal.SIGTERM)
    except Exception: pass
    try: p.wait(5)
    except Exception:
        try: os.killpg(p.pid, signal.SIGKILL)
        except Exception: pass
    return {"peak_rss_kb": peak_rss, "peak_pss_kb": peak_pss, "procs": nproc}

TAG = os.environ.get("RAM_TAG", "")  # "-js": the same run with page scripts on (JS-0); the first table was measured before JS existed

def main():
    pages = PAGES[: int(sys.argv[1])] if len(sys.argv) > 1 else PAGES
    exe = str(HERE / "target/release/omni-ui")
    rows = []
    for u in pages:
        sc = Path(tempfile.mkdtemp(prefix="ramb-")); (sc / "s.txt").write_text(f"open {u}\nhold 5000\n")
        o = run([exe, "script", "web", "--script", str(sc / "s.txt")], 40, env={**os.environ, "OMNI_WEB_JS": "1" if TAG == "-js" else "0"})
        prof = sc / "ffprof"; prof.mkdir()
        f = run(["firefox", "--headless", "--no-remote", "--new-instance", "--profile", str(prof), u], 15, env={**os.environ, "MOZ_HEADLESS": "1"})
        shutil.rmtree(sc, ignore_errors=True)
        r = {"url": u, "omni": o, "firefox": f, "pss_ratio": round(o["peak_pss_kb"] / max(f["peak_pss_kb"], 1), 3)}
        rows.append(r); print(json.dumps(r), flush=True)
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / f"ram{TAG}.json").write_text(json.dumps(rows, indent=1))
    tot_o = sum(r["omni"]["peak_pss_kb"] for r in rows); tot_f = sum(r["firefox"]["peak_pss_kb"] for r in rows)
    md = ["# BROWSER-BODY-0 step 7 — peak memory, omni-ui web vs Firefox (one page per cold start)", "",
          f"Firefox: `{subprocess.run(['firefox', '--version'], capture_output=True, text=True).stdout.strip()}`, new empty profile, `--headless --no-remote`, 15 s.",
          f"omni-ui web: `omni-ui script web` (broker + locked-down page process), open + hold 5 s, headless; page scripts {'ON (jsi)' if TAG == '-js' else 'not run (measured before JS-0)'}.",
          "Score: peak over the run of the summed PSS (and RSS) of every process in the browser's tree, sampled every 0.2 s (tools/ram_bench.py).", "",
          "| Page | omni PSS MB | Firefox PSS MB | ratio | omni RSS MB | Firefox RSS MB | procs omni / ff |", "|---|---|---|---|---|---|---|"]
    for r in rows:
        o, f = r["omni"], r["firefox"]
        md.append(f"| {r['url']} | {o['peak_pss_kb']/1024:.1f} | {f['peak_pss_kb']/1024:.1f} | {r['pss_ratio']*100:.1f} % | {o['peak_rss_kb']/1024:.1f} | {f['peak_rss_kb']/1024:.1f} | {o['procs']} / {f['procs']} |")
    md += ["", f"**Total PSS: omni {tot_o/1024:.1f} MB vs Firefox {tot_f/1024:.1f} MB = {tot_o/max(tot_f,1)*100:.1f} %** (bar: ≤ 25 %).", ""]
    (OUT / f"ram{TAG}.md").write_text("\n".join(md))
    print("\n".join(md))

if __name__ == "__main__":
    main()
