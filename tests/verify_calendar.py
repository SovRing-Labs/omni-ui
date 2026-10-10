#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""NEURAL-APPS-0 C2 verify: Calendar on OmniUI (ics.rs -> recur.rs residue clocks).
Oracle: python-dateutil rrulestr / rruleset (the field's RRULE implementation, written independently of ours).
1. A 12-event .ics (weekly, multi-weekday, fortnightly, monthly on the 31st, yearly on 29 Feb, daily with COUNT, every 3 days
   with UNTIL, a one-off, monthly with COUNT, 2nd Tuesday, last Friday with COUNT, and one unsupported rule: BYSETPOS)
   -> `omni-ui agenda` over 8 weeks = dateutil expansion (date, start, end, title) exactly; the unsupported rule is reported
   and gets no occurrences (never guessed).
2. First clash of every pair (times overlap) = dateutil brute force over 12 years.
3. Window: AT-SPI tree; "next" (one week) from key (PageDown), text, voice (espeak-ng -> whisper.cpp), switch (row 3, col 1)
   and AT-SPI click -> one action, the week advances 7 days each time.
Side effects, undone: org.a11y.Status IsEnabled true during part 3, then restored; one window.
"""
import json
import os
import subprocess
import sys
import tempfile
from datetime import date, datetime, timedelta
from pathlib import Path

from dateutil.rrule import rrulestr

HERE = Path(__file__).resolve().parents[1]
PACK = HERE.parent / "access-pack"
BIN = HERE / "target/release/omni-ui"
sys.path.insert(0, str(PACK)); sys.path.insert(0, str(HERE / "tests"))
from verify_s3 import a11y_enabled, new_record, set_a11y, send, wait, walk  # noqa: E402
from verify_reader import find_window  # noqa: E402

FROM = date(2026, 1, 5)  # a Monday
# (summary, dtstart, end: ("DTEND", v) | ("DURATION", v) | None for all-day, rrule or None, exdates)
EVENTS = [
    ("Physio", "20260106T100000", ("DTEND", "20260106T110000"), "FREQ=WEEKLY;BYDAY=TU", ["20260120T100000"]),
    ("Team call", "20260106T103000", ("DURATION", "PT45M"), "FREQ=WEEKLY;BYDAY=TU,TH", []),
    ("Bin collection", "20260108T070000", ("DURATION", "PT15M"), "FREQ=WEEKLY;INTERVAL=2", []),
    ("Rent", "20260131T090000", ("DURATION", "PT30M"), "FREQ=MONTHLY;BYMONTHDAY=31", []),
    ("Birthday", "20240229", None, "FREQ=YEARLY", []),
    ("Meds", "20260105T080000", ("DURATION", "PT10M"), "FREQ=DAILY;COUNT=30", []),
    ("Gym", "20260105T180000", ("DTEND", "20260105T190000"), "FREQ=DAILY;INTERVAL=3;UNTIL=20260315T235900", []),
    ("Dentist", "20260113T103000", ("DTEND", "20260113T113000"), None, []),
    ("Book club", "20260115T190000", ("DURATION", "PT2H"), "FREQ=MONTHLY;COUNT=6", []),
    ("Council meeting", "20260113T190000", ("DURATION", "PT1H"), "FREQ=MONTHLY;BYDAY=2TU", []),          # 2nd Tuesday
    ("Support group", "20260130T180000", ("DURATION", "PT90M"), "FREQ=MONTHLY;BYDAY=-1FR;COUNT=8", []),  # last Friday
    ("Drinks", "20260130T190000", ("DURATION", "PT2H"), "FREQ=MONTHLY;BYDAY=FR;BYSETPOS=-1", []),     # unsupported on purpose
]
UNSUPPORTED = {"Drinks"}


def ics_text() -> str:
    out = ["BEGIN:VCALENDAR", "VERSION:2.0", "PRODID:-//omni-ui verify//EN"]
    for i, (s, ds, end, rr, ex) in enumerate(EVENTS):
        out += ["BEGIN:VEVENT", f"UID:verify-{i}@omni-ui", f"SUMMARY:{s}",
                f"DTSTART;VALUE=DATE:{ds}" if len(ds) == 8 else f"DTSTART:{ds}"]
        if end:
            out.append(f"{end[0]}:{end[1]}")
        if rr:
            out.append(f"RRULE:{rr}")
        for e in ex:
            out.append(f"EXDATE:{e}")
        out.append("END:VEVENT")
    out.append("END:VCALENDAR")
    return "\r\n".join(out) + "\r\n"


def dt(s):
    return datetime.strptime(s, "%Y%m%dT%H%M%S") if "T" in s else datetime.strptime(s, "%Y%m%d")


def span(ds, end):
    st = dt(ds)
    if end is None:
        return 0, 1440
    t0 = st.hour * 60 + st.minute
    if end[0] == "DTEND":
        e = dt(end[1]); t1 = int((e - st).total_seconds() // 60) + t0
    else:
        v = end[1]; h = int(v.split("T")[1].split("H")[0]) if "H" in v else 0; m = int(v.split("H")[-1].rstrip("M").split("T")[-1]) if v.endswith("M") else 0
        t1 = t0 + h * 60 + m
    return t0, min(t1, 1440)


def expand(ev, until: datetime):
    s, ds, end, rr, ex = ev
    st = dt(ds)
    if not rr:
        return [st.date()]
    lines = [f"DTSTART:{st.strftime('%Y%m%dT%H%M%S')}", f"RRULE:{rr}"] + [f"EXDATE:{dt(e).strftime('%Y%m%dT%H%M%S')}" for e in ex]
    rs = rrulestr("\n".join(lines), forceset=True)
    return [x.date() for x in rs.between(st - timedelta(seconds=1), until, inc=True)]


def hhmm(m):
    return f"{m // 60:02d}:{m % 60:02d}"


def main() -> int:
    tmp = Path(tempfile.mkdtemp(prefix="omni-cal-"))
    ics = tmp / "home.ics"
    ics.write_text(ics_text())
    env = dict(os.environ, OMNI_UI_QUIET="1", OMNI_UI_TEST="1")
    res = {}
    ag = json.loads(subprocess.run([str(BIN), "agenda", str(ics), "--from", FROM.isoformat(), "--days", "56"], env=env,
                                   capture_output=True, text=True, check=True).stdout)
    res["events"] = ag["events"]
    warned = {e["summary"] for e in ag["events"] if e["warnings"]}
    ok_warn = warned == UNSUPPORTED and all(e["rules"] == 0 for e in ag["events"] if e["summary"] in UNSUPPORTED)

    # 1. occurrences over 8 weeks vs dateutil
    end8 = datetime.combine(FROM + timedelta(days=55), datetime.max.time())
    want = []
    for ev in EVENTS:
        if ev[0] in UNSUPPORTED:
            continue
        t0, t1 = span(ev[1], ev[2])
        for d in expand(ev, end8):
            if FROM <= d <= end8.date():
                want.append([d.isoformat(), hhmm(t0), hhmm(t1), ev[0]])
    got = [list(x) for x in ag["occurrences"]]
    key = lambda x: (x[0], x[1], x[3])  # noqa: E731
    res["occurrences"] = {"ours": len(got), "dateutil": len(want), "equal": sorted(got, key=key) == sorted(want, key=key),
                          "only_ours": [x for x in got if x not in want][:5], "only_dateutil": [x for x in want if x not in got][:5]}
    res["agenda_us"], res["conflicts_us"] = ag["agenda_us"], ag["conflicts_us"]

    # 2. first clash per pair vs dateutil brute force (12 years)
    end12 = datetime.combine(FROM + timedelta(days=12 * 365), datetime.max.time())
    sup = [ev for ev in EVENTS if ev[0] not in UNSUPPORTED]
    occ = {ev[0]: set(d for d in expand(ev, end12) if d >= FROM) for ev in sup}
    want_c = []
    for i in range(len(sup)):
        for j in range(i + 1, len(sup)):
            a, b = sup[i], sup[j]
            (a0, a1), (b0, b1) = span(a[1], a[2]), span(b[1], b[2])
            if a0 >= b1 or b0 >= a1:
                continue
            common = sorted(occ[a[0]] & occ[b[0]])
            if common:
                want_c.append([common[0].isoformat(), a[0], b[0]])
    got_c = [c for c in ag["conflicts"] if c[0] <= end12.date().isoformat()]
    norm = lambda L: sorted([c[0], *sorted(c[1:])] for c in L)  # noqa: E731
    res["conflicts"] = {"ours": got_c, "dateutil": want_c, "equal": norm(got_c) == norm(want_c)}

    # 3. window: AT-SPI + "next" from every input
    from access import bus, speech
    was = a11y_enabled()
    sock, log = tmp / "ui.sock", tmp / "actions.jsonl"
    ui = None
    try:
        set_a11y(True)
        ui = subprocess.Popen([str(BIN), "run", "calendar", str(ics), "--from", FROM.isoformat()],
                              env=dict(env, OMNI_UI_SOCK=str(sock), OMNI_UI_LOG=str(log)), stderr=open(tmp / "ui.err", "w"))
        if not wait(lambda: sock.exists(), 30):
            raise SystemExit("calendar window did not start: " + (tmp / "ui.err").read_text())
        win = wait(lambda: find_window("Calendar — home.ics"), 20, 0.3)
        if not win:
            raise SystemExit("AT-SPI: no Calendar window")
        nodes = walk(win)
        res["atspi_roles"] = sorted({r for _, r, _, _ in nodes})
        labels = [n for _, r, n, _ in nodes if r == "label"]
        res["atspi_week_heading"] = [n for _, r, n, _ in nodes if r == "heading"][:2]
        res["atspi_has_physio_tuesday"] = any(l.startswith("10:00–11:00 Physio") for l in labels)
        acts = {"key": send(sock, {"key": "PageDown"}), "text": send(sock, json.loads(bus.intent("text", "next").to_json()))}
        pcm, rate = speech.synth("Next.", rate_wpm=150)
        it = bus.intent("voice", speech.resample(pcm, rate))
        res["voice_transcript"] = it.meta.get("transcript")
        acts["voice"] = send(sock, json.loads(it.to_json()))
        acts["switch"] = send(sock, json.loads(bus.intent("switch", {"presses": [2.7, 3.8]}).to_json()))   # row 3, col 1 = next
        btn = next(n for _, r, nm, n in walk(win) if r == "push button" and nm == "Next")
        ai = btn.get_action_iface()
        n0 = sum(1 for _ in open(log))
        ai.do_action([ai.get_action_name(k) for k in range(ai.get_n_actions())].index("click"))
        acts["at-spi"] = {"action": new_record(log, n0)}
        canon = {k: (v["action"]["action"] if v.get("action") else None) for k, v in acts.items()}
        weeks = [v["action"]["week"] if v.get("action") else None for v in acts.values()]
        res["canonical"], res["weeks_after"] = canon, weeks
        sources = {v["action"].get("source") for v in acts.values() if v.get("action")}
        steps = [(date.fromisoformat(b) - date.fromisoformat(a)).days for a, b in zip([FROM.isoformat()] + weeks[:-1], weeks)]
        ok_intents = all(c == ["next", ""] for c in canon.values()) and sources == {"key", "text", "voice", "switch", "at-spi"} and steps == [7] * 5
        ok_at = {"frame", "heading", "label", "push button", "list", "list item"} <= set(res["atspi_roles"]) and res["atspi_has_physio_tuesday"]
    finally:
        if ui:
            ui.terminate()
        set_a11y(was)
        res["a11y_restored"] = a11y_enabled() == was
    res["pass"] = bool(ok_warn and res["occurrences"]["equal"] and res["conflicts"]["equal"] and ok_intents and ok_at)
    (HERE / "results/verify_calendar.json").write_text(json.dumps(res, indent=1))
    print(json.dumps({k: res[k] for k in ("occurrences", "conflicts", "agenda_us", "conflicts_us", "atspi_roles", "atspi_week_heading",
                                          "voice_transcript", "canonical", "weeks_after", "a11y_restored", "pass")}, indent=1))
    print("warnings:", {e["summary"]: e["warnings"] for e in ag["events"] if e["warnings"]})
    return 0 if res["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
