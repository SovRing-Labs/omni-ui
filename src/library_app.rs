// SPDX-License-Identifier: MIT OR Apache-2.0
//! Executive Library — the launcher's Library / Executive Reader on the native engine (NEURAL-APPS-0 step L). CLOSE, not a new
//! design: shelves = PerchLibrary.svelte's categories (+ their pinned docs) and the mobile reader's scan sources; titles and
//! badges by note.js parseDocTitle; Essence = markdownRenderer.js extractEssence (md::essence, byte-identical); Essence is the
//! default view (as the mobile reader). READ-ONLY: the doc-state / decision backend was removed from the sidecar (c76af95,
//! 2026-09-26) and where it lives again is the author's call — this app shows state as unavailable instead of inventing a store.
//! Intents: open <path>, back, next (page; at the end, the next doc), read essence|full, speak, find <words>, select.
use crate::app::{App, Log};
use crate::md::{blocks, essence, part_starts, plain, Block};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Doc { pub path: PathBuf, pub shelf: usize, pub title: String, pub badge: String, pub mtime: u64, pub pinned: bool }

pub struct Shelf { pub label: &'static str, pub root: PathBuf, pub recursive: bool, pub pinned: Vec<&'static str>, pub badge: &'static str }

pub fn shelves() -> Vec<Shelf> {
    let h = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let ai = h.join("ai-stack");
    let v = h.join("Documents/Obsidian");
    vec![
        Shelf { label: "Governance", root: ai.join("control-center/governance"), recursive: false, badge: "GOVERNANCE",
                pinned: vec!["BOOTSTRAP.md", "COMMANDER-CODEX.md", "COMMANDER-GENR.md", "COMMANDER-CHARTER.md"] },
        Shelf { label: "Systems docs", root: ai.join("docs/systems"), recursive: true, badge: "SYSTEM",
                pinned: vec!["roadmap/MANUAL.md", "lane-catalog/MANUAL.md", "dispatcherd/MANUAL.md", "done-gate/MANUAL.md", "perch/MANUAL.md"] },
        Shelf { label: "Oracle reviews", root: ai.join("_Reviews"), recursive: true, badge: "REVIEW", pinned: vec![] },
        Shelf { label: "Vault reviews", root: v.join("_Reviews"), recursive: true, badge: "REVIEW", pinned: vec![] },
        Shelf { label: "Proposals", root: v.join("200-Business/Business-Proposals"), recursive: true, badge: "PROPOSAL", pinned: vec![] },
        Shelf { label: "Debriefs & logs", root: ai.join("control-center"), recursive: false, badge: "LOG",
                pinned: vec!["LOG.md", "PLAN.md", "FLEET-5-10-GEN-PLAN.md"] },
    ]
}

/// note.js parseDocTitle: strip a leading "YYYY-MM-DD — ", lift " Oracle <LENS> [(Decision)]" into the badge.
pub fn parse_title(stem: &str, fallback: &str) -> (String, String) {
    let mut t = stem.to_string();
    let b = t.as_bytes();
    if b.len() >= 10 && b[..10].iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() }) {
        let rest = t[10..].trim_start();
        let rest = rest.strip_prefix('—').or_else(|| rest.strip_prefix('-')).map(|r| r.trim_start()).unwrap_or(rest);
        t = rest.to_string();
    }
    let lower = t.to_lowercase();
    for lens in ["converge", "adversarial", "reliability", "reimagine"] {
        for tail in [format!(" oracle {lens} (decision)"), format!(" oracle {lens}")] {
            if lower.ends_with(&tail) {
                let badge = if tail.ends_with("(decision)") { "DECISION".to_string() } else { lens.to_uppercase() };
                return (t[..t.len() - tail.len()].trim().to_string(), badge);
            }
        }
    }
    (t, fallback.to_string())
}

fn walk(dir: &Path, recursive: bool, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') { continue; }
        if p.is_dir() { if recursive && depth < 6 { walk(&p, recursive, depth + 1, out); } }
        else if name.to_lowercase().ends_with(".md") { out.push(p); }
    }
}

pub fn corpus() -> Vec<Doc> {
    let mut docs = vec![];
    for (k, s) in shelves().iter().enumerate() {
        let mut files = vec![];
        walk(&s.root, s.recursive, 0, &mut files);
        let mut shelf: Vec<Doc> = files.into_iter().map(|p| {
            let rel = p.strip_prefix(&s.root).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            let stem = p.file_stem().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let (title, badge) = parse_title(if stem.eq_ignore_ascii_case("MANUAL") || stem.eq_ignore_ascii_case("DEVLOG") { &rel } else { &stem }, s.badge);
            let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
            Doc { pinned: s.pinned.iter().any(|x| *x == rel), path: p, shelf: k, title: title.trim_end_matches(".md").to_string(), badge, mtime }
        }).collect();
        shelf.sort_by(|a, b| (b.pinned, b.mtime).cmp(&(a.pinned, a.mtime))); // pinned first, then newest (the mobile "recent" sort, working)
        docs.extend(shelf);
    }
    docs
}

fn day(secs: u64) -> String { let (y, m, d) = crate::recur::civil_from_days((secs / 86_400) as i64); format!("{y:04}-{m:02}-{d:02}") }

pub struct LibraryApp {
    pub docs: Vec<Doc>,
    pub shelf: Option<usize>, // None = Recent across every shelf
    pub open: Option<usize>,
    pub full: bool,
    pub part: usize,
    pub filter: String,
    pub status: String,
    pub scroll: i32,
    page_h: i32,
    view_h: i32,
    focus_ord: usize,
    history: Vec<usize>,
    speaker: Option<std::process::Child>,
    log: Log,
    quiet: bool,
}

impl LibraryApp {
    pub fn new() -> LibraryApp {
        let docs = corpus();
        let status = format!("{} documents on {} shelves · read-only (doc state: its store was removed 2026-09-26; J decides where it lives)", docs.len(), shelves().len());
        LibraryApp { docs, shelf: None, open: None, full: false, part: 0, filter: String::new(), status, scroll: 0, page_h: 0, view_h: 0,
                     focus_ord: 0, history: vec![], speaker: None, log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() }
    }

    /// The list on screen: one shelf (or Recent across all), filtered by words in title / badge / path.
    pub fn listing(&self) -> Vec<usize> {
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        let mut v: Vec<usize> = (0..self.docs.len()).filter(|&i| self.shelf.is_none_or(|s| self.docs[i].shelf == s)).filter(|&i| {
            let d = &self.docs[i];
            let hay = format!("{} {} {}", d.title, d.badge, d.path.display()).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        }).collect();
        if self.shelf.is_none() { v.sort_by(|&a, &b| self.docs[b].mtime.cmp(&self.docs[a].mtime)); v.truncate(60); }
        v
    }

    pub fn text_of(&self, i: usize) -> String {
        let raw = String::from_utf8_lossy(&std::fs::read(&self.docs[i].path).unwrap_or_default()).to_string(); // read only; bad UTF-8 → U+FFFD (as the browser)
        if self.full { raw } else { essence(&raw) }
    }
    pub fn parts(&self) -> usize { self.open.map(|i| part_starts(&blocks(&self.text_of(i))).len()).unwrap_or(1) }
    fn clamp(&mut self) { self.scroll = self.scroll.clamp(0, (self.page_h - self.view_h).max(0)); }
    fn stop_speaking(&mut self) { if let Some(mut c) = self.speaker.take() { let _ = c.kill(); let _ = c.wait(); } }
}

impl App for LibraryApp {
    fn title(&self) -> String { match self.open { Some(i) => format!("Library — {}", self.docs[i].title), None => "Library — Executive".into() } }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false }
    fn scroll(&self) -> i32 { self.scroll }
    fn set_extent(&mut self, page_h: i32, view_h: i32) { self.page_h = page_h; self.view_h = view_h; self.clamp(); }
    fn wheel(&mut self, dy: i32) { self.scroll += dy; self.clamp(); }
    fn fit(&self) -> Option<(i32, i32)> { Some((20, 56)) }
    fn arg_for(&self, t: &Tree, w: u32, _verb: &str) -> String { t.value(w).to_string() } // links carry a path, buttons a mode / shelf

    fn css(&self) -> String {
        "body{margin:0;padding:20px 28px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:20px;line-height:1.45}\
.btn{display:inline-block;margin:0 8px 8px 0;padding:6px 14px;border:2px solid #333333;background:#eeeeee}\
.btn.focus{border:3px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:17px;margin:6px 0 12px 0}\
h1{font-size:30px;margin:6px 0 10px 0} h2{font-size:25px;margin:16px 0 8px 0} h3{font-size:22px;margin:14px 0 6px 0} h4,h5,h6{font-size:20px;margin:12px 0 6px 0}\
p{margin:0 0 10px 0} .para{margin:0 0 12px 0}\
ol{margin:0 0 10px 0;padding:0;list-style:none} li{margin:0 0 10px 0} .n{display:none}\
.link{display:inline;color:#0033aa;text-decoration:underline} .link.focus{background:#dde8ff;border:2px solid #0047b3}\
li p{margin:2px 0 0 0;color:#444444;font-size:16px} li p.para{margin:0;color:#111111;font-size:20px}\
.code{font-family:monospace;font-size:17px;background:#f2f2f2;padding:8px;margin:0 0 12px 0;white-space:pre-wrap}".into()
    }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        t.add(Kind::Button, win, "Back", "", "back");
        t.add(Kind::Button, win, "Read aloud", "", "speak");
        let Some(i) = self.open else {
            // shelves as buttons: "Recent" + each shelf (verb find with the shelf index rides in the value)
            t.add(Kind::Button, win, "Recent", "shelf:", "select");
            for (k, s) in shelves().iter().enumerate() { t.add(Kind::Button, win, s.label, &format!("shelf:{k}"), "select"); }
            t.add(Kind::Status, win, &self.status, "", "");
            let name = self.shelf.map(|s| shelves()[s].label).unwrap_or("Recent");
            t.add(Kind::Heading, win, &format!("{name}{}", if self.filter.is_empty() { String::new() } else { format!(" · “{}”", self.filter) }), "1", "");
            let list = t.add(Kind::List, win, name, "", "");
            for k in self.listing() {
                let d = &self.docs[k];
                let li = t.add(Kind::ListItem, list, "", "", "");
                t.add(Kind::Link, li, &d.title, &d.path.to_string_lossy(), "open");
                t.add(Kind::Label, li, &format!("{} · {} · {}", d.badge, day(d.mtime), shelves()[d.shelf].label), "", "");
            }
            return t;
        };
        let d = &self.docs[i];
        t.add(Kind::Button, win, if self.full { "Essence" } else { "Full text" }, if self.full { "essence" } else { "full" }, "read");
        t.add(Kind::Status, win, &format!("{} · {} · {} · {}", d.badge, day(d.mtime), shelves()[d.shelf].label, if self.full { "full text" } else { "essence" }), "", "");
        t.add(Kind::Heading, win, &d.title, "1", "");
        let all = blocks(&self.text_of(i));
        let starts = part_starts(&all);
        let part = self.part.min(starts.len() - 1);
        if starts.len() > 1 { t.add(Kind::Status, win, &format!("Long document: part {} of {}", part + 1, starts.len()), "", ""); }
        let (from, to) = (starts[part], starts.get(part + 1).copied().unwrap_or(all.len()));
        let mut lists: std::collections::HashMap<u32, u32> = Default::default();
        let txt = |s: &str| plain(&s.replace("[[", "").replace("]]", ""));
        for b in &all[from..to] {
            match b {
                Block::Heading(l, s) => { t.add(Kind::Heading, win, s, &l.to_string(), ""); }
                Block::Para(s) => { t.add(Kind::Paragraph, win, &txt(s), "", ""); }
                Block::Code(s) => { t.add(Kind::Code, win, s, "", ""); }
                Block::Item(id, s) => {
                    let l = *lists.entry(*id).or_insert_with(|| t.add(Kind::List, win, "", "", ""));
                    let li = t.add(Kind::ListItem, l, "", "", "");
                    t.add(Kind::Paragraph, li, &txt(s), "", "");
                }
            }
        }
        t
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match verb {
            "open" => if let Some(i) = self.docs.iter().position(|d| d.path.to_string_lossy() == arg.trim()) {
                self.stop_speaking();
                if let Some(c) = self.open { self.history.push(c); }
                self.open = Some(i); self.full = false; self.part = 0; self.scroll = 0; self.focus_ord = 0;
            } else { self.status = format!("No document “{}”", arg.trim()); },
            "back" => { self.stop_speaking(); self.open = self.history.pop(); self.part = 0; self.scroll = 0; self.focus_ord = 0; }
            "read" => { self.full = arg.trim() == "full"; self.part = 0; self.scroll = 0; }
            "select" if arg.starts_with("shelf:") => { self.shelf = arg[6..].parse().ok(); self.open = None; self.history.clear(); self.scroll = 0; }
            "select" => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            "find" => { self.filter = arg.trim().to_string(); self.open = None; self.scroll = 0; }
            "next" => {
                if self.scroll + self.view_h < self.page_h { self.scroll += (self.view_h - 60).max(60); self.clamp(); }
                else if self.open.is_some() && self.part + 1 < self.parts() { self.part += 1; self.scroll = 0; }
                else if let Some(i) = self.open {
                    // the end of a document: the next one in the list on screen
                    let saved = self.open.take();
                    let list = self.listing();
                    self.open = saved;
                    if let Some(p) = list.iter().position(|&k| k == i) { if let Some(&n) = list.get(p + 1) { self.history.push(i); self.open = Some(n); self.part = 0; self.scroll = 0; } }
                }
            }
            "speak" => {
                // one reading at a time: a new request stops the previous one (the mobile reader's audio overlap, fixed)
                self.stop_speaking();
                let text = match self.open { Some(i) => self.text_of(i), None => self.listing().iter().take(10).map(|&k| self.docs[k].title.clone()).collect::<Vec<_>>().join(". ") };
                if !self.quiet {
                    if let Ok(mut c) = std::process::Command::new("access").args(["speak", "-"]).stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(mut si) = c.stdin.take() { let _ = si.write_all(plain(&text).as_bytes()); }
                        self.speaker = Some(c);
                    }
                }
            }
            _ => { self.status = format!("“{verb}” is not something the Library does."); }
        }
        let extra = json!({"open": self.open.map(|i| self.docs[i].path.to_string_lossy().to_string()), "full": self.full, "shelf": self.shelf, "part": self.part});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, _t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        match (key, text) { ("PageDown", _) | (_, Some(" ")) => Some(self.act("next", "", "key")), ("Escape", _) => Some(self.act("back", "", "key")), _ => None }
    }

    fn state(&self) -> Value {
        json!({"open": self.open.map(|i| self.docs[i].path.to_string_lossy().to_string()), "full": self.full, "shelf": self.shelf, "docs": self.docs.len(),
               "listing": self.listing().len(), "depth": self.history.len()})
    }
}
