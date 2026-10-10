// SPDX-License-Identifier: MIT OR Apache-2.0
//! Calendar — NEURAL-APPS-0 step 7 on OmniUI. The .ics file is the store; every event becomes residue-clock rules
//! (ics.rs → recur.rs), so "is it on this day" is one membership test and "when do these two clash" is one generalised CRT.
//! Intents: next / back (one week), speak (the week aloud), select.
use crate::app::{App, Log};
use crate::ics::{self, weekday, Event};
use crate::recur::{civil_from_days, days_from_civil, first_conflict};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::path::Path;

const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

pub fn iso(day: i64) -> String { let (y, m, d) = civil_from_days(day); format!("{y:04}-{m:02}-{d:02}") }
pub fn long_date(day: i64) -> String { let (_, m, d) = civil_from_days(day); format!("{} {} {}", DAYS[weekday(day) as usize], d, MONTHS[(m - 1) as usize]) }
pub fn hhmm(m: i64) -> String { format!("{:02}:{:02}", m / 60, m % 60) }
pub fn parse_iso(s: &str) -> Option<i64> { let v: Vec<i64> = s.split('-').filter_map(|x| x.parse().ok()).collect(); (v.len() == 3).then(|| days_from_civil(v[0], v[1], v[2])) }

/// Occurrences on `day`: (start minute, end minute, summary), by O(1) membership per rule.
pub fn day_items(events: &[Event], day: i64) -> Vec<(i64, i64, String)> {
    let mut v: Vec<(i64, i64, String)> = events.iter().flat_map(|e| e.rules.iter().filter(move |r| r.occurs_on(day)).map(move |r| (r.t0, r.t1, e.summary.clone()))).collect();
    v.sort();
    v
}

/// The first clash of every pair of events on or after `from`: (day, a, b), soonest first.
pub fn conflicts(events: &[Event], from: i64) -> Vec<(i64, String, String)> {
    let mut out = vec![];
    for i in 0..events.len() {
        for j in i + 1..events.len() {
            let first = events[i].rules.iter().flat_map(|a| events[j].rules.iter().filter_map(move |b| first_conflict(a, b, from))).min();
            if let Some(d) = first { out.push((d, events[i].summary.clone(), events[j].summary.clone())); }
        }
    }
    out.sort();
    out
}

pub struct CalApp {
    pub name: String,
    pub events: Vec<Event>,
    pub week: i64, // Monday of the shown week
    pub today: i64,
    pub status: String,
    focus_ord: usize,
    log: Log,
    quiet: bool,
}

impl CalApp {
    pub fn open(path: &Path, today: i64) -> CalApp {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let (events, status) = match std::fs::read_to_string(path) {
            Ok(t) => {
                let ev = ics::parse(&t);
                let warn: usize = ev.iter().map(|e| e.warnings.len()).sum();
                let s = format!("{} events{}", ev.len(), if warn > 0 { format!(" · {warn} rule parts not supported") } else { String::new() });
                (ev, s)
            }
            Err(e) => (vec![], format!("Could not open {name}: {e}")),
        };
        CalApp { name, events, week: today - weekday(today), today, status, focus_ord: 0, log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() }
    }

    fn week_text(&self) -> String {
        (0..7).map(|k| {
            let d = self.week + k;
            let items = day_items(&self.events, d);
            let body = if items.is_empty() { "nothing".to_string() } else { items.iter().map(|(a, _, s)| format!("{} {}", hhmm(*a), s)).collect::<Vec<_>>().join(", ") };
            format!("{}: {}", long_date(d), body)
        }).collect::<Vec<_>>().join(". ")
    }
}

impl App for CalApp {
    fn title(&self) -> String { format!("Calendar — {}", self.name) }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false } // next / back move a week

    fn css(&self) -> String {
        "body{margin:0;padding:20px 28px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:20px;line-height:1.35}\
.btn{display:inline-block;margin:0 10px 0 0;padding:6px 16px;border:2px solid #333333;background:#eeeeee}\
.btn.focus{border:3px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:17px;margin:10px 0 12px 0}\
h1{font-size:26px;margin:0 0 10px 0}\
ol{margin:0 0 14px 0;padding:0;list-style:none}\
li{margin:0 0 6px 0}\
.n{display:none}\
li p{margin:0 0 0 24px}\
.n + p{margin:0;font-weight:bold}".into()
    }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        for (label, verb) in [("Back", "back"), ("Next", "next"), ("Read aloud", "speak")] { t.add(Kind::Button, win, label, "", verb); }
        let ahead = conflicts(&self.events, self.week);
        t.add(Kind::Status, win, &format!("{} · {} clashes ahead", self.status, ahead.len()), "", "");
        t.add(Kind::Heading, win, &format!("Week of {}", long_date(self.week)), "", "");
        let list = t.add(Kind::List, win, "Days", "", "");
        for k in 0..7 {
            let d = self.week + k;
            let li = t.add(Kind::ListItem, list, "", "", "");
            t.add(Kind::Label, li, &format!("{}{}", long_date(d), if d == self.today { " (today)" } else { "" }), "", "");
            let items = day_items(&self.events, d);
            if items.is_empty() { t.add(Kind::Label, li, "nothing", "", ""); }
            for (a, b, s) in items { t.add(Kind::Label, li, &format!("{}–{} {}", hhmm(a), hhmm(b), s), "", ""); }
        }
        if !ahead.is_empty() {
            t.add(Kind::Heading, win, "Clashes ahead", "", "");
            let cl = t.add(Kind::List, win, "Clashes", "", "");
            for (d, a, b) in ahead.iter().take(6) {
                let li = t.add(Kind::ListItem, cl, "", "", "");
                t.add(Kind::Label, li, &format!("{}: {a} and {b} overlap", long_date(*d)), "", "");
            }
        }
        t
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match verb {
            "next" => self.week += 7,
            "back" => self.week -= 7,
            "speak" => {
                let text = self.week_text();
                if !self.quiet {
                    if let Ok(mut c) = std::process::Command::new("access").args(["speak", "-"]).stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(mut si) = c.stdin.take() { let _ = si.write_all(text.as_bytes()); }
                    }
                }
            }
            "select" => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            _ => { self.status = format!("“{verb}” is not something Calendar does."); }
        }
        let extra = json!({"week": iso(self.week)});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, _t: &Tree, key: &str, _text: Option<&str>) -> Option<Value> {
        match key { "PageDown" => Some(self.act("next", "", "key")), "PageUp" => Some(self.act("back", "", "key")), _ => None }
    }

    fn state(&self) -> Value { json!({"week": iso(self.week), "events": self.events.len(), "status": self.status}) }
}

/// `omni-ui agenda <ics> --from YYYY-MM-DD --days N`: occurrences + first clashes as JSON (tests/verify_calendar.py).
pub fn agenda_json(path: &Path, from: i64, days: i64) -> Value {
    let ev = ics::parse(&std::fs::read_to_string(path).unwrap_or_default());
    let t = std::time::Instant::now();
    let occ: Vec<Value> = (0..days).flat_map(|k| day_items(&ev, from + k).into_iter().map(move |(a, b, s)| json!([iso(from + k), hhmm(a), hhmm(b), s]))).collect();
    let agenda_us = t.elapsed().as_secs_f64() * 1e6;
    let t = std::time::Instant::now();
    let cl: Vec<Value> = conflicts(&ev, from).into_iter().map(|(d, a, b)| json!([iso(d), a, b])).collect();
    let conflict_us = t.elapsed().as_secs_f64() * 1e6;
    json!({"events": ev.iter().map(|e| json!({"summary": e.summary, "rules": e.rules.len(), "warnings": e.warnings})).collect::<Vec<_>>(),
           "occurrences": occ, "conflicts": cl, "agenda_us": agenda_us.round(), "conflicts_us": conflict_us.round()})
}
