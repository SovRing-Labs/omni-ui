// SPDX-License-Identifier: MIT OR Apache-2.0
//! Reader — NEURAL-APPS-0 step 5 on OmniUI: scans, photos of print, PDFs and text through `access read` (OCR stays in the
//! access-pack: tesseract / pdftotext, 99.23 % word accuracy on degraded scans), shown as large print that reflows to the
//! window at every text size. Intents: bigger / smaller (size), next / back (one screen), speak (what is on screen), read
//! <path> (open another document).
use crate::app::{App, Log};
use crate::widgets::{Kind, Tree, NONE};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const SIZES: [i32; 5] = [20, 26, 32, 40, 48];

pub struct ReaderApp {
    pub path: PathBuf,
    pub name: String,
    pub paras: Vec<String>,
    pub status: String,
    pub size_ix: usize,
    pub scroll: i32,
    pub page_h: i32,
    pub view_h: i32,
    focus_ord: usize,
    log: Log,
    quiet: bool,
}

/// `access read <file>` -> its text.txt (tidy paragraphs). The CLI comes from PATH, else the repo copy.
pub fn access_read(path: &Path) -> Result<(String, String), String> {
    let repo_bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../access-pack/bin/access");
    let bin = std::env::var("ACCESS_BIN").map(PathBuf::from).unwrap_or_else(|_| {
        std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("access")).find(|c| c.is_file())).unwrap_or(repo_bin)
    });
    let out = std::process::Command::new(&bin).args(["read", &path.to_string_lossy()]).output().map_err(|e| format!("{}: {e}", bin.display()))?;
    if !out.status.success() { return Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("read failed").to_string()); }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("access read output: {e}"))?;
    let txt = v["txt"].as_str().ok_or("access read: no txt")?;
    let text = std::fs::read_to_string(txt).map_err(|e| e.to_string())?;
    Ok((text, v["method"].as_str().unwrap_or("").to_string()))
}

impl ReaderApp {
    pub fn new() -> ReaderApp {
        ReaderApp { path: PathBuf::new(), name: String::new(), paras: vec![], status: "Open a scan, a PDF or a document.".into(), size_ix: 2,
                    scroll: 0, page_h: 0, view_h: 0, focus_ord: 0, log: Log::open(), quiet: std::env::var("OMNI_UI_QUIET").is_ok() }
    }

    pub fn open(&mut self, path: &Path) {
        self.path = path.to_path_buf();
        self.name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        self.scroll = 0;
        match access_read(path) {
            Ok((text, method)) => {
                self.paras = text.split("\n\n").map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
                let words: usize = self.paras.iter().map(|p| p.split_whitespace().count()).sum();
                self.status = format!("{} — {} words ({})", self.name, words, method);
            }
            Err(e) => { self.paras.clear(); self.status = format!("Could not read {}: {e}", self.name); }
        }
    }

    pub fn px(&self) -> i32 { SIZES[self.size_ix] }
    pub fn words(&self) -> Vec<&str> { self.paras.iter().flat_map(|p| p.split_whitespace()).collect() }
    fn line(&self) -> i32 { self.px() * 3 / 2 }
    fn clamp(&mut self) { self.scroll = self.scroll.clamp(0, (self.page_h - self.view_h).max(0)); }
}

impl App for ReaderApp {
    fn title(&self) -> String { format!("Reader — {}", if self.name.is_empty() { "OmniUI" } else { &self.name }) }
    fn focus_ord(&self) -> usize { self.focus_ord }
    fn set_focus_ord(&mut self, i: usize) { self.focus_ord = i; }
    fn tab_is_intent(&self) -> bool { false } // next / back page here; Tab only walks the buttons
    fn scroll(&self) -> i32 { self.scroll }
    fn set_extent(&mut self, page_h: i32, view_h: i32) { self.page_h = page_h; self.view_h = view_h; self.clamp(); }
    fn wheel(&mut self, dy: i32) { self.scroll += dy; self.clamp(); }
    fn fit(&self) -> Option<(i32, i32)> { Some((self.px(), 56)) } // body padding 28 px each side

    fn css(&self) -> String {
        // the toolbar keeps one size; the document text follows the reader's size (line length reflows to the window)
        format!("body{{margin:0;padding:20px 28px;background:#ffffff;color:#111111;font-family:sans-serif;font-size:{px}px;line-height:1.5}}\
.btn{{display:inline-block;margin:0 10px 0 0;padding:6px 16px;border:2px solid #333333;background:#eeeeee;font-size:20px;line-height:1.3}}\
.btn.focus{{border:3px solid #0047b3;background:#dde8ff}}\
.status{{color:#333333;font-size:17px;line-height:1.3;margin:12px 0 16px 0}}\
h1{{font-size:{h}px;line-height:1.25;margin:0 0 {m}px 0}}\
.para{{margin:0 0 {m}px 0}}", px = self.px(), h = self.px() * 5 / 4, m = self.px() * 3 / 4)
    }

    fn tree(&self) -> Tree {
        let mut t = Tree::default();
        let win = t.add(Kind::Window, NONE, &self.title(), "", "");
        for (label, verb) in [("Smaller", "smaller"), ("Bigger", "bigger"), ("Read aloud", "speak"), ("Back", "back"), ("Next", "next")] {
            t.add(Kind::Button, win, label, "", verb);
        }
        t.add(Kind::Status, win, &format!("{} · text {} px", self.status, self.px()), "", "");
        if !self.name.is_empty() { t.add(Kind::Heading, win, &self.name, "", ""); }
        for p in &self.paras { t.add(Kind::Paragraph, win, p, "", ""); }
        t
    }

    fn act(&mut self, verb: &str, arg: &str, source: &str) -> Value {
        match verb {
            "bigger" | "smaller" => {
                let old = self.px();
                self.size_ix = if verb == "bigger" { (self.size_ix + 1).min(SIZES.len() - 1) } else { self.size_ix.saturating_sub(1) };
                self.scroll = (self.scroll as i64 * self.px() as i64 / old as i64) as i32; // keep the reading place
                self.clamp();
            }
            "next" => { self.scroll += (self.view_h - 2 * self.line()).max(self.line()); self.clamp(); }
            "back" => { self.scroll -= (self.view_h - 2 * self.line()).max(self.line()); self.clamp(); }
            "read" | "open" if !arg.is_empty() => { let p = PathBuf::from(arg); self.open(&p); }
            "speak" => {
                let text = self.paras.join("\n\n");
                if !self.quiet && !text.is_empty() {
                    if let Ok(mut c) = std::process::Command::new("access").args(["speak", "-"]).stdin(std::process::Stdio::piped()).spawn() {
                        use std::io::Write;
                        if let Some(mut si) = c.stdin.take() { let _ = si.write_all(text.as_bytes()); }
                    }
                }
            }
            "select" => { let t = self.tree(); let w = self.focus_widget(&t); if let Some(v) = self.activate(&t, w, source) { return v; } }
            _ => { self.status = format!("“{verb}” is not something Reader does."); }
        }
        let extra = json!({"px": self.px(), "scroll": self.scroll, "page_h": self.page_h});
        self.log.record(verb, arg, source, extra)
    }

    fn app_key(&mut self, _t: &Tree, key: &str, text: Option<&str>) -> Option<Value> {
        let verb = match (key, text) {
            ("PageDown", _) | (_, Some(" ")) => "next",
            ("PageUp", _) => "back",
            (_, Some("+")) | (_, Some("=")) => "bigger",
            (_, Some("-")) => "smaller",
            _ => return None,
        };
        Some(self.act(verb, "", "key"))
    }

    fn state(&self) -> Value {
        json!({"px": self.px(), "scroll": self.scroll, "page_h": self.page_h, "view_h": self.view_h, "paragraphs": self.paras.len(),
               "words": self.words().len(), "status": self.status})
    }
}
