#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 N1 verify: the vault index and link resolver (src/vault.rs).
1. Fixture vault (tests/fixtures/vault): 20 expected resolutions from Obsidian's documented rules; links inside code are not
   links; hidden folders (.obsidian, .trash) are not indexed; a symlink loop does not hang the index.
2. J's vault (read-only): a SECOND implementation of the same rules, written here in Python from the rules (not from the Rust),
   resolves every link the Rust index found -> agreement per link; disagreements listed by rule, never by note content.
3. ~/bin/vault-link-check (the fleet's existing checker) on a sample of plain [[name]] links: where it and we differ, by its
   known limits (case-sensitive names; no attachments).
Reports counts only — no note names or text leave the machine in this output.
"""
import importlib.machinery
import importlib.util
import json
import os
import random
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from urllib.parse import unquote

HERE = Path(__file__).resolve().parents[1]
BIN = HERE / "target/release/omni-ui"
VAULT = Path.home() / "Documents/Obsidian"


def run_check(root, *extra):
    return json.loads(subprocess.run([str(BIN), "vault-check", str(root), *extra], capture_output=True, text=True, check=True, timeout=300).stdout)


class Ref:
    """Python resolver, written from the rules: case-insensitive; exact path from the vault root (with or without .md);
    ./ ../ and markdown links relative to the source folder; path relative to the source folder; partial path suffix;
    unique name; duplicate name -> same folder, else shortest path then alphabetical (flagged ambiguous)."""

    def __init__(self, root: Path):
        self.notes, self.files = [], []
        seen = set()
        for dp, dns, fns in os.walk(root, followlinks=True):
            real = os.path.realpath(dp)
            if real in seen:
                dns[:] = []
                continue
            seen.add(real)
            dns[:] = [d for d in dns if not d.startswith(".")]
            for f in fns:
                if f.startswith("."):
                    continue
                rel = os.path.relpath(os.path.join(dp, f), root).replace(os.sep, "/")
                (self.notes if f.lower().endswith(".md") else self.files).append(rel)
        self.np = {n[:-3].lower(): n for n in self.notes}
        self.fp = {f.lower(): f for f in self.files}
        self.nn, self.fn = {}, {}
        for n in self.notes:
            self.nn.setdefault(n[:-3].lower().split("/")[-1], []).append(n)
        for f in self.files:
            self.fn.setdefault(f.lower().split("/")[-1], []).append(f)

    @staticmethod
    def norm(p):
        out = []
        for part in p.split("/"):
            if part in ("", "."):
                continue
            if part == "..":
                out and out.pop()
            else:
                out.append(part)
        return "/".join(out)

    @staticmethod
    def pick(c, src_folder):
        same = [x for x in c if x.rsplit("/", 1)[0].lower() == src_folder.lower() and "/" in x] or [x for x in c if "/" not in x and src_folder == ""]
        return same[0] if same else sorted(c, key=lambda x: (len(x), x))[0]

    def resolve(self, target, source, kind):
        t = target.strip()
        if not t:
            return "SAME", False
        sf = source.rsplit("/", 1)[0] if "/" in source else ""
        strip = lambda k: k[:-3] if k.endswith(".md") else k  # noqa: E731
        rel = self.norm(f"{sf}/{t}") if (t.startswith(("./", "../")) or (kind == "Md" and not t.startswith("/"))) else self.norm(t)
        for key in (rel.lower(), self.norm(t).lower()):
            if strip(key) in self.np:
                return self.np[strip(key)], False
            if key in self.fp:
                return self.fp[key], False
        k = self.norm(f"{sf}/{t}").lower()
        if strip(k) in self.np:
            return self.np[strip(k)], False
        if k in self.fp:
            return self.fp[k], False
        root_lc = self.norm(t).lower()
        name = root_lc.split("/")[-1]
        if "/" in root_lc:
            c = [n for n in self.nn.get(strip(name), []) if ("/" + n[:-3].lower()).endswith("/" + strip(root_lc))]
            if c:
                return self.pick(c, sf), len(c) > 1
        if strip(name) in self.nn:
            c = self.nn[strip(name)]
            return self.pick(c, sf), len(c) > 1
        if name in self.fn:
            c = self.fn[name]
            return self.pick(c, sf), len(c) > 1
        return "UNRESOLVED", False


def main() -> int:
    res = {}
    # 1. fixture
    fx = run_check(HERE / "tests/fixtures/vault", "--expect", str(HERE / "tests/fixtures/vault-expect.json"))
    e = fx["expect"]
    loop = Path(tempfile.mkdtemp(prefix="vault-loop-"))
    (loop / "a").mkdir(); (loop / "a" / "Note.md").write_text("[[Note]]\n"); (loop / "a" / "back").symlink_to(loop)
    t0 = time.time()
    lp = run_check(loop)
    res["fixture"] = {"pass": e["pass"], "total": e["total"], "code_links_leaked": e["code_links_leaked"],
                      "hidden_folders_indexed": e["hidden_folders_indexed"], "failed": [r for r in e["rows"] if not r["pass"]],
                      "symlink_loop": {"notes": lp["notes"], "sec": round(time.time() - t0, 2)}}
    ok_fx = e["pass"] == e["total"] and e["code_links_leaked"] == 0 and not e["hidden_folders_indexed"] and lp["notes"] == 1

    # 2. the author's vault: Rust index vs the Python resolver, per link
    dump = run_check(VAULT, "--dump")
    links = dump.pop("links_dump")
    res["vault"] = dump
    ref = Ref(VAULT)
    res["vault_note_count_match"] = len(ref.notes) == dump["notes"] and len(ref.files) == dump["attachments"]
    agree = 0
    diff = {}
    for src, kind, tgt, got, amb in links:
        want, wamb = ref.resolve(unquote(tgt) if kind == "Md" else tgt, src, kind)
        if (want == got or (want == "SAME" and got == "SAME")) and wamb == amb:
            agree += 1
        else:
            k = f"{kind}: ours={'UNRESOLVED' if got == 'UNRESOLVED' else 'resolved'} ref={'UNRESOLVED' if want == 'UNRESOLVED' else 'resolved'} amb={amb}/{wamb}"
            diff[k] = diff.get(k, 0) + 1
    res["vault_vs_python_reference"] = {"links": len(links), "agree": agree, "rate": round(agree / max(1, len(links)), 4), "disagreements_by_rule": diff}

    # 3. vault-link-check (existing tool) on a sample of plain wiki names
    loader = importlib.machinery.SourceFileLoader("vlc", str(Path.home() / "bin/vault-link-check"))
    spec = importlib.util.spec_from_loader("vlc", loader); vlc = importlib.util.module_from_spec(spec); loader.exec_module(vlc)
    plain = [x for x in links if x[1] == "Wiki" and "/" not in x[2] and x[2]]
    random.Random(4).shuffle(plain)
    sample = plain[:150]
    # Every disagreement must be explained by a known limit of that tool (first run: 24 notes behind the symlinked folder —
    # Path.rglob does not follow symlinks, Obsidian does; 18 links it counted as resolved because a FOLDER of that name exists).
    symlinked = [str(p.relative_to(VAULT)) + "/" for p in VAULT.iterdir() if p.is_symlink() and p.is_dir()]
    cmp = {"both_resolve": 0, "both_unresolved": 0, "ours_only": 0, "theirs_only": 0}
    why = {}
    for src, kind, tgt, got, amb in sample:
        theirs = vlc.resolve_wiki_link(tgt, VAULT)
        o, t = got != "UNRESOLVED", theirs is not None
        key = "both_resolve" if o and t else "both_unresolved" if not o and not t else "ours_only" if o else "theirs_only"
        cmp[key] += 1
        if key == "ours_only":
            w = ("note behind a symlinked folder (rglob does not follow)" if any(got.startswith(s) for s in symlinked) else
                 "attachment (it checks .md only)" if not got.endswith(".md") else
                 "name differs in case (it matches case-sensitively)" if got.rsplit("/", 1)[-1][:-3] != tgt else "UNEXPLAINED")
        elif key == "theirs_only":
            rel = str(theirs.relative_to(VAULT))
            w = ("it matched a FOLDER, not a note" if theirs.is_dir() else
                 "note in a hidden folder Obsidian ignores" if any(p.startswith(".") for p in rel.split("/")) else "UNEXPLAINED")
        else:
            continue
        why[w] = why.get(w, 0) + 1
    cmp["disagreements_by_cause"] = why
    res["vs_vault_link_check_sample"] = cmp
    ok_vault = res["vault_note_count_match"] and res["vault_vs_python_reference"]["rate"] >= 0.999
    res["pass"] = bool(ok_fx and ok_vault and "UNEXPLAINED" not in why)
    (HERE / "results/verify_vault.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
