// SPDX-License-Identifier: MIT OR Apache-2.0
//! Find — the first NeuralOS app on OmniUI (NEURAL-APPS-0 step 4's face): a search field over the access-find warm
//! executor (trit index, projects/access-pack/access/find.py).
use crate::app::{App, Log};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Large print and AAA contrast by default: accessibility is the default look, not a mode.
pub const CSS: &str = "body{margin:0;padding:24px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:20px}\
h1{font-size:30px;margin:0 0 16px 0}\
.field{display:inline-block;width:520px;padding:8px 10px;border:2px solid #555555;background:#ffffff;vertical-align:middle}\
.field.focus{border:3px solid #0047b3}\
.ph{color:#595959}\
.caret{display:inline-block;width:2px;height:22px;background:#111111;margin-left:1px;vertical-align:middle}\
.btn{display:inline-block;margin-left:10px;padding:8px 18px;border:2px solid #333333;background:#eeeeee;vertical-align:middle}\
.btn.focus{border:3px solid #0047b3;background:#dde8ff}\
.status{color:#333333;font-size:17px;margin:14px 0 10px 0}\
ol{margin:0;padding:0;list-style:none}\
li{margin:0 0 10px 0}\
.n{display:inline-block;width:34px;color:#333333}\
.link{display:inline;color:#0033aa;text-decoration:underline}\
.link.focus{background:#dde8ff;border:2px solid #0047b3}\
li p{margin:2px 0 0 34px;color:#444444;font-size:16px}";

pub struct FindApp {
    pub query: String,
    pub results: Vec<(String, String)>,
    pub status: String,
    pub focus_ord: usize,
    log: Log,
    pub quiet: bool,
}

pub fn find_sock() -> PathBuf {
    if let Ok(p) = std::env::var("ACCESS_FIND_SOCK") { return p.into(); }
    let home = std::env::var("ACCESS_HOME").map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share/access-pack"));
    home.join("find.sock")
}

impl FindApp {
    pub fn new() -> FindApp {
        FindApp { query: String::new(), results: vec![], status: "Type what you are looking for, then press Enter.".into(), focus_ord: 0,
                  log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() }
    }

    fn search(&mut self, q: &str) {
        let t0 = Instant::now();
        let sock = find_sock();
        let r = (|| -> Result<Value, String> {
            let s = UnixStream::connect(&sock).map_err(|_| "Find is not running — start it with: access find serve".to_string())?;
            s.set_read_timeout(Some(Duration::from_secs(20))).ok();
            (&s).write_all(format!("{}\n", json!({"q": q, "k": 8})).as_bytes()).map_err(|e| e.to_string())?;
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).map_err(|e| e.to_string())?;
            serde_json::from_str(&line).map_err(|e| e.to_string())
        })();
        match r {
            Ok(v) if v.get("error").is_none() => {
                self.results = v["hits"].as_array().map(|a| a.iter().map(|h| {
                    let sn: String = h["snippet"].as_str().unwrap_or("").chars().take(110).collect();
                    (h["path"].as_str().unwrap_or("").to_string(), sn)
                }).collect()).unwrap_or_default();
                self.status = format!("{} results for “{}” ({:.0} ms)", self.results.len(), q, t0.elapsed().as_secs_f64() * 1e3);
            }
            Ok(v) => { self.results.clear(); self.status = format!("Find error: {}", v["error"]); }
            Err(e) => { self.results.clear(); self.status = e; }
        }
    }
}

impl App for FindApp {
    fn title(&self) -> String { "Find — OmniUI".into() }
    fn css(&self) -> String { CSS.into() }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        t.add(Kind::Heading, win, "Find", "", "");
        t.add(Kind::TextInput, win, "Search for", &self.query, "find");
        t.add(Kind::Button, win, "Find", "", "find");
        t.add(Kind::Button, win, "Read aloud", "", "speak");
        t.add(Kind::Status, win, &self.status, "", "");
        if !self.results.is_empty() {
            let list = t.add(Kind::List, win, "Results", "", "");
            for (p, s) in &self.results {
                let li = t.add(Kind::ListItem, list, "", "", "");
                let name = p.rsplit('/').next().unwrap_or(p);
                t.add(Kind::Link, li, name, p, "open");
                if !s.is_empty() { t.add(Kind::Label, li, s, "", ""); }
            }
        }
        t
    }

    fn arg_for(&self, t: &Tree, w: u32, verb: &str) -> String {
        match t.kind[w as usize] { Kind::Link => t.value(w).to_string(), _ if verb == "find" => self.query.clone(), _ => String::new() }
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match verb {
            "find" => { if !arg.trim().is_empty() { self.query = arg.trim().to_string(); } let q = self.query.clone(); self.search(&q); }
            "type" => { self.query = arg.to_string(); }
            "next" | "back" => { let t = self.tree(); self.step_focus(&t, if verb == "next" { 1 } else { -1 }); }
            "select" => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            "speak" => {
                let text = std::iter::once(self.status.clone()).chain(self.results.iter().map(|r| r.0.rsplit('/').next().unwrap_or("").to_string()))
                    .collect::<Vec<_>>().join(". ");
                if !self.quiet { let _ = std::process::Command::new("access").args(["speak", &text]).spawn(); }
            }
            "open" => {
                self.status = format!("Opening {}", arg.rsplit('/').next().unwrap_or(arg));
                if !self.quiet { let _ = std::process::Command::new("xdg-open").arg(arg).spawn(); }
            }
            _ => { self.status = format!("“{verb}” is not something Find does."); }
        }
        let a = if verb == "find" { self.query.clone() } else { arg.to_string() };
        let extra = json!({"results": self.results.len(), "top": self.results.iter().take(5).map(|r| r.0.clone()).collect::<Vec<_>>()});
        self.log.record(verb, &a, source, extra)
    }

    fn app_key(&mut self, t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        let w = self.focus_widget(t);
        match key {
            "Backspace" if t.kind[w as usize] == Kind::TextInput => { self.query.pop(); }
            "Escape" => { self.query.clear(); }
            _ => {
                if t.kind[w as usize] == Kind::TextInput {
                    if let Some(s) = text { if s.chars().all(|c| !c.is_control()) { self.query.push_str(s); } }
                }
            }
        }
        None
    }

    fn state(&self) -> Value {
        json!({"query": self.query, "status": self.status, "results": self.results.iter().map(|r| r.0.clone()).collect::<Vec<_>>()})
    }
}
