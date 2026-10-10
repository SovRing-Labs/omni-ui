// SPDX-License-Identifier: MIT OR Apache-2.0
//! Notes — NEURAL-APPS-0 step 8 on OmniUI: the Obsidian vault, READ-FORWARD (author: "Obsidian stays the brain"; "never silently
//! corrupt a real note" — this app opens no file for writing). Markdown → widgets: headings by level, paragraphs with links
//! inline, list items, code, quotes; links resolve through vault.rs; "Linked from" = backlinks; history back.
//! Intents: open <note path>, back (history), find <words> (titles), speak (the note), select.
use crate::app::{App, Log};
use crate::vault::{Kind as LKind, Res, Vault};
use crate::md::{blocks, part_starts, plain, Block};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::path::Path;

pub struct NotesApp {
    pub vault: Vault,
    pub current: Option<u32>,
    pub part: usize,
    pub history: Vec<u32>,
    pub filter: String,
    pub status: String,
    pub scroll: i32,
    page_h: i32,
    view_h: i32,
    focus_ord: usize,
    log: Log,
    quiet: bool,
}

/// One markdown line's inline content: text runs and links, in order. `inline code` is text, never a link (as the resolver).
fn inline(t: &mut Tree, parent: u32, text: &str, v: &Vault, note: u32) {
    for (k, seg) in text.split('`').enumerate() {
        if k % 2 == 1 { if !seg.trim().is_empty() { t.add(Kind::Span, parent, seg, "", ""); } } else { inline_links(t, parent, seg, v, note); }
    }
}

fn inline_links(t: &mut Tree, parent: u32, text: &str, v: &Vault, note: u32) {
    let mut rest = text;
    let mut buf = String::new();
    // a text run keeps one space at either edge: "[[a]], [[b]]" must not draw as "a,b"
    let flush = |t: &mut Tree, buf: &mut String| {
        if !buf.trim().is_empty() {
            let (l, r) = (if buf.starts_with(char::is_whitespace) { " " } else { "" }, if buf.ends_with(char::is_whitespace) { " " } else { "" });
            t.add(Kind::Span, parent, &format!("{l}{}{r}", plain(buf)), "", "");
        }
        buf.clear();
    };
    while !rest.is_empty() {
        let wl = rest.find("[[");
        let ml = rest.find("](").and_then(|j| rest[..j].rfind('[').map(|o| (o, j)));
        let next = match (wl, ml) { (Some(a), Some((o, _))) => if a <= o { Some((a, true)) } else { Some((o, false)) }, (Some(a), None) => Some((a, true)), (None, Some((o, _))) => Some((o, false)), _ => None };
        let Some((at, wiki)) = next else { buf.push_str(rest); break };
        if wiki {
            let Some(end) = rest[at + 2..].find("]]") else { buf.push_str(rest); break };
            let inner = rest[at + 2..at + 2 + end].replace("\\|", "|");
            let embed = at > 0 && rest.as_bytes()[at - 1] == b'!';
            buf.push_str(&rest[..if embed { at - 1 } else { at }]);
            flush(t, &mut buf);
            let (tp, shown) = inner.split_once('|').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((inner.clone(), inner.replace('#', " › ").trim_start_matches(" › ").to_string()));
            let target = tp.split('#').next().unwrap_or("").trim().to_string();
            link_widget(t, parent, v, note, &target, &shown, embed, false);
            rest = &rest[at + 2 + end + 2..];
        } else {
            let (o, j) = ml.unwrap();
            let Some(close) = rest[j + 2..].find(')') else { buf.push_str(rest); break };
            let shown = rest[o + 1..j].to_string();
            let url = rest[j + 2..j + 2 + close].trim().trim_matches(|c| c == '<' || c == '>').to_string();
            buf.push_str(&rest[..o]);
            if url.contains("://") || url.starts_with("mailto:") {
                buf.push_str(&shown); // external links: text only (this app does not open the web)
            } else {
                flush(t, &mut buf);
                let dec = url.replace("%20", " ");
                link_widget(t, parent, v, note, dec.split('#').next().unwrap_or(""), &shown, false, true);
            }
            rest = &rest[j + 2 + close + 1..];
        }
    }
    flush(t, &mut buf);
}

fn link_widget(t: &mut Tree, parent: u32, v: &Vault, note: u32, target: &str, shown: &str, embed: bool, md: bool) {
    let found = v.notes[note as usize].links.iter().find(|l| l.target.trim() == target.trim() && (l.kind == LKind::Md) == md);
    match found.map(|l| l.res) {
        Some(Res::Note(n)) => { t.add(Kind::Link, parent, &plain(shown), &v.notes[n as usize].rel, "open"); }
        Some(Res::File(f)) => { t.add(Kind::Span, parent, &format!("[{}: {}]", if embed { "embedded file" } else { "file" }, v.files[f as usize].rsplit('/').next().unwrap_or("")), "", ""); }
        Some(Res::SameNote) => { t.add(Kind::Span, parent, &plain(shown), "", ""); }
        _ => { t.add(Kind::Span, parent, &format!("{} (not created yet)", plain(shown)), "", ""); }
    }
}


impl NotesApp {
    pub fn open(root: &Path) -> NotesApp {
        let vault = Vault::open(root);
        let st = vault.stats();
        let status = format!("{} notes · {} links ({} not created yet) · indexed in {} ms · read-only", st["notes"], st["links"], st["unresolved"], st["index_ms"]);
        NotesApp { vault, current: None, part: 0, history: vec![], filter: String::new(), status, scroll: 0, page_h: 0, view_h: 0, focus_ord: 0,
                   log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() }
    }

    fn title_of(&self, n: u32) -> String { let r = &self.vault.notes[n as usize].rel; r.rsplit('/').next().unwrap_or(r).trim_end_matches(".md").to_string() }

    /// Home list: notes whose title holds every filter word, else the most linked-to notes.
    pub fn home_list(&self) -> Vec<u32> {
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        let mut v: Vec<u32> = (0..self.vault.notes.len() as u32).filter(|&n| { let t = self.vault.notes[n as usize].rel.to_lowercase(); words.iter().all(|w| t.contains(w.as_str())) }).collect();
        v.sort_by_key(|&n| (std::cmp::Reverse(self.vault.backlinks[n as usize].len()), self.vault.notes[n as usize].rel.clone()));
        v.truncate(40);
        v
    }

    fn go(&mut self, n: u32) { if let Some(c) = self.current { if c != n { self.history.push(c); } } self.current = Some(n); self.part = 0; self.scroll = 0; self.focus_ord = 0; }
    pub fn parts(&self) -> usize { self.current.map(|n| part_starts(&blocks(&std::fs::read_to_string(self.vault.root.join(&self.vault.notes[n as usize].rel)).unwrap_or_default())).len()).unwrap_or(1) }
    fn clamp(&mut self) { self.scroll = self.scroll.clamp(0, (self.page_h - self.view_h).max(0)); }
}

impl App for NotesApp {
    fn title(&self) -> String { match self.current { Some(n) => format!("Notes — {}", self.title_of(n)), None => "Notes — vault".into() } }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false }
    fn scroll(&self) -> i32 { self.scroll }
    fn set_extent(&mut self, page_h: i32, view_h: i32) { self.page_h = page_h; self.view_h = view_h; self.clamp(); }
    fn wheel(&mut self, dy: i32) { self.scroll += dy; self.clamp(); }
    fn fit(&self) -> Option<(i32, i32)> { Some((20, 56)) }

    fn css(&self) -> String {
        "body{margin:0;padding:20px 28px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:20px;line-height:1.45}\
.btn{display:inline-block;margin:0 10px 0 0;padding:6px 16px;border:2px solid #333333;background:#eeeeee}\
.btn.focus{border:3px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:17px;margin:10px 0 12px 0}\
h1{font-size:30px;margin:6px 0 10px 0} h2{font-size:25px;margin:16px 0 8px 0} h3{font-size:22px;margin:14px 0 6px 0}\
h4,h5,h6{font-size:20px;margin:12px 0 6px 0}\
p{margin:0 0 10px 0} .para{margin:0 0 12px 0}\
ol{margin:0 0 10px 0;padding:0;list-style:none} li{margin:0 0 6px 0}\
.n{display:none}\
.link{display:inline;color:#0033aa;text-decoration:underline}\
.link.focus{background:#dde8ff;border:2px solid #0047b3}\
.code{font-family:monospace;font-size:17px;background:#f2f2f2;padding:8px;margin:0 0 12px 0;white-space:pre-wrap}".into()
    }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        for (label, verb) in [("Back", "back"), ("Read aloud", "speak")] { t.add(Kind::Button, win, label, "", verb); }
        let Some(n) = self.current else {
            t.add(Kind::Status, win, &self.status, "", "");
            t.add(Kind::Heading, win, if self.filter.is_empty() { "Most linked notes" } else { "Notes matching" }, "1", "");
            let list = t.add(Kind::List, win, "Notes", "", "");
            for k in self.home_list() {
                let li = t.add(Kind::ListItem, list, "", "", "");
                t.add(Kind::Link, li, &self.title_of(k), &self.vault.notes[k as usize].rel, "open");
                t.add(Kind::Label, li, &format!("{} · linked from {}", self.vault.notes[k as usize].rel, self.vault.backlinks[k as usize].len()), "", "");
            }
            return t;
        };
        let rel = self.vault.notes[n as usize].rel.clone();
        t.add(Kind::Status, win, &rel, "", "");
        let text = std::fs::read_to_string(self.vault.root.join(&rel)).unwrap_or_default(); // read only
        let all = blocks(&text);
        let starts = part_starts(&all);
        let parts = starts.len();
        let part = self.part.min(parts - 1);
        let (from, to) = (starts[part], starts.get(part + 1).copied().unwrap_or(all.len()));
        if parts > 1 { t.add(Kind::Status, win, &format!("Long note: part {} of {} (Next moves on at the end)", part + 1, parts), "", ""); }
        let mut lists: std::collections::HashMap<u32, u32> = Default::default();
        for b in &all[from..to] {
            match b {
                Block::Heading(l, s) => { t.add(Kind::Heading, win, s, &l.to_string(), ""); }
                Block::Para(s) => { let p = t.add(Kind::Paragraph, win, "", "", ""); inline(&mut t, p, s, &self.vault, n); }
                Block::Code(s) => { t.add(Kind::Code, win, s, "", ""); }
                Block::Item(id, s) => {
                    let l = *lists.entry(*id).or_insert_with(|| t.add(Kind::List, win, "", "", ""));
                    let li = t.add(Kind::ListItem, l, "", "", "");
                    inline(&mut t, li, s, &self.vault, n);
                }
            }
        }
        let back = &self.vault.backlinks[n as usize];
        if !back.is_empty() {
            t.add(Kind::Heading, win, &format!("Linked from ({})", back.len()), "2", "");
            let l = t.add(Kind::List, win, "Linked from", "", "");
            for &b in back.iter().take(50) { let li = t.add(Kind::ListItem, l, "", "", ""); t.add(Kind::Link, li, &self.title_of(b), &self.vault.notes[b as usize].rel, "open"); }
        }
        t
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match verb {
            "open" => match self.vault.find(arg.trim()) { Some(n) => self.go(n), None => self.status = format!("No note “{}”", arg.trim()) },
            "back" => { if let Some(p) = self.history.pop() { self.current = Some(p); } else { self.current = None; } self.part = 0; self.scroll = 0; self.focus_ord = 0; }
            "find" => { self.filter = arg.trim().to_string(); self.current = None; self.scroll = 0; }
            "next" => {
                if self.scroll + self.view_h >= self.page_h && self.part + 1 < self.parts() { self.part += 1; self.scroll = 0; }
                else { self.scroll += (self.view_h - 60).max(60); self.clamp(); }
            }
            "speak" => {
                let t = self.tree();
                let text: Vec<String> = (0..t.len() as u32).filter(|&w| matches!(t.kind[w as usize], Kind::Span | Kind::Link | Kind::Paragraph | Kind::Code)).map(|w| t.label(w).to_string()).filter(|s| !s.is_empty()).collect();
                if !self.quiet {
                    if let Ok(mut c) = std::process::Command::new("access").args(["speak", "-"]).stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(mut si) = c.stdin.take() { let _ = si.write_all(text.join(" ").as_bytes()); }
                    }
                }
            }
            "select" => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            _ => { self.status = format!("“{verb}” is not something Notes does."); }
        }
        let extra = json!({"current": self.current.map(|n| self.vault.notes[n as usize].rel.clone()), "depth": self.history.len()});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, _t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        match (key, text) { ("PageDown", _) | (_, Some(" ")) => Some(self.act("next", "", "key")), ("Escape", _) | ("Backspace", _) => Some(self.act("back", "", "key")), _ => None }
    }

    fn state(&self) -> Value {
        json!({"current": self.current.map(|n| self.vault.notes[n as usize].rel.clone()), "depth": self.history.len(), "home": self.home_list().len()})
    }
}
