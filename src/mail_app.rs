// SPDX-License-Identifier: MIT OR Apache-2.0
//! Mail — NEURAL-APPS-0 step 7b on OmniUI. `access mail <mbox|Maildir> --json` (access-pack: Python's standard mailbox / email
//! parsers; threading by headers, quote-sift for messages without them) → a thread list → one thread's messages.
//! Intents: open <n> (a thread), back (to the list), next (next thread / next page), speak, find <words> (filter the list).
use crate::app::{App, Log};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Thread { pub subject: String, pub count: usize, pub last_date: String, pub from: Vec<String>, pub msgs: Vec<(String, String, String, String)> }

pub struct MailApp {
    pub name: String,
    pub threads: Vec<Thread>,
    pub placed: Value,
    pub messages: usize,
    pub open: Option<usize>,
    pub filter: String,
    pub status: String,
    pub scroll: i32,
    page_h: i32,
    view_h: i32,
    focus_ord: usize,
    log: Log,
    quiet: bool,
}

pub fn access_mail(path: &Path) -> Result<Value, String> {
    let repo_bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../access-pack/bin/access");
    let bin = std::env::var("ACCESS_BIN").map(PathBuf::from).unwrap_or_else(|_| {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("access")).find(|c| c.is_file())).unwrap_or(repo_bin)
    });
    let out = std::process::Command::new(&bin).args(["mail", &path.to_string_lossy(), "--json"]).output().map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("mail failed").to_string()); }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("access mail output: {e}"))
}

impl MailApp {
    pub fn open(path: &Path) -> MailApp {
        let name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let mut app = MailApp { name, threads: vec![], placed: json!({}), messages: 0, open: None, filter: String::new(), status: String::new(),
                                scroll: 0, page_h: 0, view_h: 0, focus_ord: 0, log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() };
        match access_mail(path) {
            Ok(v) => {
                app.messages = v["messages"].as_u64().unwrap_or(0) as usize;
                app.placed = v["placed"].clone();
                app.threads = v["threads"].as_array().map(|a| a.iter().map(|t| Thread {
                    subject: t["subject"].as_str().unwrap_or("").to_string(), count: t["count"].as_u64().unwrap_or(0) as usize,
                    last_date: t["last_date"].as_str().unwrap_or("").to_string(),
                    from: t["from"].as_array().map(|f| f.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                    msgs: t["messages"].as_array().map(|m| m.iter().map(|x| (x["from"].as_str().unwrap_or("").to_string(), x["date"].as_str().unwrap_or("").to_string(),
                        x["text"].as_str().unwrap_or("").to_string(), x["how"].as_str().unwrap_or("").to_string())).collect()).unwrap_or_default(),
                }).collect()).unwrap_or_default();
                app.status = format!("{} messages in {} conversations", app.messages, app.threads.len());
            }
            Err(e) => app.status = format!("Could not open {}: {e}", app.name),
        }
        app
    }

    /// Thread indices shown in the list (all, or those whose subject / senders hold every filter word).
    pub fn visible(&self) -> Vec<usize> {
        let words: Vec<String> = self.filter.to_lowercase().split_whitespace().map(String::from).collect();
        (0..self.threads.len()).filter(|&i| {
            let t = &self.threads[i];
            let hay = format!("{} {}", t.subject, t.from.join(" ")).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        }).collect()
    }
    fn page(&self) -> i32 { (self.view_h - 60).max(60) }
    fn clamp(&mut self) { self.scroll = self.scroll.clamp(0, (self.page_h - self.view_h).max(0)); }
}

impl App for MailApp {
    fn title(&self) -> String { format!("Mail — {}", self.name) }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false }
    fn scroll(&self) -> i32 { self.scroll }
    fn set_extent(&mut self, page_h: i32, view_h: i32) { self.page_h = page_h; self.view_h = view_h; self.clamp(); }
    fn wheel(&mut self, dy: i32) { self.scroll += dy; self.clamp(); }
    fn fit(&self) -> Option<(i32, i32)> { Some((20, 56)) } // mail is full of long URLs

    fn css(&self) -> String {
        "body{margin:0;padding:20px 28px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:20px;line-height:1.4}\
.btn{display:inline-block;margin:0 10px 0 0;padding:6px 16px;border:2px solid #333333;background:#eeeeee}\
.btn.focus{border:3px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:17px;margin:10px 0 12px 0}\
h1{font-size:26px;line-height:1.25;margin:0 0 12px 0}\
ol{margin:0;padding:0;list-style:none}\
li{margin:0 0 12px 0}\
.n{display:inline-block;width:40px;color:#333333}\
.link{display:inline;color:#0033aa;text-decoration:underline}\
.link.focus{background:#dde8ff;border:2px solid #0047b3}\
li p{margin:2px 0 0 40px;color:#444444;font-size:16px}\
p{margin:0 0 6px 0}\
.para{margin:0 0 18px 0;white-space:normal}".into()
    }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        for (label, verb) in [("Back", "back"), ("Next", "next"), ("Read aloud", "speak")] { t.add(Kind::Button, win, label, "", verb); }
        match self.open {
            None => {
                let vis = self.visible();
                let f = if self.filter.is_empty() { String::new() } else { format!(" · {} match “{}”", vis.len(), self.filter) };
                t.add(Kind::Status, win, &format!("{}{f}", self.status), "", "");
                t.add(Kind::Heading, win, "Conversations", "", "");
                let list = t.add(Kind::List, win, "Conversations", "", "");
                for i in vis {
                    let th = &self.threads[i];
                    let li = t.add(Kind::ListItem, list, "", "", "");
                    t.add(Kind::Link, li, &th.subject, &i.to_string(), "open");
                    t.add(Kind::Label, li, &format!("{} messages · {} · {}", th.count, th.last_date, th.from.join(", ")), "", "");
                }
            }
            Some(k) => {
                let th = &self.threads[k];
                t.add(Kind::Status, win, &format!("Conversation {} of {} · {} messages", k + 1, self.threads.len(), th.count), "", "");
                t.add(Kind::Heading, win, &th.subject, "", "");
                for (from, date, text, how) in &th.msgs {
                    let placed = match how.as_str() { "sift" => " · placed by quoted text", "subject" => " · placed by subject", _ => "" };
                    t.add(Kind::Label, win, &format!("{from} — {date}{placed}"), "", "");
                    for p in text.split("\n\n").map(|p| p.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|p| !p.is_empty()) {
                        t.add(Kind::Paragraph, win, &p, "", "");
                    }
                }
            }
        }
        t
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match (verb, self.open) {
            ("open", _) => if let Ok(i) = arg.trim().parse::<usize>() { if i < self.threads.len() { self.open = Some(i); self.scroll = 0; self.focus_ord = 0; } },
            ("back", Some(_)) => { self.open = None; self.scroll = 0; }
            ("back", None) => { self.scroll -= self.page(); self.clamp(); }
            ("next", Some(k)) => { if k + 1 < self.threads.len() { self.open = Some(k + 1); self.scroll = 0; } }
            ("next", None) => { self.scroll += self.page(); self.clamp(); }
            ("find", _) => { self.filter = arg.trim().to_string(); self.open = None; self.scroll = 0; }
            ("speak", _) => {
                let text = match self.open {
                    Some(k) => self.threads[k].msgs.iter().map(|(f, _, x, _)| format!("{f} wrote: {x}")).collect::<Vec<_>>().join("\n\n"),
                    None => self.visible().iter().take(10).map(|&i| self.threads[i].subject.clone()).collect::<Vec<_>>().join(". "),
                };
                if !self.quiet {
                    if let Ok(mut c) = std::process::Command::new("access").args(["speak", "-"]).stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(mut si) = c.stdin.take() { let _ = si.write_all(text.as_bytes()); }
                    }
                }
            }
            ("select", _) => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            _ => { self.status = format!("“{verb}” is not something Mail does."); }
        }
        let extra = json!({"open": self.open, "filter": self.filter, "scroll": self.scroll});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, _t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        match (key, text) {
            ("PageDown", _) | (_, Some(" ")) => Some(self.act("next", "", "key")),
            ("PageUp", _) | ("Escape", _) => Some(self.act("back", "", "key")),
            _ => None,
        }
    }

    fn state(&self) -> Value {
        json!({"open": self.open, "threads": self.threads.len(), "visible": self.visible().len(), "scroll": self.scroll, "page_h": self.page_h,
               "subjects": self.threads.iter().map(|t| t.subject.clone()).collect::<Vec<_>>(),
               "counts": self.threads.iter().map(|t| t.count).collect::<Vec<_>>()})
    }
}
