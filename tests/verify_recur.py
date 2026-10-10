#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 C1 verify: residue-clock recurrence (projects/omni-ui/src/recur.rs).
1. `omni-ui recur-check 10000`: residue answers vs direct enumeration on 10k random rule pairs (day / month rings, COUNT,
   UNTIL, EXDATE, time-of-day overlap) -> 0 mismatches required.
2. The pure-CRT subset (day ring, no bounds) re-solved by bin/reason-gen's calendar.crt solver and its independent day-walk
   verifier (a second back-end, Python) -> identical first meeting day or "never".
"""
import importlib.machinery
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parents[1]
AI = HERE.parents[1]


def main() -> int:
    out = subprocess.run([str(HERE / "target/release/omni-ui"), "recur-check", "10000"], cwd=HERE, capture_output=True, text=True, timeout=900)
    summ = json.loads(out.stdout)
    full = json.loads((HERE / "results/recur_check.json").read_text())
    loader = importlib.machinery.SourceFileLoader("reason_gen", str(AI / "bin/reason-gen"))
    spec = importlib.util.spec_from_loader("reason_gen", loader)
    rg = importlib.util.module_from_spec(spec)
    loader.exec_module(rg)
    T = lambda kind, text, value=None: value  # noqa: E731  (trace sink: keep the value)
    agree = disagree = verified = 0
    bad = []
    for p in full["crt_pairs"]:
        # reason-gen counts days from its own day 0; ours are offsets from 2026-01-01 — the meeting offset is the same number
        pr = {"p": p["p"], "a": p["a"], "q": p["q"], "b": p["b"]}
        ans = rg.solve_calendar(pr, T)
        theirs = None if ans == "never" else int(ans.split()[1])
        verified += rg.verify_calendar(pr, ans)
        if theirs == p["x"]:
            agree += 1
        else:
            disagree += 1
            if len(bad) < 5:
                bad.append({"pair": p, "reason_gen": ans})
    res = {"recur_check": {k: summ[k] for k in ("pairs", "mismatches", "with_conflict", "never_or_beyond", "by_kind",
                                                 "residue_us_per_pair", "reference_us_per_pair")},
           "reason_gen_crosscheck": {"pairs": agree + disagree, "agree": agree, "disagree": disagree, "reason_gen_self_verified": verified,
                                     "first_disagreements": bad}}
    res["pass"] = summ["mismatches"] == 0 and disagree == 0 and verified == agree + disagree and agree > 0
    (HERE / "results/verify_recur.json").write_text(json.dumps(res, indent=1))
    print(json.dumps(res, indent=1))
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
